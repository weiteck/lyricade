use std::{
  ops::Deref,
  sync::{Arc, LazyLock, atomic::AtomicUsize},
};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex::Regex;
use reqwest::{Response, StatusCode, Url};
use scraper::Element;
use tokio::sync::Semaphore;
use tracing::{error, trace, warn};

use crate::{
  lyrics::{Lyrics, LyricsType},
  provider::{LyricsData, Provider, ProviderError, ProviderId, ProviderResult, ProviderState},
  track::Track,
};

const SEARCH_URL: &str = "https://www.azlyrics.com/search/";
const X_PARAM_URL: &str = "https://www.azlyrics.com/geo.js";

/// Regex to match `ep.setAttribute("value", "<CAPTURE>");`.
static X_PARAM_REGEX: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r#"ep\.setAttribute\(\s*"value"\s*,\s*"(?<x>[^"]+)"\s*\);"#)
    .expect("should be valid regex")
});

#[derive(Debug)]
pub(crate) struct AzLyricsProvider {
  semaphore: Semaphore,
  state: Arc<ProviderState>,
  rate_limited_until: ArcSwap<Option<DateTime<Utc>>>,
  req_delayed_until: ArcSwap<Option<DateTime<Utc>>>,
  x_param: ArcSwap<String>,
}

impl AzLyricsProvider {
  pub(crate) fn new() -> Self {
    let semaphore = tokio::sync::Semaphore::new(1);
    let rate_limited_until = ArcSwap::new(Arc::new(None));
    let req_delayed_until = ArcSwap::new(Arc::new(None));
    let state = Arc::new(ProviderState::new(ProviderId::AzLyrics, &semaphore));
    let x_param = ArcSwap::new(Arc::new(String::new()));

    Self {
      semaphore,
      state,
      rate_limited_until,
      req_delayed_until,
      x_param,
    }
  }
}

#[async_trait]
impl Provider for AzLyricsProvider {
  async fn api_fetch(
    &self,
    http_client: reqwest::Client,
    req_counter: Arc<AtomicUsize>,
    track: &Track,
  ) -> ProviderResult {
    if self.x_param.load().is_empty() {
      self
        .refresh_x_param(&http_client, &req_counter, track)
        .await?;

      self.sleep_for_default_req_delay();
    }

    trace!("AzLyricsProvider: {track}: Step 1/2: Finding matching track");

    let url = match self.search(&http_client, &req_counter, track).await {
      // Retry once if new 'x' param is provided
      Err(ProviderError::Permanent)
        if self
          .refresh_x_param(&http_client, &req_counter, track)
          .await
          == Ok(true) =>
      {
        self.search(&http_client, &req_counter, track).await
      }
      res => res,
    }?;

    // Sleep for the default request delay between multiple requests
    self.sleep_for_default_req_delay();

    trace!("AzLyricsProvider: {track}: Step 2/2:  Getting lyrics for track with URL \"{url}\"");

    self
      .get_lyrics_for_song_url(&http_client, &req_counter, track, &url)
      .await
  }

  fn id(&self) -> ProviderId {
    ProviderId::AzLyrics
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

impl AzLyricsProvider {
  async fn refresh_x_param(
    &self,
    http_client: &reqwest::Client,
    req_counter: &Arc<AtomicUsize>,
    track: &Track,
  ) -> Result<bool, ProviderError> {
    trace!("AzLyricsProvider: {track}: Renewing URL 'x' param");
    trace!("AzLyricsProvider: {track}: GET request to \"{}\"", X_PARAM_URL);

    let response = http_client.get(X_PARAM_URL).send().await.map_err(|e| {
      error!("AzLyricsProvider: {track}: {e}");
      ProviderError::Permanent
    })?;
    let response_status = response.status();

    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if response_status == StatusCode::TOO_MANY_REQUESTS {
      return Err(self.handle_too_many_requests(&response, track));
    }

    if let Ok(body) = response.text().await {
      for line in body.lines() {
        if let Some(caps) = X_PARAM_REGEX.captures(line)
          && let Some(value) = &caps.name("x")
          && let x_param = String::from(value.as_str())
        {
          if self.x_param.load().as_ref() == &x_param {
            trace!("AzLyricsProvider: {track}: Got existing 'x' param \"{}\"", value.as_str());
            return Ok(false);
          }

          trace!("AzLyricsProvider: {track}: Got new 'x' param \"{}\"", value.as_str());
          self.x_param.store(Arc::new(x_param));
          return Ok(true);
        }
      }
    }

    Err(ProviderError::Permanent)
  }

  async fn search(
    &self,
    http_client: &reqwest::Client,
    req_counter: &Arc<AtomicUsize>,
    track: &Track,
  ) -> Result<String, ProviderError> {
    let url = Url::parse_with_params(
      SEARCH_URL,
      &[
        ("q", format!("{} {}", track.artist_name, track.track_name).as_str()),
        ("x", self.x_param.load().as_str()),
      ],
    )
    .map_err(|e| {
      error!("AzLyricsProvider: {track}: Could not build search URL from Track data: {e}");
      ProviderError::Permanent
    })?;

    trace!("AzLyricsProvider: {track}: GET request to \"{}\"", &url);

    let response = http_client.get(url).send().await.map_err(|e| {
      error!("AzLyricsProvider: {track}: {e}");
      ProviderError::Permanent
    })?;
    let response_status = response.status();

    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if response_status == StatusCode::TOO_MANY_REQUESTS {
      return Err(self.handle_too_many_requests(&response, track));
    }

    if let Ok(html) = response.text().await.inspect_err(|e| {
      error!("AzLyricsProvider: {track}: Failed to parse full text from response: {e}");
    }) && let document = scraper::Html::parse_document(&html)
      && let Ok(div_selector) = scraper::Selector::parse("div.panel")
      && document.select(&div_selector).count() == 1
      && let links = document
        .select(&div_selector)
        .flat_map(|el| el.descendent_elements())
        .filter(|el| el.value().name() == "a")
        .collect::<Vec<_>>()
    {
      for link in &links {
        let Some(url) = link.value().attr("href") else {
          continue;
        };

        // <a> should have two <b> children containing track and artist names
        let text = link
          .descendent_elements()
          .filter(|el| el.value().name() == "b")
          .map(|el| el.text().collect::<String>())
          .collect::<Vec<_>>();

        if text.len() != 2 {
          warn!(
            "AzLyricsProvider: {track}: Search result link with href \"{url}\" contained {} <b> elements instead of the expected 2",
            text.len()
          );
          continue;
        }

        if let Some(track_name) = text.first()
          && let Some(artist_name) = text.get(1)
          && track.artist_name.eq_ignore_ascii_case(artist_name.trim())
          && track
            .track_name
            .eq_ignore_ascii_case(track_name.trim().trim_matches('"'))
        {
          trace!(
            "AzLyricsProvider: {track}: Found matching song with URL {url} in {} search results",
            links.len()
          );
          return Ok(url.to_string());
        }
      }

      trace!("AzLyricsProvider: {track}: No exact match found in {} search results", links.len());
      return Err(ProviderError::NotFound);
    }

    error!(
      "AzLyricsProvider: {track}: Failed to scrape search results with status {response_status}"
    );
    Err(ProviderError::Permanent)
  }

  async fn get_lyrics_for_song_url(
    &self,
    http_client: &reqwest::Client,
    req_counter: &Arc<AtomicUsize>,
    track: &Track,
    url: &str,
  ) -> Result<LyricsData, ProviderError> {
    trace!("AzLyricsProvider: {track}: GET request to \"{}\"", &url);

    let response = http_client.get(url).send().await.map_err(|e| {
      error!("AzLyricsProvider: {track}: {e}");
      ProviderError::Permanent
    })?;
    let response_status = response.status();

    req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    if response_status == StatusCode::TOO_MANY_REQUESTS {
      return Err(self.handle_too_many_requests(&response, track));
    }

    if let Ok(html) = response.text().await.inspect_err(|e| {
      error!("AzLyricsProvider: {track}: Failed to parse full text from response: {e}");
    }) && let document = scraper::Html::parse_document(&html)
      && let Ok(div_selector) = scraper::Selector::parse("div.col-xs-12")
      && let Some(parent_div) = document.select(&div_selector).nth(1)
    {
      dbg!(&parent_div);

      if let Some(lyrics_div) = parent_div
        .child_elements()
        .filter(|el| {
          dbg!("CANDIDATE: {}", &el);
          let keep =
            el.value().name() == "div" && el.attr("class").is_none() && el.attr("id").is_none();
          dbg!("CANDIDATE VALID: {}", keep);
          keep
        })
        .nth(1)
        .map(|el| {
          dbg!("FINAL: {}", &el);
          el.text()
        })
      {
        lyrics_div.into_iter().for_each(|s| error!("{s}"));
      }
    }

    error!("AzLyricsProvider: {track}: Failed to scrape lyrics with status {response_status}");
    Err(ProviderError::Permanent)
  }

  // async fn get_lyrics_for_video_id(
  //   &self,
  //   http_client: &reqwest::Client,
  //   req_counter: &Arc<AtomicUsize>,
  //   track: &Track,
  //   video_id: &str,
  // ) -> ProviderResult {
  //   let get_lyrics_url = format!("{API_BASE_URL}/{video_id}");
  //   let get_lyrics_url = Url::parse(&get_lyrics_url).map_err(|e| {
  //     error!("AzLyricsProvider: {track}: Could not parse URL from \"{get_lyrics_url}\": {e}");
  //     ProviderError::Permanent
  //   })?;

  //   trace!("AzLyricsProvider: {track}: Step 2/2: Getting lyrics for track with videoId {video_id}");
  //   trace!("AzLyricsProvider: {track}: GET request to \"{}\"", &get_lyrics_url);

  //   let response = http_client.get(get_lyrics_url).send().await.map_err(|e| {
  //     error!("AzLyricsProvider: {track}: Error encountered while getting lyrics for {track}: {e}");
  //     ProviderError::Permanent
  //   })?;
  //   let response_status = response.status();

  //   req_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

  //   if response_status == StatusCode::TOO_MANY_REQUESTS {
  //     return Err(self.handle_too_many_requests(&response, track));
  //   }

  //   if let Ok(api_response) = response.json::<ApiLyricsResponse>().await.inspect_err(|e| {
  //     error!("AzLyricsProvider: {track}: Failed to parse get lyrics response: {e}");
  //   }) {
  //     trace!("AzLyricsProvider: {track}: API get lyrics response:\n{:#?}", &api_response);

  //     match api_response {
  //       ApiLyricsResponse::Success { data, .. } => {
  //         if data.len() > 1 {
  //           warn!(
  //             "AzLyricsProvider: {track}: `ApiLyricsResponse.data` contains {} items when 1 was expected",
  //             data.len()
  //           );
  //         }

  //         if let Some(ApiLyricsResponseItem {
  //           plain_lyric,
  //           synced_lyrics,
  //           ..
  //         }) = data.first().cloned()
  //         {
  //           let plain_lyric = plain_lyric.trim().to_string();
  //           let synced_lyrics = synced_lyrics.map(|s| s.trim().to_string());

  //           let plain_lyrics = if plain_lyric.is_empty() {
  //             None
  //           } else {
  //             Some(Lyrics {
  //               lyrics_type: LyricsType::Plain,
  //               contents: plain_lyric,
  //             })
  //           };

  //           let sync_lyrics = if synced_lyrics.as_ref().is_none_or(String::is_empty) {
  //             None
  //           } else {
  //             synced_lyrics.map(|s| Lyrics {
  //               lyrics_type: LyricsType::Sync,
  //               contents: s,
  //             })
  //           };

  //           return Ok(LyricsData {
  //             instrumental: None,
  //             plain_lyrics,
  //             sync_lyrics,
  //           });
  //         }

  //         error!("AzLyricsProvider: {track}: Failed to parse lyrics data from `Success` response");
  //         return Err(ProviderError::Permanent);
  //       }

  //       ApiLyricsResponse::Error { error, .. } => {
  //         return Err(self.handle_error(error, track));
  //       }
  //     }
  //   }

  //   error!("AzLyricsProvider: {track}: Server responded with {response_status}");
  //   Err(ProviderError::Permanent)
  // }

  fn handle_too_many_requests(&self, response: &Response, track: &Track) -> ProviderError {
    // Set retry delay if 429 too many requests
    let req_delay = if let Some(v) = response.headers().get("x-rate-limit-retry-after-seconds")
      && let Ok(s) = v.to_str()
      && let Ok(req_delay) = str::parse::<f64>(s)
    {
      warn!(
        "AzLyricsProvider: {track}: Too many requests - retry-delay of {req_delay:.0$}s requested by server",
        if req_delay.fract() >= 0.01 { 2 } else { 0 }
      );
      req_delay
    } else {
      warn!(
        "AzLyricsProvider: {track}: Too many requests - no \"x-rate-limit-retry-after-seconds\" header; defaulting to delay of 5s"
      );
      5.0
    };

    self.set_rate_limited(req_delay);

    ProviderError::RateLimited
  }
}
