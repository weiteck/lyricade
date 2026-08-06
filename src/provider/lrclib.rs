use std::{
  sync::{Arc, atomic::AtomicUsize},
  time::Duration,
};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use tokio::sync::Semaphore;
use tracing::{debug, error, trace, warn};

use crate::{
  lyrics::{Lyrics, LyricsType},
  provider::{LyricsData, Provider, ProviderError, ProviderId, ProviderResult},
  track::Track,
};

const API_URL: &str = "https://lrclib.net/api/get";

#[derive(Debug)]
pub(crate) struct LrcLibProvider {
  semaphore: Arc<Semaphore>,
  rate_limited_until: ArcSwap<Option<DateTime<Utc>>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ApiResponse {
  #[serde(rename_all = "camelCase")]
  Success {
    // Unused fields:
    //   id: i64,
    //   name: Option<String>,
    //   track_name: Option<String>,
    //   artist_name: Option<String>,
    //   album_name: Option<String>,
    //   duration: Option<f64>,
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
  },

  #[serde(rename_all = "camelCase")]
  Error {
    status_code: i64,
    name: String,
    message: String,
  },
}

impl LrcLibProvider {
  pub(crate) fn new() -> Self {
    // Implementation Note:
    // https://lrclib.net/docs suggests making sequential requests only and honouring
    // the delay returned in 429 responses in the 'Retry-After' header
    let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
    let rate_limited_until = ArcSwap::new(Arc::new(None));

    Self {
      semaphore,
      rate_limited_until,
    }
  }
}

#[async_trait]
impl Provider for LrcLibProvider {
  async fn fetch(
    &self,
    http_client: reqwest::Client,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult {
    // Limit connections - LrcLib docs suggest sequential-only requests
    match self.semaphore.acquire().await {
      Ok(_) => trace!(
        "LrcLibProvider: Acquired Provider connection semaphore permit ({} available)",
        self.semaphore.available_permits()
      ),
      Err(e) => {
        error!("LrcLibProvider: Error encountered while getting lyrics for {track}: {e}");
        return Err(ProviderError::Permanent);
      }
    }

    // Limiting is checked before calling function, so we can safely reset here
    if self.rate_limited_until.load().is_some() {
      self.rate_limited_until.swap(Arc::new(None));
    }

    let url = Url::parse_with_params(
      API_URL,
      &[
        ("track_name", &track.track_name),
        ("artist_name", &track.artist_name),
        ("album_name", &track.album_name),
        ("duration", &track.duration.to_string()),
      ],
    )
    .map_err(|e| {
      error!("LrcLibProvider: Could not parse Track into request URL for {track}: {e}");
      ProviderError::Permanent
    })?;

    debug!("LrcLibProvider: Getting lyrics from lrclib.net for {}", &track);
    trace!("LrcLibProvider: GET request to \"{}\"", &url);

    let response = http_client.get(url).send().await.map_err(|e| {
      error!("LrcLibProvider: Error encountered while getting lyrics for {track}: {e}");
      ProviderError::Permanent
    })?;
    let response_status = response.status();

    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Return requested retry delay if 429 too many requests
    if response_status == StatusCode::TOO_MANY_REQUESTS
      && let Some(v) = response.headers().get("Retry-After")
      && let Ok(s) = v.to_str()
      && let Ok(delay) = str::parse::<u64>(s)
    {
      warn!(
        "LrcLibProvider: Too many requests for {track} - retry-delay of {delay}s requested by server"
      );
      self
        .rate_limited_until
        .swap(Arc::new(Some(Utc::now() + Duration::from_secs(delay))));
      return Err(ProviderError::Transient);
    }

    if let Ok(api_response) = response.json::<ApiResponse>().await {
      trace!("LrcLibProvider: lrclib.net API response:\n{:#?}", &api_response);

      match api_response {
        ApiResponse::Success {
          instrumental,
          plain_lyrics,
          synced_lyrics,
        } => {
          debug!("LrcLibProvider: Found {track}");

          return Ok(LyricsData {
            instrumental: if instrumental { Some(true) } else { None },
            plain_lyrics: plain_lyrics.map(|s| Lyrics {
              lyrics_type: LyricsType::Plain,
              contents: s,
            }),
            sync_lyrics: synced_lyrics.map(|s| Lyrics {
              lyrics_type: LyricsType::Sync,
              contents: s,
            }),
          });
        }

        ApiResponse::Error {
          status_code: 404, ..
        } => {
          debug!("LrcLibProvider: Could not find {track}");
          return Err(ProviderError::NotFound);
        }

        ApiResponse::Error {
          status_code,
          name,
          message,
        } => {
          warn!("LrcLibProvider: {status_code} {name} for {track}: {message}");
          return Err(ProviderError::Permanent);
        }
      };
    }

    error!("LrcLibProvider: {response_status} server response while getting lyrics for {track}");
    Err(ProviderError::Permanent)
  }

  fn id(&self) -> ProviderId {
    ProviderId::LrcLib
  }

  fn is_busy(&self) -> bool {
    self.semaphore.available_permits() == 0
      || self
        .rate_limited_until
        .load()
        .is_some_and(|dt| dt > Utc::now())
  }
}
