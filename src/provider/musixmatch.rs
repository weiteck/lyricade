use std::sync::{Arc, atomic::AtomicUsize};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use musixmatch_inofficial::{Error as MusixmatchError, Musixmatch, storage::SessionStorage};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, trace, warn};

use crate::{
  PROVIDER_MANAGER,
  lyrics::{Lyrics, LyricsType},
  provider::{
    LyricsData, Provider, ProviderError, ProviderId, ProviderResult, ProviderState,
    ProviderTestResult, manager::PROVIDER_TEST_TRACKS,
  },
  track::Track,
};

#[derive(Default)]
pub struct SessionStore {
  token: ArcSwap<Option<String>>,
}

impl SessionStorage for SessionStore {
  fn write(&self, data: &str) {
    self.token.swap(Arc::new(Some(data.to_string())));
  }

  fn read(&self) -> Option<String> {
    self.token.load().as_ref().clone()
  }
}

pub struct MusixmatchProvider {
  client: Musixmatch,
  semaphore: Semaphore,
  state: Arc<ProviderState>,
  rate_limited_until: ArcSwap<Option<DateTime<Utc>>>,
  req_delayed_until: ArcSwap<Option<DateTime<Utc>>>,
}

impl MusixmatchProvider {
  pub fn new() -> Self {
    let session_store = SessionStore::default();
    let client = Musixmatch::builder()
      .storage(Box::new(session_store))
      .build()
      .unwrap_or_else(|e| {
        error!("MusixmatchProvider: Failed to build Musixmatch client; using default: {e}");
        Musixmatch::default()
      });

    let semaphore = tokio::sync::Semaphore::new(1);
    let state = Arc::new(ProviderState::new(ProviderId::Musixmatch, &semaphore));
    let rate_limited_until = ArcSwap::new(Arc::new(None));
    let req_delayed_until = ArcSwap::new(Arc::new(None));

    Self {
      client,
      semaphore,
      state,
      rate_limited_until,
      req_delayed_until,
    }
  }
}

#[async_trait]
impl Provider for MusixmatchProvider {
  async fn api_fetch(
    &self,
    _http_client: reqwest::Client,
    _user_agent: &str,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult {
    let result = self
      .client
      .matcher_subtitle(
        &track.track_name,
        &track.artist_name,
        musixmatch_inofficial::models::SubtitleFormat::Lrc,
        Some(track.duration),
        Some(3.0),
      )
      .await
      .map(|res| LyricsData {
        instrumental: None,
        plain_lyrics: None,
        sync_lyrics: Some(Lyrics {
          lyrics_type: LyricsType::Sync,
          contents: res.subtitle_body,
        }),
      });

    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    match result {
      Ok(lyrics_data) => Ok(lyrics_data),
      Err(e) => {
        let provider_error = match e {
          MusixmatchError::NotFound => {
            trace!("MusixmatchProvider: {track}: Could not find track");
            ProviderError::NotFound
          }
          MusixmatchError::InvalidData(msg) => {
            error!("MusixmatchProvider: {track}: API error: Invalid data: {msg}");
            ProviderError::NotFound
          }
          MusixmatchError::Ratelimit => {
            warn!("MusixmatchProvider: {track}: Too many requests - retry-delay of 5s set");
            self.set_rate_limited(5.0);
            ProviderError::RateLimited
          }
          MusixmatchError::TokenExpired => {
            // The client handles renewing the token and retries once automatically.
            // This error should only occur if the renewal returns the same error
            error!("MusixmatchProvider: {track}: API error: Token expired");
            ProviderError::Permanent
          }
          MusixmatchError::MusixmatchError { status_code, msg } => {
            error!("MusixmatchProvider: {track}: API error {status_code}: {msg}");
            ProviderError::Permanent
          }
          MusixmatchError::MissingCredentials => {
            error!("MusixmatchProvider: {track}: API error: Missing credentials");
            ProviderError::Permanent
          }
          MusixmatchError::WrongCredentials => {
            error!("MusixmatchProvider: {track}: API error: Wrong credentials");
            ProviderError::Permanent
          }
          MusixmatchError::NotAvailable => {
            error!("MusixmatchProvider: {track}: API error: Not available");
            ProviderError::Permanent
          }
          MusixmatchError::Http(error) => {
            error!("MusixmatchProvider: {track}: API error: HTTP error: {error}");
            ProviderError::Permanent
          }
          _ => {
            error!("MusixmatchProvider: {track}: API error: Unknown error");
            ProviderError::Permanent
          }
        };
        Err(provider_error)
      }
    }
  }

  fn id(&self) -> ProviderId {
    ProviderId::Musixmatch
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

  async fn test(&self) -> ProviderTestResult {
    let id = Self::id(&self);

    assert!(
      PROVIDER_MANAGER
        .primary_providers_order()
        .iter()
        .chain(PROVIDER_MANAGER.secondary_providers_order().iter())
        .any(|&pid| pid == id),
      "{id}Provider not initialised (must be in default Providers)"
    );

    // Results as of 2026-09-06
    let expected = [
      Some(String::from(
        "[00:15.99] Slow down, you crazy child\n[00:18.93] You're so ambitious for a juvenile\n[00:22.40] But then, if you're so smart\n[00:24.26] Tell me, why are you still so afraid? Mm\n[00:31.39] Where's the fire? What's the hurry about?\n[00:33.98] You better cool it off before you burn it out\n[00:37.89] You got so much to do and only so many hours in a day, hey\n[00:46.34] But you know that when the truth is told\n[00:49.00] That you can get what you want, or you can just get old\n[00:52.81] You're gonna kick off before you even get halfway through, ooh\n[01:00.19] When will you realize Vienna waits for you?\n[01:09.41] Slow down, you're doing fine\n[01:12.42] You can't be everything you wanna be before your time\n[01:16.20] Although it's so romantic, on the borderline tonight, tonight\n[01:24.93] Too bad, but it's the life you lead\n[01:27.75] You're so ahead of yourself that you forgot what you need\n[01:31.60] Though you can see when you're wrong\n[01:33.32] You know you can't always see when you're right, you're right\n[01:39.92] You've got your passion, you've got your pride\n[01:42.85] But don't you know that only fools are satisfied?\n[01:47.72] Dream on, but don't imagine they'll all come true, ooh\n[01:54.06] When will you realize Vienna waits for you?\n[02:02.57] \n[02:18.60] Slow down, you crazy child\n[02:21.39] And take the phone off the hook and disappear for a while\n[02:26.15] It's alright, you can afford to lose a day or two, ooh\n[02:32.98] When will you realize Vienna waits for you?\n[02:42.09] And you know that when the truth is told\n[02:44.65] That you can get what you want, or you can just get old\n[02:48.64] You're gonna kick off before you even get halfway through, ooh\n[02:55.94] Why don't you realize Vienna waits for you?\n[03:03.74] When will you realize Vienna waits for you?\n[03:09.76] ",
      )),
      Some(String::from(
        "[00:00.58] If I should stay\n[00:10.45] I would only be in your way\n[00:20.14] So I'll go, but I know\n[00:27.99] I'll think of you every step of the way\n[00:38.40] \n[00:42.94] And I will always love you\n[00:53.81] I will always love you\n[01:01.56] \n[01:07.10] You\n[01:10.25] My darling, you, mm-mm\n[01:16.93] Bittersweet memories\n[01:24.12] That is all I'm taking with me\n[01:31.54] So goodbye, please don't cry\n[01:38.48] We both know I'm not what you, you need\n[01:45.94] And I will always love you\n[01:55.78] I will always love you\n[02:04.58] \n[02:35.54] I hope life treats you kind\n[02:42.64] And I hope you have all you've dreamed of\n[02:49.99] And I wish you joy and happiness\n[02:56.89] But above all this, I wish you love\n[03:05.36] \n[03:08.75] And I will always love you\n[03:18.44] I will always love you\n[03:24.91] I will always love you\n[03:32.53] I will always love you\n[03:41.46] I will always love you\n[03:48.25] I, I will always love you\n[04:01.48] \n[04:03.85] You\n[04:06.50] Darling, I love you\n[04:09.96] Ooh, I'll always, I'll always love you\n[04:20.29] ",
      )),
      Some(String::from(
        "[00:15.26] Seasons change\n[00:18.66] And I tried hard just to soften you\n[00:25.21] \n[00:28.95] And seasons change\n[00:32.31] But I've grown tired trying to change for you\n[00:39.17] Because I've been waiting on you\n[00:46.32] I've been waiting on you\n[00:52.89] Cause I've been waiting on you\n[00:58.03] Oooh\n[01:00.02] I've been waiting on you\n[01:06.18] \n[01:11.37] As it breaks\n[01:14.39] The summer will wait\n[01:17.70] The winter will wash what is left\n[01:21.61] Of the taste\n[01:25.02] As it breaks\n[01:28.12] The summer will warm\n[01:31.20] But the winter will crave what has gone\n[01:35.69] Will crave what has all\n[01:38.76] Gone away\n[01:41.16] People change\n[01:43.94] But you know some people never do\n[01:50.97] \n[01:54.21] You know, when people change\n[01:58.07] They gain a piece, but they lose one too\n[02:04.81] Cause I've been hanging on you\n[02:12.02] I've been waiting on you\n[02:18.65] Cause I've been waiting on you\n[02:25.74] I've been hanging on you\n[02:31.90] \n[02:37.12] As it breaks\n[02:40.05] The summer will wake\n[02:43.39] But the winter will wash what is left\n[02:47.43] Of the taste\n[02:50.86] As it breaks\n[02:53.75] The summer will warm\n[02:57.09] But the winter will crave what has gone\n[03:01.32] Will crave what has gone\n[03:04.76] Will crave what has all\n[03:07.97] Gone away\n[03:12.66] \n[03:20.35] Cause I've been waiting on you\n[03:22.54] ",
      )),
    ];

    assert_eq!(
      expected.len(),
      PROVIDER_TEST_TRACKS.len(),
      "test tracks and expected results must be equal length"
    );

    let mut passed = 0;

    for (idx, track) in PROVIDER_TEST_TRACKS.iter().enumerate() {
      let token = CancellationToken::new();
      let _guard = token.drop_guard_ref();

      let lyrics = PROVIDER_MANAGER
        .fetch()
        .track(track)
        .with_provider(id)
        .cancel_token(token.clone())
        .call()
        .await
        .inspect(|l| trace!("{id}Provider: Test: Returned lyrics for {track}:\n{l:#?}"))
        .and_then(|l| l.sync_lyrics)
        .map(|l| l.contents);

      if let Some(expected) = expected.get(idx)
        && expected == &lyrics
      {
        passed += 1;
      } else if let Some(expected) = expected.get(idx)
        && expected != &lyrics
      {
        error!(
          r"{id}Provider: Test: Returned lyrics did not match expected lyrics:
----- Result:
{lyrics:#?}
----- Expected:
{expected:#?}"
        );
      }
    }

    let pass_rate = f64::from(passed) / PROVIDER_TEST_TRACKS.len() as f64;

    info!("{id}Provider: Test: Passed {passed}/{} tests", PROVIDER_TEST_TRACKS.len());

    match pass_rate {
      ..0.0 => ProviderTestResult::Failed,
      1.0.. => ProviderTestResult::Success,
      _ => ProviderTestResult::Degraded,
    }
  }
}
