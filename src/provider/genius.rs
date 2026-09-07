use std::sync::{Arc, atomic::AtomicUsize};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Response, StatusCode, Url};
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

const API_SEARCH_URL: &str = "https://genius.com/api/search/song";

#[derive(Debug)]
pub struct GeniusProvider {
  semaphore: Semaphore,
  state: Arc<ProviderState>,
  rate_limited_until: ArcSwap<Option<DateTime<Utc>>>,
  req_delayed_until: ArcSwap<Option<DateTime<Utc>>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiSearchResponse {
  response: ApiSearchResponseData,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiSearchResponseData {
  sections: [ApiSearchResponseSection; 1],
}

#[derive(Debug, Clone, Deserialize)]
struct ApiSearchResponseSection {
  hits: Vec<ApiSearchResponseHit>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiSearchResponseHit {
  result: ApiSearchResponseSong,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiSearchResponseSong {
  artist_names: String,
  title: String,
  url: String,
}

impl GeniusProvider {
  pub fn new() -> Self {
    let semaphore = tokio::sync::Semaphore::new(1);
    let rate_limited_until = ArcSwap::new(Arc::new(None));
    let req_delayed_until = ArcSwap::new(Arc::new(None));
    let state = Arc::new(ProviderState::new(ProviderId::Genius, &semaphore));

    Self {
      semaphore,
      state,
      rate_limited_until,
      req_delayed_until,
    }
  }
}

#[async_trait]
impl Provider for GeniusProvider {
  async fn api_fetch(
    &self,
    http_client: reqwest::Client,
    user_agent: &str,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult {
    trace!("GeniusProvider: {track}: Step 1/2: Finding matching song URL");
    let url = self
      .find_song_url(&http_client, user_agent, &req_counter, track)
      .await?;

    // Sleep for the default request delay between multiple requests
    self.sleep_for_default_req_delay();

    trace!("GeniusProvider: {track}: Step 2/2: Getting lyrics for track with URL \"{url}\"");
    self
      .get_lyrics_for_song_url(&http_client, user_agent, &req_counter, track, &url)
      .await
  }

  fn id(&self) -> ProviderId {
    ProviderId::Genius
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
        "Slow down, you crazy childYou're so ambitious for a juvenileBut then if you're so smartTell me why are you still so afraid? MmWhere's the fire, what's the hurry about?You'd better cool it off before you burn it outYou've got so much to doAnd only so many hours in a day, heyBut you know that when the truth is toldThat you can get what you want or you can just get oldYou're gonna kick off before you even get halfway through, oohWhen will you realize Vienna waits for you?Slow down, you're doin' fineYou can't be everything you wanna be before your timeAlthough it's so romantic on the borderline tonight, tonightToo bad, but it's the life you leadYou're so ahead of yourself, that you forgot what you needThough you can see when you're wrongYou know you can't always see when you're rightYou're rightYou've got your passion, you've got your prideBut don't you know that only fools are satisfied?Dream on, but don't imagine they'll all come true, oohWhen will you realize Vienna waits for you?Slow down, you crazy childAnd take the phone off the hook and disappear for a whileIt's all right, you can afford to lose a day or two, oohWhen will you realize Vienna waits for you?And you know that when the truth is toldThat you can get what you want or you could just get oldYou're gonna kick off before you even get halfway through, oohWhy don't you realize Vienna waits for you?When will you realize Vienna waits for you?",
      )),
      Some(String::from(
        "If I should stay\nI would only be in your way\nSo I'll go, but I know\nI'll think of you every step of the way\n\nAnd I will always love you\nI will always love you\n\nYou\nMy darling, you\nMm hmm\n\nBittersweet memories\nThat is all I'm taking with me\nSo goodbye, please don't cry\nWe both know I'm not what you, you need\n\nAnd I will always love you\nI will always love you\nYou\n\nI hope life treats you kind\nAnd I hope you have all you dreamed of\nAnd I wish to you joy and happiness\nBut above all this, I wish you love\n\nAnd I will always love you\nI will always love you\nI will always love you\nI will always love you\nI will always love you\nI, I will always love you, you\nDarling, I love you\nOoh, I'll always, I'll always love you",
      )),
      Some(String::from(
        "Seasons change\nAnd I've tried hard just to soften you\nWell, seasons change\nBut I've grown tired trying to change for you\n'Cause I've been waiting on you\nI've been waiting on you\n'Cause I've been waiting on you, ooh-ooh, ooh\nI've been weighing on you\n\nAs it breaks\nThe summer will wake\nBut the winter will wash what is left of the taste\nAs it breaks\nThe summer will warm\nBut the winter will crave what has gone\nWill crave what has all gone away\n\nPeople change\nBut you know some people never do\nYou know when people change\nThey gain a peace, but they lose one too\n'Cause I've been hanging on you, ooh-ooh, ooh\nI've been weighing on you\n'Cause I've been waiting on you, ooh-ooh, ooh\nI've been hanging on you\nAs it breaks\nThe summer will wake\nBut the winter will wash what is left of the taste\nAs it breaks\nThe summer will warm\nBut the winter will crave what has gone\nWill crave what has gone\nWill crave what has all gone away\n\n'Cause I've been waiting on you",
      )),
    ];

    assert_eq!(
      expected.len(),
      PROVIDER_TEST_TRACKS.len(),
      "test tracks and expected results must be equal length"
    );

    let mut passed = 0;

    // Linebreaks can vary for scrapers each page render so they're removed
    let expected = expected.map(|s| s.map(|s| s.lines().collect()));

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
        .and_then(|l| l.plain_lyrics)
        .map(|l| l.contents.lines().collect::<String>());

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

impl GeniusProvider {
  async fn find_song_url(
    &self,
    http_client: &reqwest::Client,
    user_agent: &str,
    req_counter: &Arc<AtomicUsize>,
    track: &Track,
  ) -> Result<String, ProviderError> {
    let search_url = Url::parse_with_params(
      API_SEARCH_URL,
      &[
        ("q", format!("{} {}", track.artist_name, track.track_name)),
        ("per_page", 5.to_string()),
      ],
    )
    .map_err(|e| {
      error!("GeniusProvider: {track}: Could not build search URL from Track data: {e}");
      ProviderError::NotFound
    })?;

    trace!("GeniusProvider: {track}: GET request to \"{}\"", &search_url);

    let response = http_client
      .get(search_url)
      .header(reqwest::header::USER_AGENT, user_agent)
      .send()
      .await
      .map_err(|e| {
        error!("GeniusProvider: {track}: {e}");
        ProviderError::Permanent
      })?;
    let response_status = response.status();

    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if response_status == StatusCode::TOO_MANY_REQUESTS {
      return Err(self.handle_too_many_requests(&response, track));
    }

    match response.json::<ApiSearchResponse>().await {
      Ok(api_response) => {
        trace!("GeniusProvider: {track}: API search response:\n{:#?}", &api_response);

        let songs = api_response.response.sections[0]
          .hits
          .iter()
          .map(|hit| &hit.result)
          .collect::<Vec<_>>();

        if let Some(url) = find_best_match(&songs, track) {
          trace!(
            "GeniusProvider: {track}: Found matching song with URL {url} in {} search results",
            songs.len()
          );

          return Ok(url);
        }

        trace!("GeniusProvider: {track}: No exact match found in {} search results", songs.len());
        Err(ProviderError::NotFound)
      }
      Err(e) => {
        error!(
          "GeniusProvider: {track}: Failed to parse search response with status {response_status}: {e}"
        );
        Err(ProviderError::Permanent)
      }
    }
  }

  async fn get_lyrics_for_song_url(
    &self,
    http_client: &reqwest::Client,
    user_agent: &str,
    req_counter: &Arc<AtomicUsize>,
    track: &Track,
    url: &str,
  ) -> ProviderResult {
    trace!("GeniusProvider: {track}: GET request to \"{}\"", &url);

    let response = http_client
      .get(url)
      .header(reqwest::header::USER_AGENT, user_agent)
      .send()
      .await
      .map_err(|e| {
        error!("GeniusProvider: {track}: Error encountered while getting lyrics for {track}: {e}");
        ProviderError::Permanent
      })?;
    let response_status = response.status();

    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if response_status == StatusCode::TOO_MANY_REQUESTS {
      return Err(self.handle_too_many_requests(&response, track));
    }

    if let Ok(html) = response.text().await.inspect_err(|e| {
      error!("GeniusProvider: {track}: Failed to parse full text from response: {e}");
    }) && let document = scraper::Html::parse_document(&html)
      && let Ok(include_selector) = scraper::Selector::parse(r#"div[data-lyrics-container="true"]"#)
    {
      let contents = document
        .select(&include_selector)
        .map(|element| collect_text(element))
        .collect::<String>();

      if !contents.is_empty() {
        trace!("GeniusProvider: {track}: Scraped plain lyrics from HTML");

        let lyrics = Lyrics {
          lyrics_type: LyricsType::Plain,
          contents,
        };

        return Ok(LyricsData {
          instrumental: None,
          plain_lyrics: Some(lyrics),
          sync_lyrics: None,
        });
      }

      // Genius uses a <div> with a class beginning with "LyricsPlaceholder"
      // if track is known but not yet transcribed
      if let Ok(selector) = scraper::Selector::parse(r#"div[class^="LyricsPlaceholder"]"#)
        && document.select(&selector).count() > 0
      {
        return Err(ProviderError::NotFound);
      }
    }

    error!(
      "GeniusProvider: {track}: Failed to parse search lyrics from response with status {response_status}"
    );
    Err(ProviderError::Permanent)
  }

  fn handle_too_many_requests(&self, response: &Response, track: &Track) -> ProviderError {
    // Set retry delay if 429 too many requests
    let req_delay = if let Some(v) = response.headers().get("Retry-After")
      && let Ok(s) = v.to_str()
      && let Ok(req_delay) = str::parse::<f64>(s)
    {
      warn!(
        "GeniusProvider: {track}: Too many requests - retry-delay of {req_delay:.0$}s requested by server",
        if req_delay.fract() >= 0.01 { 2 } else { 0 }
      );
      req_delay
    } else {
      warn!(
        "GeniusProvider: {track}: Too many requests - no \"Retry-After\" header; defaulting to delay of 10s"
      );
      10.0
    };

    self.set_rate_limited(req_delay);

    ProviderError::RateLimited
  }
}

#[allow(clippy::cast_possible_truncation)]
fn find_best_match(songs: &[&ApiSearchResponseSong], track: &Track) -> Option<String> {
  // Exact match
  songs
    .iter()
    .find(|&&song| {
      song.artist_names.eq_ignore_ascii_case(&track.artist_name)
        && song.title.eq_ignore_ascii_case(&track.track_name)
    })
    .map(|hit| hit.url.clone())
}

fn collect_text(element: scraper::ElementRef) -> String {
  // Genius places non-lyrics text like attributions, comments, etc. in a <div> with
  // this data attribute inside the lyrics <div>, so we have to filter it out
  let exclude_attr = "data-exclude-from-selection";

  let mut buf = String::new();

  for child in element.children() {
    match child.value() {
      scraper::Node::Text(text) => {
        // Exclude verse/chorus/bridge section markers
        if let trimmed = text.trim()
          && trimmed.starts_with('[')
          && trimmed.ends_with(']')
          && trimmed != "[?]"
        {
          continue;
        }

        buf.push_str(text);
      }

      scraper::Node::Element(element) => {
        if element.attr(exclude_attr).is_some() {
          continue;
        }

        // Add line-breaks between sections, ensuring no double or leading empty line
        if element.name() == "br" && !buf.is_empty() && !buf.ends_with("\n\n") {
          buf.push('\n');
        } else if let Some(child_elem) = scraper::ElementRef::wrap(child) {
          buf.push_str(&collect_text(child_elem));
        }
      }

      _ => {}
    }
  }

  buf
}
