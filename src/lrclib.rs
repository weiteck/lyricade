const API_BASE_URL: &str = "https://lrclib.net/api";
static API_URL_GET_LYRICS_FROM_TRACK_SIGNATURE: LazyLock<String> =
  LazyLock::new(|| format!("{}/get", API_BASE_URL));

use std::{
  sync::{Arc, LazyLock, atomic::AtomicUsize},
  time::Duration,
};

use anyhow::anyhow;
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use tokio::task::AbortHandle;
use tracing::{debug, error, trace, warn};

use crate::{
  Result,
  lyrics::{Lyrics, LyricsType},
  settings::CONNECTION_LIMIT,
  track::Track,
  util::now,
};

#[derive(Debug, Clone)]
pub struct LrcLibLyricsResponse {
  pub instrumental: bool,
  pub plain_lyrics: Option<Lyrics>,
  pub synced_lyrics: Option<Lyrics>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(unused)]
#[serde(untagged)]
enum ApiResponse {
  #[serde(rename_all = "camelCase")]
  Success {
    id: i64,
    name: Option<String>,
    track_name: Option<String>,
    artist_name: Option<String>,
    album_name: Option<String>,
    duration: Option<f64>,
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
  },
  Error {
    code: i64,
    name: String,
    message: String,
  },
}

#[derive(Debug, Clone)]
pub struct LrcLibClient {
  http_client: reqwest::Client,
  limiter: Arc<leaky_bucket::RateLimiter>,
  semaphore: Arc<tokio::sync::Semaphore>,
  completed_requests: Arc<AtomicUsize>,
  req_rate_logger_abort_handle: AbortHandle,
}

impl LrcLibClient {
  pub fn new() -> Self {
    let user_agent = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

    let http_client = match reqwest::Client::builder()
      .tls_backend_rustls()
      .timeout(Duration::from_secs(30))
      .read_timeout(Duration::from_secs(15))
      .user_agent(user_agent)
      .build()
    {
      Ok(c) => c,
      Err(e) => panic!("Error creating HTTP client: {e}"),
    };

    let limiter = Arc::new(
      leaky_bucket::RateLimiter::builder()
        .initial(4)
        .max(16)
        .refill(2)
        .interval(Duration::from_millis(150))
        .build(),
    );

    let completed_requests = Arc::new(AtomicUsize::new(0));

    let req_rate_logger_abort_handle = spawn_request_rate_logger(Arc::clone(&completed_requests));

    Self {
      http_client,
      limiter,
      semaphore: Arc::new(tokio::sync::Semaphore::new(CONNECTION_LIMIT)),
      completed_requests,
      req_rate_logger_abort_handle,
    }
  }

  pub fn default() -> Self {
    Self::new()
  }

  pub async fn lyrics_from_track_signature(
    &self,
    track: &mut Track,
  ) -> Result<LrcLibLyricsResponse> {
    let url = Url::parse_with_params(
      &API_URL_GET_LYRICS_FROM_TRACK_SIGNATURE,
      &[
        ("track_name", &track.track_name),
        ("artist_name", &track.artist_name),
        ("album_name", &track.album_name),
        ("duration", &track.duration.to_string()),
      ],
    )?;
    let url_str = url.as_str();

    debug!("Getting lyrics from lrclib.net for {}", &track);
    trace!("GET request to \"{}\"", &url);

    // Try 5 req at 1 req/sec if API is rate-limited
    let mut attempts = 0;
    let (response_status, response) = loop {
      // Limit concurrent connections
      let permit = self
        .semaphore
        .acquire()
        .await
        .expect("semaphore unexpectedly closed");
      trace!(
        "Acquired connection permit from semaphore; {} connections free",
        CONNECTION_LIMIT.saturating_sub(self.semaphore.available_permits())
      );

      // Rate limit requests
      self.limiter.acquire_one().await;
      trace!(
        "Acquired token from rate-limiter bucket; {} tokens remaining",
        self.limiter.balance()
      );

      let response = self.http_client.get(url_str).send().await?;

      attempts += 1;
      self
        .completed_requests
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

      if response.status() == StatusCode::TOO_MANY_REQUESTS && attempts < 5 {
        // Retry in 1s
        drop(permit);
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        continue;
      } else {
        break (response.status(), response);
      };
    };

    if let Ok(api_response) = response.json::<ApiResponse>().await {
      trace!("lrclib.net API response:\n{:#?}", &api_response);

      track.last_api_check_at = Some(now());

      match api_response {
        ApiResponse::Success {
          instrumental,
          plain_lyrics,
          synced_lyrics,
          ..
        } => {
          return Ok(LrcLibLyricsResponse {
            instrumental,
            plain_lyrics: plain_lyrics.map(|s| Lyrics {
              lyrics_type: LyricsType::Plain,
              contents: s,
            }),
            synced_lyrics: synced_lyrics.map(|s| Lyrics {
              lyrics_type: LyricsType::Sync,
              contents: s,
            }),
          });
        }
        ApiResponse::Error {
          code,
          name,
          message,
        } => {
          let response = format_args!(
            "lrclib.net API responded with error while getting lyrics for {}:\n\"{code}: {name}: {message}\"",
            &track
          );
          return Err(anyhow!("{response}")).inspect_err(|e| warn!("{e}"));
        }
      };
    }

    let error = format_args!(
      "lrclib.net server responded with status code {} while getting lyrics for {}",
      &response_status, &track
    );
    error!("{error}");
    Err(anyhow!("{error}"))
  }

  pub fn shutdown_request_rate_logger(&self) {
    self.req_rate_logger_abort_handle.abort();
    trace!("Aborted LrcLibClient request_rate_logger task");
  }
}

/// Spawn background worker to log HTTP request rate.
fn spawn_request_rate_logger(counter: Arc<AtomicUsize>) -> AbortHandle {
  tokio::spawn(async move {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    let mut last_count = 0;

    loop {
      interval.tick().await;
      trace!("Tick: LrcLibClient request_rate_logger");

      let count = counter.load(std::sync::atomic::Ordering::Relaxed);
      let delta = count.saturating_sub(last_count);
      last_count = count;

      if delta > 0 {
        debug!(
          "Current completed request rate: {} req/sec ({} total requests)",
          delta, count
        );
      }
    }
  })
  .abort_handle()
}
