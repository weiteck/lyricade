use std::sync::{Arc, atomic::AtomicUsize};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use tokio::sync::Semaphore;
use tracing::{debug, error, trace, warn};

use crate::{
  lyrics::{Lyrics, LyricsType},
  provider::{LyricsData, Provider, ProviderError, ProviderId, ProviderResult, ProviderState},
  track::Track,
};

const API_URL: &str = "https://lrclib.net/api/get";

#[derive(Debug)]
pub(crate) struct LrcLibProvider {
  semaphore: Semaphore,
  rate_limited_until: ArcSwap<Option<DateTime<Utc>>>,
  state: Arc<ProviderState>,
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
    let semaphore = tokio::sync::Semaphore::new(1);
    let rate_limited_until = ArcSwap::new(Arc::new(None));
    let state = Arc::new(ProviderState::new(ProviderId::LrcLib, &semaphore));

    Self {
      semaphore,
      rate_limited_until,
      state,
    }
  }
}

#[async_trait]
impl Provider for LrcLibProvider {
  async fn api_fetch(
    &self,
    http_client: reqwest::Client,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult {
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
      error!("LrcLibProvider: {track}: Could not parse Track into request URL: {e}");
      ProviderError::Permanent
    })?;

    trace!("LrcLibProvider: {track}: GET request to \"{}\"", &url);

    let response = http_client.get(url).send().await.map_err(|e| {
      error!("LrcLibProvider: {track}: {e}");
      ProviderError::Permanent
    })?;
    let response_status = response.status();

    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Return requested retry delay if 429 too many requests
    if response_status == StatusCode::TOO_MANY_REQUESTS {
      if let Some(v) = response.headers().get("Retry-After")
        && let Ok(s) = v.to_str()
        && let Ok(req_delay) = str::parse::<f64>(s)
      {
        warn!(
          "LrcLibProvider: {track}: Too many requests - retry-delay of {req_delay:.0$}s requested by server",
          if req_delay.fract() >= 0.01 { 2 } else { 0 }
        );
        self.set_rate_limited(req_delay);
      } else {
        warn!(
          "LrcLibProvider: {track}: Too many requests - no \"Retry-After\" header; defaulting to delay of 5s"
        );
        self.set_rate_limited(5.0);
      }

      return Err(ProviderError::RateLimited);
    }

    if let Ok(api_response) = response.json::<ApiResponse>().await {
      trace!("LrcLibProvider: {track}: lrclib.net API response:\n{:#?}", &api_response);

      match api_response {
        ApiResponse::Success {
          instrumental,
          plain_lyrics,
          synced_lyrics,
        } => {
          debug!("LrcLibProvider: {track}: Found track");

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
          debug!("LrcLibProvider: {track}: Could not find track");
          return Err(ProviderError::NotFound);
        }

        ApiResponse::Error {
          status_code,
          name,
          message,
        } => {
          warn!("LrcLibProvider: {track}: {status_code} {name}: {message}");
          return Err(ProviderError::Permanent);
        }
      };
    }

    error!("LrcLibProvider: {track}: server responded with {response_status}");
    Err(ProviderError::Permanent)
  }

  fn id(&self) -> ProviderId {
    ProviderId::LrcLib
  }

  fn rate_limited_until(&self) -> &ArcSwap<Option<DateTime<Utc>>> {
    &self.rate_limited_until
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
}
