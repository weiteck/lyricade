use std::{
  collections::HashSet,
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::Duration,
};

use arc_swap::ArcSwap;
use chrono::Utc;
use reqwest::Client as HttpClient;
use tokio::{sync::Semaphore, task::JoinHandle, time::interval};
use tokio_util::sync::CancellationToken;

use tracing::{debug, error, info, trace, warn};

use crate::{
  PROVIDERS, SETTINGS, USER_AGENT,
  lyrics::LyricsType,
  provider::{LyricsData, Provider, ProviderError, ProviderId, ProviderState, Providers},
  settings::CONNECTION_LIMIT,
  track::Track,
};

#[derive(Debug)]
pub(crate) struct ProviderManager {
  providers: ArcSwap<Vec<Arc<dyn Provider>>>,
  primary_providers_order: ArcSwap<Vec<ProviderId>>,
  secondary_providers_order: ArcSwap<Vec<ProviderId>>,
  provider_maintenance_task: ArcSwap<JoinHandle<()>>,
  http_client: HttpClient,
  semaphore: Arc<Semaphore>,
  completed_requests: Arc<AtomicUsize>,
  preferred_lyrics: ArcSwap<LyricsType>,
}

impl ProviderManager {
  #[must_use]
  pub(crate) fn new() -> Self {
    let (primary_providers_order, secondary_providers_order) = get_provider_order();
    let providers = init_providers(&primary_providers_order, &secondary_providers_order);

    // Spawn background worker to check for expired rate-limits, which is needed in case a
    // Provider is in a rate-limited state and not tried for a while, causing the UI showing
    // Provider state to be stale
    let providers_cloned = providers.clone();
    let provider_maintenance_task =
      ArcSwap::new(Arc::new(spawn_provider_maintenance_task(providers_cloned)));

    let providers = ArcSwap::new(Arc::new(providers));
    let primary_providers_order = ArcSwap::new(Arc::new(primary_providers_order));
    let secondary_providers_order = ArcSwap::new(Arc::new(secondary_providers_order));

    let http_client = match reqwest::Client::builder()
      .tls_backend_rustls()
      .timeout(Duration::from_secs(10))
      .read_timeout(Duration::from_secs(10))
      .user_agent(&*USER_AGENT)
      .build()
    {
      Ok(c) => c,
      Err(e) => panic!("Failed to initialise HTTP client: {e}"),
    };

    let semaphore = Arc::new(Semaphore::new(CONNECTION_LIMIT));

    let completed_requests = Arc::new(AtomicUsize::new(0));

    let preferred_lyrics = SETTINGS
      .read()
      .map(|settings| settings.prefer_lyrics_type)
      .unwrap_or_default();
    let preferred_lyrics = ArcSwap::new(Arc::new(preferred_lyrics));

    Self {
      providers,
      primary_providers_order,
      secondary_providers_order,
      provider_maintenance_task,
      http_client,
      semaphore,
      completed_requests,
      preferred_lyrics,
    }
  }

  pub(crate) async fn fetch(
    &self,
    track: &Track,
    cancel_token: CancellationToken,
  ) -> Option<LyricsData> {
    let providers = self.providers.load();
    let preferred_lyrics_type = self.preferred_lyrics.load();

    let mut primary_not_checked = self
      .primary_providers_order()
      .iter()
      .copied()
      .collect::<HashSet<_>>();
    let mut secondary_not_checked = self
      .secondary_providers_order()
      .iter()
      .copied()
      .collect::<HashSet<_>>();

    if primary_not_checked.is_empty() {
      // This should never happen
      error!("ProviderManager: {track}: Cannot fetch lyrics without a Primary Provider");
      return None;
    }

    let mut result: Option<LyricsData> = None;

    // Limit concurrent connections
    let _permit = self
      .semaphore
      .acquire()
      .await
      .expect("semaphore unexpectedly closed");
    trace!(
      "ProviderManager: {track}: Acquired connection semaphore permit ({} available)",
      self.semaphore.available_permits()
    );

    let mut interval = interval(Duration::from_millis(20));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.reset();

    // We loop through each Provider and return only when one of the following is true:
    // (1) we have the lyrics type requested;
    // (2) a Provider says the Track is instrumental; or
    // (3) all *primary* Providers have returned a result.
    // Secondary providers are a fallback for when primary Providers are busy
    loop {
      for provider in providers.iter() {
        let id = provider.id();

        if !primary_not_checked.contains(&id) && !secondary_not_checked.contains(&id) {
          continue;
        }

        result = match provider
          .fetch(self.http_client.clone(), Arc::clone(&self.completed_requests), track)
          .await
        {
          Ok(data) => {
            if data.instrumental == Some(true) {
              debug!("ProviderManager: {track}: Got \"instrumental\" flag from {}", provider.id());
            }
            if data.plain_lyrics.is_some() {
              debug!("ProviderManager: {track}: Got plain lyrics from {}", provider.id());
            }
            if data.sync_lyrics.is_some() {
              debug!("ProviderManager: {track}: Got sync lyrics from {}", provider.id());
            }

            primary_not_checked.remove(&id);
            secondary_not_checked.remove(&id);

            if let Some(ex_data) = result {
              Some(LyricsData {
                instrumental: ex_data.instrumental.or(data.instrumental),
                plain_lyrics: ex_data.plain_lyrics.or(data.plain_lyrics),
                sync_lyrics: ex_data.sync_lyrics.or(data.sync_lyrics),
              })
            } else {
              Some(data)
            }
          }

          Err(e) => match e {
            ProviderError::NotFound | ProviderError::Permanent => {
              debug!("ProviderManager: {track}: No lyrics found from {}", provider.id());

              primary_not_checked.remove(&id);
              secondary_not_checked.remove(&id);

              None
            }
            ProviderError::Delayed | ProviderError::RateLimited | ProviderError::NoConnections => {
              None
            }
          },
        };

        // Return the result if we have the lyrics type requested, or the track is instrumental
        if result.as_ref().is_some_and(|data| {
          data.sync_lyrics.is_some()
            || preferred_lyrics_type.as_ref() == &LyricsType::Plain && data.plain_lyrics.is_some()
            || data.instrumental == Some(true)
        }) {
          return result;
        }

        // Checked all primary Providers - return any lyrics we have, even if not preferred type
        if primary_not_checked.is_empty() {
          if result.as_ref().is_some_and(|data| {
            data.plain_lyrics.is_some() || data.sync_lyrics.is_some() || data.instrumental.is_some()
          }) {
            return result;
          }

          return None;
        }
      }

      // Wait for next loop interval or cancellation
      tokio::select! {
        _ = interval.tick() => {}

        () = cancel_token.cancelled() => {
          trace!("ProviderManager: {track}: Fetch lyrics cancelled");
          return None;
        }
      }
    }
  }

  pub(crate) fn reinitialise(&self) {
    // Replace Providers
    let (primary_providers_order, secondary_providers_order) = get_provider_order();
    self
      .primary_providers_order
      .swap(Arc::new(primary_providers_order));
    self
      .secondary_providers_order
      .swap(Arc::new(secondary_providers_order));

    let providers =
      init_providers(&self.primary_providers_order(), &self.secondary_providers_order());

    // Replace background worker to check for expired rate-limits
    self.provider_maintenance_task.load().abort();
    let jh = spawn_provider_maintenance_task(providers.clone());
    self.provider_maintenance_task.store(Arc::new(jh));

    self.providers.store(Arc::new(providers));

    // Replace preferred lyrics
    let preferred_lyrics = SETTINGS
      .read()
      .map(|settings| Arc::new(settings.prefer_lyrics_type))
      .unwrap_or(self.preferred_lyrics.load().clone());
    self.preferred_lyrics.store(preferred_lyrics);
  }

  pub(crate) fn primary_providers_order(&self) -> Vec<ProviderId> {
    self.primary_providers_order.load().to_vec()
  }

  pub(crate) fn secondary_providers_order(&self) -> Vec<ProviderId> {
    self.secondary_providers_order.load().to_vec()
  }

  pub(crate) fn provider_state(&self) -> Vec<Arc<ProviderState>> {
    self.providers.load().iter().map(|p| p.state()).collect()
  }
}

fn get_provider_order() -> (Vec<ProviderId>, Vec<ProviderId>) {
  PROVIDERS
    .read()
    .inspect_err(|_| error!("Providers lock was poisoned while initialising lyrics providers"))
    .map_or_else(
      |_| {
        warn!("Initialising default Providers");
        let default = Providers::default();
        (
          default
            .primary
            .iter()
            .filter(|ps| ps.enabled)
            .map(|ps| ps.id)
            .collect::<Vec<_>>(),
          default
            .secondary
            .iter()
            .filter(|ps| ps.enabled)
            .map(|ps| ps.id)
            .collect::<Vec<_>>(),
        )
      },
      |g| {
        (
          g.primary
            .iter()
            .filter(|ps| ps.enabled)
            .map(|ps| ps.id)
            .collect::<Vec<_>>(),
          g.secondary
            .iter()
            .filter(|ps| ps.enabled)
            .map(|ps| ps.id)
            .collect::<Vec<_>>(),
        )
      },
    )
}

fn init_providers(
  primary_order: &[ProviderId],
  secondary_order: &[ProviderId],
) -> Vec<Arc<dyn Provider>> {
  // Ensure there are no duplicate `ProviderId`s, then construct `Provider`s
  let mut primary_set = HashSet::with_capacity(primary_order.len());
  let mut secondary_set = HashSet::with_capacity(secondary_order.len());
  let primary_providers = primary_order
    .iter()
    .filter(|&id| primary_set.insert(id))
    .map(|id| id.init_provider())
    .collect::<Vec<_>>();
  let secondary_providers = secondary_order
    .iter()
    .filter(|&id| !primary_set.contains(&id) && secondary_set.insert(id))
    .map(|id| id.init_provider())
    .collect::<Vec<_>>();

  let providers = primary_providers
    .into_iter()
    .chain(secondary_providers)
    .collect::<Vec<_>>();

  info!(
    "Initialised lyrics providers: {:#?}",
    providers.iter().map(|p| p.id()).collect::<Vec<_>>()
  );

  providers
}

fn spawn_provider_maintenance_task(
  providers: Vec<Arc<dyn Provider>>,
) -> tokio::task::JoinHandle<()> {
  tokio::spawn(async move {
    let mut interval = interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.reset();

    loop {
      for provider in &providers {
        let state = provider.state_ref();
        let rate_limited_until = provider.rate_limited_until();

        if state.rate_limited.load(Ordering::Relaxed)
          && rate_limited_until
            .load()
            .as_ref()
            .is_some_and(|expires_at| expires_at < Utc::now())
        {
          debug!("{}: Resetting stale expired rate-limit", provider.id());
          rate_limited_until.store(Arc::new(None));
        }
      }

      interval.tick().await;
    }
  })
}
