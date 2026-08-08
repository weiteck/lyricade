use std::{
  sync::{Arc, atomic::AtomicUsize},
  time::Duration,
};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Response, StatusCode, Url};
use serde::Deserialize;
use tracing::{debug, error, trace, warn};

use crate::{
  lyrics::{Lyrics, LyricsType},
  provider::{LyricsData, Provider, ProviderError, ProviderId, ProviderResult},
  track::Track,
};

const API_BASE_URL: &str = "https://api-lyrics.simpmusic.org/v1";
const API_SEARCH_URL: &str = "https://api-lyrics.simpmusic.org/v1/search";

#[derive(Debug)]
pub(crate) struct SimpMusicProvider {
  rate_limited_until: ArcSwap<Option<DateTime<Utc>>>,
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
    let rate_limited_until = ArcSwap::new(Arc::new(None));

    Self { rate_limited_until }
  }
}

#[async_trait]
impl Provider for SimpMusicProvider {
  async fn fetch(
    &self,
    http_client: reqwest::Client,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult {
    let video_id = self
      .find_video_id(&http_client, &req_counter, track)
      .await?;

    self
      .get_lyrics_for_video_id(&http_client, &req_counter, track, &video_id)
      .await
  }

  fn id(&self) -> ProviderId {
    ProviderId::SimpMusic
  }

  fn is_busy(&self) -> bool {
    if let Some(dt) = self.rate_limited_until.load().as_ref() {
      if dt > &Utc::now() {
        true
      } else {
        trace!("SimpMusicProvider: Rate-limit expired");
        self.rate_limited_until.swap(Arc::new(None));

        false
      }
    } else {
      false
    }
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

    debug!("SimpMusicProvider: {track}: Searching for track");
    trace!("SimpMusicProvider: {track}: GET request to \"{}\"", &search_url);

    let response = http_client.get(search_url).send().await.map_err(|e| {
      error!("SimpMusicProvider: {track}: Error encountered while searching for {track}: {e}");
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
            debug!(
              "SimpMusicProvider: {track}: Found match with video ID {video_id} in {} search results",
              data.len()
            );

            return Ok(video_id);
          }

          debug!(
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

    error!("SimpMusicProvider: {track}: {response_status}");
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

    debug!("SimpMusicProvider: {track}: Getting lyrics for {}", &track);
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

    error!("SimpMusicProvider: {track}: {response_status}");
    Err(ProviderError::Permanent)
  }

  fn handle_error(&self, error: ApiError, track: &Track) -> ProviderError {
    match error {
      ApiError { code: 404, .. } => {
        debug!("SimpMusicProvider: Could not find {track}");
        ProviderError::NotFound
      }

      ApiError {
        code: 429, reason, ..
      } => {
        debug!("SimpMusicProvider: Rate-limited while fetching {track}: {reason}");
        self
          .rate_limited_until
          .swap(Arc::new(Some(Utc::now() + Duration::from_secs(2))));
        ProviderError::Transient
      }

      ApiError { code, reason, .. } => {
        warn!("SimpMusicProvider: {code} for {track}: {reason}");
        ProviderError::Permanent
      }
    }
  }

  fn handle_too_many_requests(&self, response: &Response, track: &Track) -> ProviderError {
    // Set retry delay if 429 too many requests
    let delay = if let Some(v) = response.headers().get("x-rate-limit-retry-after-seconds")
      && let Ok(s) = v.to_str()
      && let Ok(delay) = str::parse::<u64>(s)
    {
      warn!(
        "SimpMusicProvider: Too many requests for {track} - retry-delay of {delay}s requested by server"
      );
      delay
    } else {
      warn!(
        "SimpMusicProvider: Too many requests for {track} - no retry-after header; defaulting to delay of 5s"
      );
      5
    };

    self
      .rate_limited_until
      .swap(Arc::new(Some(Utc::now() + Duration::from_secs(delay))));

    ProviderError::Transient
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
