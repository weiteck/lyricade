use std::sync::{Arc, atomic::AtomicUsize};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Response, StatusCode, Url};
use serde::Deserialize;
use tokio::sync::Semaphore;
use tracing::{error, trace, warn};

use crate::{
  lyrics::{Lyrics, LyricsType},
  provider::{LyricsData, Provider, ProviderError, ProviderId, ProviderResult, ProviderState},
  track::Track,
};

const API_SEARCH_URL: &str = "https://genius.com/api/search/song";

#[derive(Debug)]
pub(crate) struct GeniusProvider {
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
  pub(crate) fn new() -> Self {
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
      ProviderError::Permanent
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
