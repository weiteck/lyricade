use std::{
  fmt::{Debug, Display},
  sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
  },
  time::Duration,
};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use diesel::{
  backend::Backend,
  deserialize::{FromSql, FromSqlRow},
  expression::AsExpression,
  serialize::ToSql,
  sql_types::Text,
  sqlite::Sqlite,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Semaphore, SemaphorePermit, TryAcquireError};
use tracing::{debug, error, trace};

use crate::{
  lyrics::Lyrics,
  provider::{genius::GeniusProvider, lrclib::LrcLibProvider, simpmusic::SimpMusicProvider},
  track::Track,
};

pub(crate) mod manager;

mod genius;
mod lrclib;
mod simpmusic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub(crate) enum ProviderId {
  #[default]
  LrcLib,
  SimpMusic,
  Genius,
}

impl ProviderId {
  pub(crate) const ALL: [Self; 3] = [
    ProviderId::LrcLib,
    ProviderId::SimpMusic,
    ProviderId::Genius,
  ];
}

impl Display for ProviderId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ProviderId::LrcLib => write!(f, "LRCLIB"),
      ProviderId::SimpMusic => write!(f, "SimpMusic"),
      ProviderId::Genius => write!(f, "Genius"),
    }
  }
}

impl From<&str> for ProviderId {
  fn from(value: &str) -> Self {
    match value {
      "LRCLIB" => ProviderId::LrcLib,
      "SimpMusic" => ProviderId::SimpMusic,
      "Genius" => ProviderId::Genius,
      _ => ProviderId::default(),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderTier {
  Primary,
  Secondary,
}

impl ProviderId {
  pub(crate) fn init_provider(self) -> Arc<dyn Provider> {
    match self {
      ProviderId::LrcLib => Arc::new(LrcLibProvider::new()),
      ProviderId::SimpMusic => Arc::new(SimpMusicProvider::new()),
      ProviderId::Genius => Arc::new(GeniusProvider::new()),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, AsExpression, FromSqlRow)]
#[diesel(sql_type = Text)]
pub(crate) struct Providers(pub(crate) Vec<ProviderId>);

impl Default for Providers {
  fn default() -> Self {
    Self(vec![
      ProviderId::LrcLib,
      ProviderId::SimpMusic,
      ProviderId::Genius,
    ])
  }
}

impl FromSql<Text, Sqlite> for Providers {
  fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> diesel::deserialize::Result<Self> {
    let s = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?;
    Ok(
      ron::from_str::<Vec<ProviderId>>(&s)
        .map(Providers)
        .inspect_err(|error| {
          error!("Error deserialising `Providers` from database value \"{s}\"; using default value: {error}");
        })
        .unwrap_or_default(),
    )
  }
}

impl ToSql<Text, Sqlite> for Providers {
  fn to_sql<'b>(
    &'b self,
    out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
  ) -> diesel::serialize::Result {
    out.set_value(ron::to_string(&self.0)?);
    Ok(diesel::serialize::IsNull::No)
  }
}

#[derive(Debug)]
pub(crate) struct ProviderState {
  pub(crate) id: ProviderId,
  pub(crate) total_requests: AtomicUsize,
  pub(crate) current_requests: AtomicUsize,
  pub(crate) total_permits: AtomicUsize,
  pub(crate) available_permits: AtomicUsize,
  pub(crate) rate_limited: AtomicBool,
}

impl ProviderState {
  fn new(id: ProviderId, semaphore: &Semaphore) -> Self {
    let permits = semaphore.available_permits();

    Self {
      id,
      total_requests: AtomicUsize::new(0),
      current_requests: AtomicUsize::new(0),
      total_permits: AtomicUsize::new(permits),
      available_permits: AtomicUsize::new(permits),
      rate_limited: AtomicBool::new(false),
    }
  }
}

#[derive(Debug, Clone)]
pub(crate) struct LyricsData {
  pub(crate) instrumental: Option<bool>,
  pub(crate) plain_lyrics: Option<Lyrics>,
  pub(crate) sync_lyrics: Option<Lyrics>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProviderError {
  /// Connection semaphore permits exhausted.
  NoConnections,
  /// Provider is not allowing new requests until the delay between requests has expired.
  Delayed,
  /// Server has returned a 429 error, so the `Provider` is temporarily not allowing new connections.
  RateLimited,
  /// Server returned a 404 error, or otherwise that indicated lyrics were not found.
  NotFound,
  /// Server returned an error other than 404 or 429.
  Permanent,
}

pub(crate) type ProviderResult = Result<LyricsData, ProviderError>;

#[async_trait]
pub(crate) trait Provider: Debug + Send + Sync {
  #[must_use]
  fn id(&self) -> ProviderId;

  fn state(&self) -> Arc<ProviderState>;

  fn state_ref(&self) -> &Arc<ProviderState>;

  fn semaphore(&self) -> &Semaphore;

  fn rate_limited_until(&self) -> &ArcSwap<Option<DateTime<Utc>>>;

  fn req_delayed_until(&self) -> &ArcSwap<Option<DateTime<Utc>>>;

  /// The internal fetch implementation.
  #[must_use]
  async fn api_fetch(
    &self,
    http_client: reqwest::Client,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult;

  /// Get lyrics from the API. Wraps the actual implementation.
  #[must_use]
  async fn fetch(
    &self,
    http_client: reqwest::Client,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult {
    let permit = self.fetch_begin()?;

    trace!("{}Provider: {track}: Requesting lyrics", self.id());

    let result = self.api_fetch(http_client, req_counter, track).await;

    self.fetch_end(permit);

    result
  }

  fn fetch_begin(&self) -> Result<SemaphorePermit<'_>, ProviderError> {
    let permit = self.acquire_conn_permit()?;

    self.check_rate_limited()?;
    self.check_delayed()?;

    let state = self.state_ref();
    state.total_requests.fetch_add(1, Ordering::Relaxed);
    state.current_requests.fetch_add(1, Ordering::Relaxed);
    state
      .available_permits
      .store(self.semaphore().available_permits(), Ordering::Relaxed);

    Ok(permit)
  }

  fn fetch_end(&self, permit: SemaphorePermit) {
    drop(permit);

    self.set_req_delay(self.default_req_delay_secs());

    let state = self.state_ref();
    state.current_requests.fetch_sub(1, Ordering::Relaxed);
    state
      .available_permits
      .store(self.semaphore().available_permits(), Ordering::Relaxed);
  }

  fn acquire_conn_permit(&self) -> Result<SemaphorePermit<'_>, ProviderError> {
    let semaphore = self.semaphore();

    match semaphore.try_acquire() {
      Ok(permit) => Ok(permit),
      Err(e) => match e {
        TryAcquireError::Closed => {
          error!("{}Provider: Error encountered while acquiring connection permit: {e}", self.id());
          Err(ProviderError::Permanent)
        }
        TryAcquireError::NoPermits => Err(ProviderError::NoConnections),
      },
    }
  }

  fn check_rate_limited(&self) -> Result<(), ProviderError> {
    let rate_limited_until = self.rate_limited_until();

    let res = if let Some(dt) = rate_limited_until.load().as_ref() {
      if dt > &Utc::now() {
        Err(ProviderError::RateLimited)
      } else {
        debug!("{}Provider: Rate-limit expired", self.id());
        rate_limited_until.swap(Arc::new(None));

        Ok(())
      }
    } else {
      Ok(())
    };

    let state = self.state_ref();
    let rate_limited = state.rate_limited.load(Ordering::Relaxed);
    if res.is_ok() && rate_limited {
      state.rate_limited.store(false, Ordering::Relaxed);
    } else if res.is_err() && !rate_limited {
      state.rate_limited.store(true, Ordering::Relaxed);
    }

    res
  }

  fn check_delayed(&self) -> Result<(), ProviderError> {
    let no_requests_until = self.req_delayed_until();

    if let Some(dt) = no_requests_until.load().as_ref() {
      if dt > &Utc::now() {
        Err(ProviderError::Delayed)
      } else {
        trace!("{}Provider: Connection delay expired", self.id());
        no_requests_until.swap(Arc::new(None));

        Ok(())
      }
    } else {
      Ok(())
    }
  }

  fn set_rate_limited(&self, secs: f64) {
    let expires_at = Utc::now() + Duration::from_secs_f64(secs);
    self.rate_limited_until().swap(Arc::new(Some(expires_at)));

    let state = self.state_ref();
    state.rate_limited.store(true, Ordering::Relaxed);
  }

  fn set_req_delay(&self, secs: Option<f64>) {
    if let Some(secs) = secs {
      let expires_at = Utc::now() + Duration::from_secs_f64(secs);
      self.req_delayed_until().swap(Arc::new(Some(expires_at)));
    }
  }

  fn default_req_delay_secs(&self) -> Option<f64> {
    None
  }

  fn sleep_for_default_req_delay(&self) {
    if let Some(secs) = self.default_req_delay_secs() {
      std::thread::sleep(Duration::from_secs_f64(secs));
      trace!("{}Provider: Connection delay expired (thread slept)", self.id());
    }
  }
}
