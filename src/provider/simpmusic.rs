use std::{
  sync::{Arc, atomic::AtomicUsize},
  time::Duration,
};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::TryFutureExt;
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
#[allow(unused)]
enum ApiSearchResponse {
  Success {
    data: Vec<ApiSearchResponseItem>,
    success: bool,
  },
  Error {
    error: ApiError,
    success: bool,
  },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
#[allow(unused)]
enum ApiLyricsResponse {
  Success {
    data: Vec<ApiLyricsResponseData>,
    success: bool,
  },
  Error {
    error: ApiError,
    success: bool,
  },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(unused)]
struct ApiLyricsResponseData {
  song_title: String,
  artist_name: String,
  album_name: String,
  plain_lyric: String,
  synced_lyrics: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(unused)]
struct ApiSearchResponseItem {
  video_id: String,
  song_title: String,
  artist_name: String,
  duration_seconds: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(unused)]
struct ApiError {
  error: bool,
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
    // Limiting is checked before calling function, so we can safely reset here
    if self.rate_limited_until.load().is_some() {
      self.rate_limited_until.swap(Arc::new(None));
    }

    let search_url = Url::parse_with_params(
      API_SEARCH_URL,
      &[("q", format!("{} {} {}", track.artist_name, track.track_name, track.album_name))],
    )
    .map_err(|e| {
      error!("SimpMusicProvider: Could not parse Track into search URL for {track}: {e}");
      ProviderError::Permanent
    })?;

    debug!("SimpMusicProvider: Searching SimpMusic for {}", &track);
    trace!("SimpMusicProvider: GET request to \"{}\"", &search_url);

    let response = http_client.get(search_url).send().await.map_err(|e| {
      error!("SimpMusicProvider: Error encountered while searching for {track}: {e}");
      ProviderError::Permanent
    })?;
    let response_status = response.status();

    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if response_status == StatusCode::TOO_MANY_REQUESTS {
      return self.handle_too_many_requests(&response, track);
    }

    if let Ok(api_response) = response.json::<ApiSearchResponse>().await.inspect_err(|e| {
      error!("SimpMusicProvider: Failed to parse response while searching for {track}: {e}");
    }) {
      trace!("SimpMusicProvider: API search response:\n{:#?}", &api_response);

      match api_response {
        ApiSearchResponse::Success { data, .. } => {
          if let Some(video_id) = find_best_match(&data, track) {
            debug!(
              "SimpMusicProvider: Found match with video ID {video_id} in {} search results for {track}",
              data.len()
            );

            let get_lyrics_url =
              Url::parse(&format!("{API_BASE_URL}/{video_id}")).map_err(|e| {
                error!("SimpMusicProvider: Could not parse get lyrics URL for {track}: {e}");
                ProviderError::Permanent
              })?;

            debug!("SimpMusicProvider: Getting lyrics for {}", &track);
            trace!("SimpMusicProvider: GET request to \"{}\"", &get_lyrics_url);

            let response = http_client.get(get_lyrics_url).send().await.map_err(|e| {
              error!("SimpMusicProvider: Error encountered while getting lyrics for {track}: {e}");
              ProviderError::Permanent
            })?;

            req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            if response.status() == StatusCode::TOO_MANY_REQUESTS {
              return self.handle_too_many_requests(&response, track);
            }

            if let Ok(api_response) = response.json::<ApiLyricsResponse>().await.inspect_err(|e| {
              error!(
                "SimpMusicProvider: Failed to parse response while getting lyrics for {track}: {e}"
              );
            }) {
              trace!("SimpMusicProvider: API get lyrics response:\n{:#?}", &api_response);

              match api_response {
                ApiLyricsResponse::Success { data, .. } => {
                  if data.len() > 1 {
                    warn!(
                      "SimpMusicProvider: `ApiLyricsResponse.data` for {track} contains {} items when 1 was expected",
                      data.len()
                    );
                  }

                  if let Some(ApiLyricsResponseData {
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

                  error!(
                    "SimpMusicProvider: Failed to parse lyrics data from `Success` response for {track}"
                  );
                  return Err(ProviderError::Permanent);
                }

                ApiLyricsResponse::Error { error, .. } => {
                  return self.handle_error_response(error, track);
                }
              }
            }
          } else {
            debug!(
              "SimpMusicProvider: No good match found in {} search results for {track}",
              data.len()
            );
            return Err(ProviderError::NotFound);
          }
        }

        ApiSearchResponse::Error { error, .. } => return self.handle_error_response(error, track),
      }
    }

    error!("SimpMusicProvider: {response_status} server response while getting lyrics for {track}");
    Err(ProviderError::Permanent)
  }

  fn id(&self) -> ProviderId {
    ProviderId::SimpMusic
  }

  fn is_busy(&self) -> bool {
    self
      .rate_limited_until
      .load()
      .is_some_and(|dt| dt > Utc::now())
  }
}

impl SimpMusicProvider {
  fn handle_error_response(&self, error: ApiError, track: &Track) -> ProviderResult {
    match error {
      ApiError { code: 404, .. } => {
        debug!("SimpMusicProvider: Could not find {track}");
        Err(ProviderError::NotFound)
      }

      ApiError {
        code: 429, reason, ..
      } => {
        debug!("SimpMusicProvider: Rate-limited while fetching {track}: {reason}");
        self
          .rate_limited_until
          .swap(Arc::new(Some(Utc::now() + Duration::from_secs(2))));
        Err(ProviderError::Transient)
      }

      ApiError { code, reason, .. } => {
        warn!("SimpMusicProvider: {code} for {track}: {reason}");
        Err(ProviderError::Permanent)
      }
    }
  }

  fn handle_too_many_requests(&self, response: &Response, track: &Track) -> ProviderResult {
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

    Err(ProviderError::Transient)
  }
}

#[allow(clippy::cast_possible_truncation)]
fn find_best_match(items: &[ApiSearchResponseItem], track: &Track) -> Option<String> {
  items
    .iter()
    .find(|&item| {
      item.artist_name.eq_ignore_ascii_case(&track.artist_name)
        && item.song_title.eq_ignore_ascii_case(&track.track_name)
        && (item.duration_seconds - (track.duration as i32)).abs() < 5
    })
    .map(|item| item.video_id.clone())
}
