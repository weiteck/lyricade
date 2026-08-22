use std::sync::{Arc, atomic::AtomicUsize};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Response, StatusCode, Url};
use serde::Deserialize;
use tokio::sync::Semaphore;
use tracing::{error, trace, warn};

use crate::{
  lyrics::{Lyrics, LyricsType},
  provider::{LyricsData, Provider, ProviderError, ProviderId, ProviderResult, ProviderState},
  track::Track,
};

const API_BASE_URL: &str = "https://api-lyrics.simpmusic.org/v1";
const API_SEARCH_URL: &str = "https://api-lyrics.simpmusic.org/v1/search";

#[derive(Debug)]
pub(crate) struct SimpMusicProvider {
  semaphore: Semaphore,
  state: Arc<ProviderState>,
  rate_limited_until: ArcSwap<Option<DateTime<Utc>>>,
  req_delayed_until: ArcSwap<Option<DateTime<Utc>>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ApiSearchResponse {
  Success {
    data: Vec<ApiSearchResponseItem>,
    // success: bool,
  },
  Error {
    error: ApiError,
    // success: bool,
  },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ApiLyricsResponse {
  Success {
    data: Vec<ApiLyricsResponseItem>,
    // success: bool,
  },
  Error {
    error: ApiError,
    // success: bool,
  },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiLyricsResponseItem {
  // song_title: String,
  // artist_name: String,
  // album_name: String,
  plain_lyric: String,
  synced_lyrics: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiSearchResponseItem {
  video_id: String,
  song_title: String,
  artist_name: String,
  duration_seconds: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiError {
  // error: bool,
  code: i32,
  reason: String,
}

impl SimpMusicProvider {
  pub(crate) fn new() -> Self {
    let semaphore = tokio::sync::Semaphore::new(2);
    let rate_limited_until = ArcSwap::new(Arc::new(None));
    let req_delayed_until = ArcSwap::new(Arc::new(None));
    let state = Arc::new(ProviderState::new(ProviderId::SimpMusic, &semaphore));

    Self {
      semaphore,
      state,
      rate_limited_until,
      req_delayed_until,
    }
  }
}

#[async_trait]
impl Provider for SimpMusicProvider {
  async fn api_fetch(
    &self,
    http_client: reqwest::Client,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult {
    let video_id = self
      .find_video_id(&http_client, &req_counter, track)
      .await?;

    // Sleep for the default request delay between multiple requests
    self.sleep_for_default_req_delay();

    self
      .get_lyrics_for_video_id(&http_client, &req_counter, track, &video_id)
      .await
  }

  fn id(&self) -> ProviderId {
    ProviderId::SimpMusic
  }

  fn state(&self) -> Arc<ProviderState> {
    Arc::clone(&self.state)
  }

  fn state_ref(&self) -> &Arc<ProviderState> {
    &self.state
  }

  fn semaphore(&self) -> &Semaphore {
    &self.semaphore
  }

  fn rate_limited_until(&self) -> &ArcSwap<Option<DateTime<Utc>>> {
    &self.rate_limited_until
  }

  fn req_delayed_until(&self) -> &ArcSwap<Option<DateTime<Utc>>> {
    &self.req_delayed_until
  }

  fn default_req_delay_secs(&self) -> Option<f64> {
    Some(0.2)
  }
}

impl SimpMusicProvider {
  async fn find_video_id(
    &self,
    http_client: &reqwest::Client,
    req_counter: &Arc<AtomicUsize>,
    track: &Track,
  ) -> Result<String, ProviderError> {
    let search_url = Url::parse_with_params(
      API_SEARCH_URL,
      &[("q", format!("{} {} {}", track.artist_name, track.track_name, track.album_name))],
    )
    .map_err(|e| {
      error!("SimpMusicProvider: {track}: Could not build search URL from Track data: {e}");
      ProviderError::Permanent
    })?;

    trace!("SimpMusicProvider: {track}: Step 1/2: Finding matching videoId");
    trace!("SimpMusicProvider: {track}: GET request to \"{}\"", &search_url);

    let response = http_client.get(search_url).send().await.map_err(|e| {
      error!("SimpMusicProvider: {track}: {e}");
      ProviderError::Permanent
    })?;
    let response_status = response.status();

    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if response_status == StatusCode::TOO_MANY_REQUESTS {
      return Err(self.handle_too_many_requests(&response, track));
    }

    if let Ok(api_response) = response.json::<ApiSearchResponse>().await.inspect_err(|e| {
      error!("SimpMusicProvider: {track}: Failed to parse search response: {e}");
    }) {
      trace!("SimpMusicProvider: {track}: API search response:\n{:#?}", &api_response);

      match api_response {
        ApiSearchResponse::Success { mut data, .. } => {
          if let Some(video_id) = find_best_match(&mut data, track) {
            trace!(
              "SimpMusicProvider: {track}: Found match with video ID {video_id} in {} search results",
              data.len()
            );

            return Ok(video_id);
          }

          trace!(
            "SimpMusicProvider: {track}: No good match found in {} search results",
            data.len()
          );
          return Err(ProviderError::NotFound);
        }

        ApiSearchResponse::Error { error, .. } => {
          return Err(self.handle_error(error, track));
        }
      }
    }

    error!("SimpMusicProvider: {track}: Server responded with {response_status}");
    Err(ProviderError::Permanent)
  }

  async fn get_lyrics_for_video_id(
    &self,
    http_client: &reqwest::Client,
    req_counter: &Arc<AtomicUsize>,
    track: &Track,
    video_id: &str,
  ) -> ProviderResult {
    let get_lyrics_url = format!("{API_BASE_URL}/{video_id}");
    let get_lyrics_url = Url::parse(&get_lyrics_url).map_err(|e| {
      error!("SimpMusicProvider: {track}: Could not parse URL from \"{get_lyrics_url}\": {e}");
      ProviderError::Permanent
    })?;

    trace!(
      "SimpMusicProvider: {track}: Step 2/2: Getting lyrics for track with videoId {video_id}"
    );
    trace!("SimpMusicProvider: {track}: GET request to \"{}\"", &get_lyrics_url);

    let response = http_client.get(get_lyrics_url).send().await.map_err(|e| {
      error!("SimpMusicProvider: {track}: Error encountered while getting lyrics for {track}: {e}");
      ProviderError::Permanent
    })?;
    let response_status = response.status();

    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if response_status == StatusCode::TOO_MANY_REQUESTS {
      return Err(self.handle_too_many_requests(&response, track));
    }

    if let Ok(api_response) = response.json::<ApiLyricsResponse>().await.inspect_err(|e| {
      error!("SimpMusicProvider: {track}: Failed to parse get lyrics response: {e}");
    }) {
      trace!("SimpMusicProvider: {track}: API get lyrics response:\n{:#?}", &api_response);

      match api_response {
        ApiLyricsResponse::Success { data, .. } => {
          if data.len() > 1 {
            warn!(
              "SimpMusicProvider: {track}: `ApiLyricsResponse.data` contains {} items when 1 was expected",
              data.len()
            );
          }

          if let Some(ApiLyricsResponseItem {
            plain_lyric,
            synced_lyrics,
            ..
          }) = data.first().cloned()
          {
            let plain_lyric = plain_lyric.trim().to_string();
            let synced_lyrics = synced_lyrics.map(|s| s.trim().to_string());

            let plain_lyrics = if plain_lyric.is_empty() {
              None
            } else {
              Some(Lyrics {
                lyrics_type: LyricsType::Plain,
                contents: plain_lyric,
              })
            };

            let sync_lyrics = if synced_lyrics.as_ref().is_none_or(String::is_empty) {
              None
            } else {
              synced_lyrics.map(|s| Lyrics {
                lyrics_type: LyricsType::Sync,
                contents: s,
              })
            };

            return Ok(LyricsData {
              instrumental: None,
              plain_lyrics,
              sync_lyrics,
            });
          }

          error!("SimpMusicProvider: {track}: Failed to parse lyrics data from `Success` response");
          return Err(ProviderError::Permanent);
        }

        ApiLyricsResponse::Error { error, .. } => {
          return Err(self.handle_error(error, track));
        }
      }
    }

    error!("SimpMusicProvider: {track}: Server responded with {response_status}");
    Err(ProviderError::Permanent)
  }

  fn handle_error(&self, error: ApiError, track: &Track) -> ProviderError {
    match error {
      ApiError { code: 404, .. } => {
        trace!("SimpMusicProvider: {track}: Could not find track");
        ProviderError::NotFound
      }

      ApiError {
        code: 429, reason, ..
      } => {
        warn!("SimpMusicProvider: {track}: API error 429: {reason}");
        self.set_rate_limited(2.0);
        ProviderError::RateLimited
      }

      ApiError { code, reason, .. } => {
        error!("SimpMusicProvider: {track}: API error {code}: {reason}");
        ProviderError::Permanent
      }
    }
  }

  fn handle_too_many_requests(&self, response: &Response, track: &Track) -> ProviderError {
    // Set retry delay if 429 too many requests
    let req_delay = if let Some(v) = response.headers().get("x-rate-limit-retry-after-seconds")
      && let Ok(s) = v.to_str()
      && let Ok(req_delay) = str::parse::<f64>(s)
    {
      warn!(
        "SimpMusicProvider: {track}: Too many requests - retry-delay of {req_delay:.0$}s requested by server",
        if req_delay.fract() >= 0.01 { 2 } else { 0 }
      );
      req_delay
    } else {
      warn!(
        "SimpMusicProvider: {track}: Too many requests - no \"x-rate-limit-retry-after-seconds\" header; defaulting to delay of 5s"
      );
      5.0
    };

    self.set_rate_limited(req_delay);

    ProviderError::RateLimited
  }
}

#[allow(clippy::cast_possible_truncation)]
fn find_best_match(items: &mut [ApiSearchResponseItem], track: &Track) -> Option<String> {
  // Sort by difference to `Track` duration so closest match will be returned
  items.sort_by(|a, b| {
    a.duration_seconds
      .saturating_sub(track.duration as i32)
      .cmp(&b.duration_seconds.saturating_sub(track.duration as i32))
  });

  // Exact match
  items
    .iter()
    .find(|&item| {
      item.artist_name.eq_ignore_ascii_case(&track.artist_name)
        && item.song_title.eq_ignore_ascii_case(&track.track_name)
        && item
          .duration_seconds
          .saturating_sub(track.duration as i32)
          .abs()
          < 5
    })
    // Next closest match
    .or_else(|| {
      items.iter().find(|&item| {
        item
          .artist_name
          .to_ascii_lowercase()
          .starts_with(&track.artist_name.to_lowercase())
          && item
            .song_title
            .to_ascii_lowercase()
            .starts_with(&track.track_name.to_lowercase())
          && item
            .duration_seconds
            .saturating_sub(track.duration as i32)
            .abs()
            < 5
      })
    })
    .map(|item| item.video_id.clone())
}
