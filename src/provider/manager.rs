use std::{
  collections::HashSet,
  sync::{Arc, atomic::AtomicUsize},
  time::Duration,
};

use arc_swap::ArcSwap;
use reqwest::Client as HttpClient;
use tokio::sync::Semaphore;
use tracing::{error, info, trace};

use crate::{
  SETTINGS, USER_AGENT,
  lyrics::LyricsType,
  provider::{LyricsData, Provider, ProviderError, ProviderId},
  settings::{CONNECTION_LIMIT, Settings},
  track::Track,
};

#[derive(Debug)]
pub(crate) struct ProviderManager {
  providers: ArcSwap<Vec<Arc<dyn Provider>>>,
  primary_providers_order: ArcSwap<Vec<ProviderId>>,
  secondary_providers_order: ArcSwap<Vec<ProviderId>>,
  http_client: HttpClient,
  semaphore: Arc<Semaphore>,
  completed_requests: Arc<AtomicUsize>,
  preferred_lyrics: ArcSwap<LyricsType>,
}

impl ProviderManager {
  #[must_use]
  pub(crate) fn new() -> Self {
    let (primary_providers_order, secondary_providers_order) = SETTINGS
      .read()
      .inspect_err(|_| error!("Settings lock was poisoned while initialising lyrics providers"))
      .map_or_else(
        |_| (Settings::default().primary_providers, Settings::default().secondary_providers),
        |g| (g.primary_providers.clone(), g.secondary_providers.clone()),
      );

    let providers = init_providers(&primary_providers_order.0, &secondary_providers_order.0);
    let providers = ArcSwap::new(Arc::new(providers));
    let primary_providers_order = ArcSwap::new(Arc::new(primary_providers_order.0));
    let secondary_providers_order = ArcSwap::new(Arc::new(secondary_providers_order.0));

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
      http_client,
      semaphore,
      completed_requests,
      preferred_lyrics,
    }
  }

  pub(crate) async fn fetch(&self, track: &Track) -> Option<LyricsData> {
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

    let mut result: Option<LyricsData> = None;

    // Limit concurrent connections
    let _permit = self
      .semaphore
      .acquire()
      .await
      .expect("semaphore unexpectedly closed");
    trace!(
      "Acquired connection semaphore permit ({} available)",
      CONNECTION_LIMIT.saturating_sub(self.semaphore.available_permits())
    );

    // We loop through each Provider and return only when one of the following is true:
    // (1) we have the lyrics type requested;
    // (2) a Provider says the Track is instrumental; or
    // (3) all *primary* Providers have returned a result.
    // Secondary providers are a fallback for when primary Providers are busy
    loop {
      for provider in providers.iter() {
        let id = provider.id();

        if (!primary_not_checked.contains(&id) && !secondary_not_checked.contains(&id))
          || provider.is_busy()
        {
          continue;
        }

        trace!("Attempting to fetch lyrics from provider {} for {track}", provider.id());

        result = match provider
          .fetch(self.http_client.clone(), Arc::clone(&self.completed_requests), track)
          .await
        {
          Ok(new_data) => {
            primary_not_checked.remove(&id);
            secondary_not_checked.remove(&id);
            if let Some(ex_data) = result {
              Some(LyricsData {
                instrumental: ex_data.instrumental.or(new_data.instrumental),
                plain_lyrics: ex_data.plain_lyrics.or(new_data.plain_lyrics),
                sync_lyrics: ex_data.sync_lyrics.or(new_data.sync_lyrics),
              })
            } else {
              Some(new_data)
            }
          }

          Err(e) => match e {
            ProviderError::NotFound | ProviderError::Permanent => {
              primary_not_checked.remove(&id);
              secondary_not_checked.remove(&id);
              None
            }
            ProviderError::Transient => None,
          },
        };

        // Return the result if we have the lyrics type requested, or the track is instrumental
        if result.as_ref().is_some_and(|data| {
          data.sync_lyrics.is_some()
            || preferred_lyrics_type.as_ref() == &LyricsType::Plain && data.plain_lyrics.is_some()
            || data.instrumental.is_some_and(|inst| inst)
        }) {
          return result;
        }
      }

      // Return any lyrics we have, even if not preferred type
      if primary_not_checked.is_empty() {
        if result.as_ref().is_some_and(|data| {
          data.plain_lyrics.is_some() || data.sync_lyrics.is_some() || data.instrumental.is_some()
        }) {
          return result;
        }

        return None;
      }

      std::thread::sleep(Duration::from_millis(20));
    }
  }

  pub(crate) fn init_providers_from_settings(&self) {
    let (primary_providers_order, secondary_providers_order) = SETTINGS
      .read()
      .inspect_err(|_| error!("Settings lock was poisoned while initialising lyrics providers"))
      .map_or_else(
        |_| (Settings::default().primary_providers, Settings::default().secondary_providers),
        |g| (g.primary_providers.clone(), g.secondary_providers.clone()),
      );

    self
      .primary_providers_order
      .swap(Arc::new(primary_providers_order.0));
    self
      .secondary_providers_order
      .swap(Arc::new(secondary_providers_order.0));

    let providers =
      Arc::new(init_providers(&self.primary_providers_order(), &self.secondary_providers_order()));
    self.providers.swap(providers);
  }

  pub(crate) fn primary_providers_order(&self) -> Vec<ProviderId> {
    self.primary_providers_order.load().to_vec()
  }

  pub(crate) fn secondary_providers_order(&self) -> Vec<ProviderId> {
    self.secondary_providers_order.load().to_vec()
  }
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
