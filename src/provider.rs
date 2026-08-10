use std::{
  fmt::{Debug, Display},
  sync::{Arc, atomic::AtomicUsize},
};

use async_trait::async_trait;
use diesel::{
  backend::Backend,
  deserialize::{FromSql, FromSqlRow},
  expression::AsExpression,
  serialize::ToSql,
  sql_types::Text,
  sqlite::Sqlite,
};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::{
  lyrics::Lyrics,
  provider::{lrclib::LrcLibProvider, simpmusic::SimpMusicProvider},
  track::Track,
};

pub(crate) mod manager;

mod lrclib;
mod simpmusic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub(crate) enum ProviderId {
  #[default]
  LrcLib,
  SimpMusic,
}

impl ProviderId {
  pub(crate) const ALL: [Self; 2] = [ProviderId::LrcLib, ProviderId::SimpMusic];
}

impl Display for ProviderId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ProviderId::LrcLib => write!(f, "LRCLIB"),
      ProviderId::SimpMusic => write!(f, "SimpMusic"),
    }
  }
}

impl From<&str> for ProviderId {
  fn from(value: &str) -> Self {
    match value {
      "LRCLIB" => ProviderId::LrcLib,
      "SimpMusic" => ProviderId::SimpMusic,
      _ => ProviderId::default(),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderTier {
  Primary,
  Secondary,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProviderState {
  pub(crate) id: ProviderId,
  pub(crate) enabled: bool,
  pub(crate) tier: ProviderTier,
}

impl ProviderId {
  pub(crate) fn init_provider(self) -> Arc<dyn Provider> {
    match self {
      ProviderId::LrcLib => Arc::new(LrcLibProvider::new()),
      ProviderId::SimpMusic => Arc::new(SimpMusicProvider::new()),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, AsExpression, FromSqlRow)]
#[diesel(sql_type = Text)]
pub(crate) struct Providers(pub(crate) Vec<ProviderId>);

impl Default for Providers {
  fn default() -> Self {
    Self(vec![ProviderId::LrcLib, ProviderId::SimpMusic])
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

#[derive(Debug, Clone)]
pub(crate) struct LyricsData {
  pub(crate) instrumental: Option<bool>,
  pub(crate) plain_lyrics: Option<Lyrics>,
  pub(crate) sync_lyrics: Option<Lyrics>,
}

#[derive(Debug, Clone)]
pub(crate) enum ProviderError {
  NotFound,
  Transient,
  Permanent,
}

pub(crate) type ProviderResult = Result<LyricsData, ProviderError>;

#[async_trait]
pub(crate) trait Provider: Debug + Send + Sync {
  #[must_use]
  fn id(&self) -> ProviderId;

  /// Function checks if the `Provider` is busy (rate-limited or no free connections),
  // and resets the stored state if no longer busy.
  fn is_busy(&self) -> bool;

  /// Get lyrics from the API.
  #[must_use]
  async fn fetch(
    &self,
    http_client: reqwest::Client,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult;
}
