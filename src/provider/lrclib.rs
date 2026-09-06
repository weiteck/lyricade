use std::sync::{Arc, atomic::AtomicUsize};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
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

const API_URL: &str = "https://lrclib.net/api/get";

#[derive(Debug)]
pub struct LrcLibProvider {
  semaphore: Semaphore,
  state: Arc<ProviderState>,
  rate_limited_until: ArcSwap<Option<DateTime<Utc>>>,
  req_delayed_until: ArcSwap<Option<DateTime<Utc>>>,
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
  pub fn new() -> Self {
    // Implementation Note:
    // https://lrclib.net/docs suggests making sequential requests only and honouring
    // the delay returned in 429 responses in the 'Retry-After' header
    let semaphore = tokio::sync::Semaphore::new(1);
    let state = Arc::new(ProviderState::new(ProviderId::LrcLib, &semaphore));
    let rate_limited_until = ArcSwap::new(Arc::new(None));
    let req_delayed_until = ArcSwap::new(Arc::new(None));

    Self {
      semaphore,
      state,
      rate_limited_until,
      req_delayed_until,
    }
  }
}

#[async_trait]
impl Provider for LrcLibProvider {
  async fn api_fetch(
    &self,
    http_client: reqwest::Client,
    _user_agent: &str,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult {
    let url = Url::parse_with_params(
      API_URL,
      [
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
          trace!("LrcLibProvider: {track}: Found track");

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
          trace!("LrcLibProvider: {track}: Could not find track");
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

  fn default_req_delay_secs(&self) -> Option<f64> {
    Some(0.2)
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
        "[00:15.41] Slow down you crazy child\n[00:18.97] You're so ambitious for a juvenile\n[00:22.29] But then if you're so smart tell me\n[00:24.99] Why are you still so afraid? (mmmmm)\n[00:30.59] Where's the fire, what's the hurry about?\n[00:33.82] You better cool it off before you burn it out\n[00:37.43] You got so much to do and only\n[00:39.98] So many hours in a day (Ay)\n[00:45.95] But you know that when the truth is told\n[00:48.94] That you can get what you want\n[00:50.90] Or you can just get old\n[00:52.89] You're gonna kick off before you even get halfway through (Oooh)\n[00:59.92] When will you realize, Vienna waits for you?\n[01:06.80] \n[01:09.18] Slow down you're doing fine\n[01:12.15] You can't be everything you want to be before your time\n[01:16.12] Although it's so romantic on the borderline tonight (tonight)\n[01:24.21] Too bad, but it's the life you lead\n[01:27.52] You're so ahead of yourself that you forgot what you need\n[01:31.49] Though you can see when you're wrong\n[01:33.27] You know you can't always see when you're right (you're right)\n[01:39.75] You got your passion, you got your pride\n[01:43.04] But don't you know that only fools are satisfied?\n[01:47.31] Dream on, but don't imagine they'll all come true (Oooh)\n[01:53.85] When will you realize, Vienna waits for you?\n[02:01.66] \n[02:18.21] Slow down you crazy child\n[02:21.48] Take the phone off the hook and disappear for a while\n[02:25.96] It's alright, you can afford to lose a day or two (oooh)\n[02:32.68] When will you realize, Vienna waits for you?\n[02:41.44] And you know that when the truth is told\n[02:44.68] That you can get what you want or you can just get old\n[02:48.32] You're gonna kick off before you even get halfway through (oooh)\n[02:55.75] Why don't you realize, Vienna waits for you?\n[03:03.36] When will you realize, Vienna waits for you?\n[03:09.26] ",
      )),
      Some(String::from(
        "[00:00.56] If I should stay\n[00:03.64] \n[00:10.47] I would only be in your way\n[00:20.19] So I'll go, but I know\n[00:27.98] I'll think of you every step of the way\n[00:39.26] \n[00:42.92] And I will always love you\n[00:53.83] I will always love you\n[00:59.61] \n[01:07.08] You\n[01:10.29] My darling, you, mm, mm\n[01:16.90] Bittersweet memories\n[01:24.10] That is all I'm taking with me\n[01:31.56] So goodbye, please don't cry\n[01:38.43] We both know I'm not what you, you need\n[01:45.96] And I will always love you\n[01:55.72] I will always love you\n[02:05.55] You\n[02:08.19] \n[02:35.55] I hope life treats you kind\n[02:42.68] And I hope you have all you've dreamed of\n[02:49.97] And I wish you joy and happiness\n[02:56.86] But above all this, I wish you love\n[03:04.97] \n[03:08.77] And I will always love you\n[03:18.46] I will always love you\n[03:24.92] I will always love you\n[03:32.51] I will always love you\n[03:41.45] I will always love you\n[03:48.24] I, I will always love you\n[04:00.31] \n[04:03.88] You\n[04:06.45] Darling, I love you\n[04:10.49] I'll always, I'll always love you\n[04:21.15] ",
      )),
      Some(String::from(
        "[00:29.29] Seasons change\n[00:32.56] And I tried hard just to soften you\n[00:36.10] \n[00:42.98] And seasons change\n[00:46.47] But I've grown tired of trying to change for you\n[00:53.32] 'Cause I've been waiting on you\n[01:00.02] I've been waiting on you\n[01:07.08] 'Cause I've been waiting on you\n[01:13.96] I've been waiting on you\n[01:18.04] \n[01:25.34] As it breaks\n[01:28.36] The summer will wake\n[01:31.41] But the winter will wash what is left\n[01:35.86] Of the taste\n[01:39.18] As it breaks\n[01:42.16] The summer will warm\n[01:45.35] But the winter will crave what is gone\n[01:49.63] Will crave what has all\n[01:52.74] Gone away\n[01:55.20] People change\n[01:58.08] But you know some people never do\n[02:01.89] \n[02:08.35] You know when people change\n[02:12.19] They gain a piece but they lose one too\n[02:19.23] 'Cause I've been hanging on you\n[02:26.08] I've been waiting on you\n[02:33.01] 'Cause I've been waiting on you\n[02:39.87] I've been hanging on you\n[02:43.68] \n[02:51.15] As it breaks\n[02:54.26] The summer will wake\n[02:57.32] But the winter will wash what is left\n[03:01.45] Of the taste\n[03:04.93] As it breaks\n[03:07.72] The summer will warm\n[03:10.94] But the winter will crave what is gone\n[03:15.40] Will crave what is gone\n[03:18.70] Will crave what has all\n[03:21.91] Gone away\n[03:24.14] \n[03:34.42] 'Cause I've been waiting on you\n[03:38.43] ",
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
