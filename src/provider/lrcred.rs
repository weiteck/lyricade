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
  lyrics::{
    Lyrics, LyricsType, convert_enhanced_lrc_lyrics_to_lrc, convert_lrc_lyrics_to_plain,
    lyrics_are_lrc, ttml::parse_ttml_as_lrc,
  },
  provider::{
    LyricsData, Provider, ProviderError, ProviderId, ProviderResult, ProviderState,
    ProviderTestResult, manager::PROVIDER_TEST_TRACKS,
  },
  track::Track,
};

const API_SEARCH_URL: &str = "https://lrc.red/search.json";

#[derive(Debug)]
pub struct LrcRedProvider {
  semaphore: Semaphore,
  state: Arc<ProviderState>,
  rate_limited_until: ArcSwap<Option<DateTime<Utc>>>,
  req_delayed_until: ArcSwap<Option<DateTime<Utc>>>,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchResponse {
  hits: Vec<SearchResult>,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchResult {
  isrc: String,
  title: String,
  artist: String,
  album: String,
  duration: f32,
}

#[derive(Debug, Clone, Deserialize)]
struct IsrcResponse {
  title: String,
  artist: String,
  duration: f32,
  files: LyricsUrls,
  // isrc: String, // unused
  // album: String, // unused
}

#[derive(Debug, Clone, Deserialize)]
struct LyricsUrls {
  ttml: Option<String>,
  lrc: Option<String>,
  // lyricsfile: Option<String>, // unused
}

impl LrcRedProvider {
  pub fn new() -> Self {
    let semaphore = tokio::sync::Semaphore::new(1);
    let state = Arc::new(ProviderState::new(ProviderId::LrcRed, &semaphore));
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
impl Provider for LrcRedProvider {
  async fn api_fetch(
    &self,
    http_client: reqwest::Client,
    _user_agent: &str,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult {
    let isrc = self.api_search_for_isrc(track, &http_client).await?;
    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let lyrics_urls = self.api_get_lyrics_urls(&isrc, track, &http_client).await?;
    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let lyrics = self
      .api_get_lyrics(lyrics_urls, track, &http_client)
      .await?;
    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let lyrics_data = if lyrics.lyrics_type == LyricsType::Sync {
      LyricsData {
        instrumental: None,
        plain_lyrics: None,
        sync_lyrics: Some(lyrics),
      }
    } else {
      LyricsData {
        instrumental: None,
        plain_lyrics: Some(lyrics),
        sync_lyrics: None,
      }
    };

    Ok(lyrics_data)
  }

  fn id(&self) -> ProviderId {
    ProviderId::LrcRed
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

    // Results as of 2026-11-03
    let expected = [
      Some(String::from(
        "[ti:Vienna]\n[ar:Billy Joel]\n[al:The Stranger]\n[length:03:34]\n\n[00:15.95]Slow down, you crazy child\n[00:19.11]You're so ambitious for a juvenile\n[00:22.49]But then if you're so smart\n[00:24.58]Tell me why are you still so afraid? Mmm\n[00:31.24]Where's the fire, what's the hurry about?\n[00:33.90]You'd better cool it off before you burn it out\n[00:37.68]You've got so much to do\n[00:39.57]And only so many hours in a day, hey\n[00:46.28]But you know that when the truth is told\n[00:49.10]That you can get what you want or you can just get old\n[00:52.85]You're gonna kick off before you even get halfway through, ooh\n[01:00.25]When will you realize Vienna waits for you?\n[01:09.55]Slow down, you're doin' fine\n[01:12.32]You can't be everything you wanna be before your time\n[01:16.26]Although it's so romantic on the borderline tonight, tonight\n[01:25.01]Too bad, but it's the life you lead\n[01:27.64]You're so ahead of yourself, that you forgot what you need\n[01:31.45]Though you can see when you're wrong\n[01:33.34]You know you can't always see when you're right\n[01:37.34]You're right\n[01:39.98]You've got your passion, you've got your pride\n[01:43.32]But don't you know that only fools are satisfied?\n[01:47.74]Dream on, but don't imagine they'll all come true, ooh\n[01:54.02]When will you realize Vienna waits for you?\n[02:18.60]Slow down, you crazy child\n[02:21.35]And take the phone off the hook and disappear for a while\n[02:26.13]It's all right, you can afford to lose a day or two, ooh\n[02:32.97]When will you realize Vienna waits for you?\n[02:41.85]And you know that when the truth is told\n[02:44.72]That you can get what you want or you could just get old\n[02:48.48]You're gonna kick off before you even get halfway through, ooh-ooh\n[02:56.08]Why don't you realize Vienna waits for you?\n[03:03.87]When will you realize Vienna waits for you?\n",
      )),
      Some(String::from(
        "[ti:I Will Always Love You]\n[ar:Whitney Houston]\n[al:The Bodyguard (Original Soundtrack Album)]\n[length:04:31]\n\n[00:00.80]If I should stay\n[00:10.60]I would only be in your way\n[00:20.48]So I'll go, but I know\n[00:28.18]I'll think of you every step of the way\n[00:43.14]And I will always love you\n[00:54.16]I will always love you\n[01:07.28]You\n[01:10.29]My darling, you, mm-mm\n[01:17.06]Bittersweet memories\n[01:24.28]That is all I'm taking with me\n[01:31.74]So goodbye, please don't cry\n[01:38.62]We both know I'm not what you, you need\n[01:46.00]And I will always love you\n[01:55.80]I will always love you\n[02:35.64]I hope life treats you kind\n[02:42.82]And I hope you have all you've dreamed of\n[02:50.15]And I wish you joy and happiness\n[02:56.96]But above all this, I wish you love\n[03:08.92]And I will always love you\n[03:18.50]I will always love you\n[03:25.26]I will always love you\n[03:32.64]I will always love you\n[03:41.58]I will always love you\n[03:48.44]I, I will always love you\n[04:04.12]You\n[04:06.73]Darling, I love you\n[04:09.98]Ooh, I'll always, I'll always love you\n",
      )),
      Some(String::from(
        "[ti:Seasons (Waiting On You)]\n[ar:Future Islands]\n[al:Singles]\n[length:03:46]\n\n[00:29.35]Seasons change\n[00:32.76]And I've tried hard just to soften you\n[00:43.00]Well, seasons change\n[00:46.46]But I've grown tired trying to change for you\n[00:53.32]'Cause I've been waiting on you\n[01:00.39]I've been waiting on you\n[01:07.04]'Cause I've been waiting on you\n[01:12.18]Ooh-ooh-ooh, ooh, I've been waiting on you\n[01:25.44]As it breaks\n[01:28.44]The summer will wake\n[01:31.68]But the winter will wash what is left of the taste\n[01:39.18]As it breaks\n[01:42.19]The summer will warm\n[01:45.39]But the winter will crave what has gone\n[01:49.68]Will crave what has all gone away\n[01:55.23]People change\n[01:58.04]But you know some people never do\n[02:08.34]You know when people change\n[02:12.18]They gain a piece, but they lose one, too\n[02:19.04]'Cause I've been hanging on you\n[02:24.19]Ooh-ooh-ooh, ooh, I've been waiting on you\n[02:32.74]'Cause I've been waiting on you\n[02:37.88]Ooh-ooh-ooh, ooh, I've been hanging on you\n[02:51.19]As it breaks\n[02:54.15]The summer will wake\n[02:57.40]But the winter will wash what is left of the taste\n[03:04.88]As it breaks\n[03:07.89]The summer will warm\n[03:11.11]But the winter will crave what has gone\n[03:15.38]Will crave what has gone\n[03:18.81]Will crave what has all gone away\n[03:34.46]'Cause I've been waiting on you\n",
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

impl LrcRedProvider {
  async fn api_search_for_isrc(
    &self,
    track: &Track,
    http_client: &reqwest::Client,
  ) -> Result<String, ProviderError> {
    let search_url = Url::parse_with_params(
      API_SEARCH_URL,
      [("q", &format!("{} {} {}", track.track_name, track.artist_name, track.album_name))],
    )
    .map_err(|e| {
      error!("LrcRedProvider: {track}: Could not parse Track into request URL: {e}");
      ProviderError::NotFound
    })?;

    trace!("LrcRedProvider: {track}: Step 1/3: GET request to \"{}\"", &search_url);

    let response = http_client.get(search_url).send().await.map_err(|e| {
      error!("LrcRedProvider: {track}: {e}");
      ProviderError::Permanent
    })?;

    // Return requested retry delay if 429 too many requests
    self.handle_rate_limited(&response, track)?;

    if let Ok(search_response) = response.json::<SearchResponse>().await {
      trace!("LrcRedProvider: {track}: lrc.red API search response:\n{:#?}", &search_response);

      if search_response.hits.is_empty() {
        trace!("LrcRedProvider: {track}: Not found");
        return Err(ProviderError::NotFound);
      }

      return find_best_matching_isrc(&search_response.hits, track);
    }

    error!("LrcRedProvider: {track}: Could not parse server search response");
    Err(ProviderError::Permanent)
  }

  async fn api_get_lyrics_urls(
    &self,
    isrc: &str,
    track: &Track,
    http_client: &reqwest::Client,
  ) -> Result<LyricsUrls, ProviderError> {
    let track_url = Url::parse(&format!("https://lrc.red/s/{isrc}.json")).map_err(|e| {
      error!("LrcRedProvider: {track}: Could not parse URL from ISRC: {e}");
      ProviderError::NotFound
    })?;

    trace!("LrcRedProvider: {track}: Step 2/3: GET request to \"{}\"", &track_url);

    let response = http_client.get(track_url).send().await.map_err(|e| {
      error!("LrcRedProvider: {track}: {e}");
      ProviderError::Permanent
    })?;

    // Return requested retry delay if 429 too many requests
    self.handle_rate_limited(&response, track)?;

    if let Ok(track_response) = response.json::<IsrcResponse>().await {
      trace!("LrcRedProvider: {track}: lrc.red API ISRC response:\n{:#?}", &track_response);

      if !track_response.title.eq_ignore_ascii_case(&track.track_name)
        || !track_response
          .artist
          .eq_ignore_ascii_case(&track.artist_name)
        || (track_response.duration - track.duration).abs() >= 5.0
      {
        warn!(
          "LrcRedProvider: {track}: Track details in object returned from ISRC do not match - aborting"
        );
        trace!("LrcRedProvider: {track}: Not found");
        return Err(ProviderError::NotFound);
      }

      return Ok(track_response.files);
    }

    error!("LrcRedProvider: {track}: Could not parse server response for ISRC \"{isrc}\"");
    Err(ProviderError::Permanent)
  }

  async fn api_get_lyrics(
    &self,
    lyrics_urls: LyricsUrls,
    track: &Track,
    http_client: &reqwest::Client,
  ) -> Result<Lyrics, ProviderError> {
    let ttml_lyrics = lyrics_urls.lrc.is_none() && lyrics_urls.ttml.is_some();
    let lyrics_url = lyrics_urls
      .lrc
      .or(lyrics_urls.ttml)
      .ok_or(ProviderError::NotFound)
      .inspect_err(|_| {
        warn!("LrcRedProvider: {track}: Returned track object does not contain an LRC or TTML URL");
        trace!("LrcRedProvider: {track}: Not found");
      })?;
    let lyrics_url = Url::parse(&lyrics_url).map_err(|e| {
      error!("LrcRedProvider: {track}: Could not parse URL from \"{lyrics_url}\": {e}");
      ProviderError::NotFound
    })?;

    trace!("LrcRedProvider: {track}: Step 3/3: GET request to \"{}\"", &lyrics_url);

    let response = http_client.get(lyrics_url).send().await.map_err(|e| {
      error!("LrcRedProvider: {track}: {e}");
      ProviderError::Permanent
    })?;

    // Return requested retry delay if 429 too many requests
    self.handle_rate_limited(&response, track)?;

    let contents = response.text().await.map_err(|e| {
      error!("LrcRedProvider: {track}: {e}");
      ProviderError::NotFound
    })?;

    trace!("LrcRedProvider: {track}: lrc.red API track response:\n{:#?}", &contents);

    let lyrics = if ttml_lyrics {
      let parsed = parse_ttml_as_lrc(&contents).map_err(|e| {
        error!("LrcRedProvider: {track}: {e}");
        ProviderError::NotFound
      })?;
      trace!("LrcRedProvider: {track}: Parsed LRC lyrics from TTML");
      parsed
    } else {
      // Lyrics should be sync given we are specifically using the LRC URL,
      // but we test to make sure
      let (lyrics_type, contents) = if lyrics_are_lrc(&contents) {
        trace!("LrcRedProvider: {track}: Returned LRC lyrics");
        (LyricsType::Sync, convert_enhanced_lrc_lyrics_to_lrc(&contents))
      } else {
        warn!("LrcRedProvider: {track}: Returned plain lyrics when LRC format was expected");

        // Strip out any LRC-style tags from what is otherwise a plain lyrics file
        let plain_lyrics = convert_lrc_lyrics_to_plain(&contents);

        (LyricsType::Plain, plain_lyrics)
      };

      Lyrics {
        lyrics_type,
        contents,
      }
    };

    Ok(lyrics)
  }

  fn handle_rate_limited(
    &self,
    response: &reqwest::Response,
    track: &Track,
  ) -> Result<(), ProviderError> {
    if response.status() == StatusCode::TOO_MANY_REQUESTS {
      // TODO: Remove - testing only
      let headers = response.headers();
      warn!("LrcRedProvider: {track}: Headers for 429 response:\n{headers:#?}");

      if let Some(v) = response.headers().get("Retry-After")
        && let Ok(s) = v.to_str()
        && let Ok(req_delay) = str::parse::<f64>(s)
      {
        warn!(
          "LrcRedProvider: {track}: Too many requests - retry-delay of {req_delay:.0$}s requested by server",
          if req_delay.fract() >= 0.01 { 2 } else { 0 }
        );
        self.set_rate_limited(req_delay);
      } else {
        warn!(
          "LrcRedProvider: {track}: Too many requests - no \"Retry-After\" header; defaulting to delay of 5s"
        );
        self.set_rate_limited(5.0);
      }

      return Err(ProviderError::RateLimited);
    }

    Ok(())
  }
}

fn find_best_matching_isrc(hits: &[SearchResult], track: &Track) -> Result<String, ProviderError> {
  hits
    .iter()
    .enumerate()
    .find(|(_, sr)| {
      sr.title.eq_ignore_ascii_case(&track.track_name)
        && sr.artist.eq_ignore_ascii_case(&track.artist_name)
        && sr.album.eq_ignore_ascii_case(&track.album_name)
        && (sr.duration - track.duration).abs() < 5.0
    })
    .or_else(|| {
      hits.iter().enumerate().find(|(_, sr)| {
        sr.title.eq_ignore_ascii_case(&track.track_name)
          && sr.artist.eq_ignore_ascii_case(&track.artist_name)
          && (sr.duration - track.duration).abs() < 5.0
      })
    })
    .map(|(idx, sr)| {
      trace!("LrcRedProvider: {track}: Found matching track at hit {idx}");
      sr.isrc.clone()
    })
    .ok_or(ProviderError::NotFound)
    .inspect_err(|_| {
      trace!("LrcRedProvider: {track}: Not found: No matching track in {} hits", hits.len());
    })
}
