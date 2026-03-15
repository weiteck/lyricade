const API_BASE_URL: &str = "https://lrclib.net/api";
static API_URL_GET_LYRICS_FROM_TRACK_SIGNATURE: LazyLock<String> =
    LazyLock::new(|| format!("{}/get", API_BASE_URL));

use std::{sync::LazyLock, time::Duration};

use anyhow::anyhow;
use reqwest::Url;
use serde::Deserialize;
use tracing::{debug, error, trace, warn};

use crate::{
    Result,
    lyrics::{Lyrics, LyricsType},
    track::Track,
    util::now,
};

#[derive(Debug, Clone, Deserialize)]
#[allow(unused)]
#[serde(untagged)]
enum ApiResponse {
    #[serde(rename_all = "camelCase")]
    Success {
        id: i64,
        name: Option<String>,
        track_name: Option<String>,
        artist_name: Option<String>,
        album_name: Option<String>,
        duration: Option<f64>,
        instrumental: bool,
        plain_lyrics: Option<String>,
        synced_lyrics: Option<String>,
    },
    Error {
        code: i64,
        name: String,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct LrcLibClient {
    http_client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct LrcLibLyricsResponse {
    pub instrumental: bool,
    pub plain_lyrics: Option<Lyrics>,
    pub synced_lyrics: Option<Lyrics>,
}

impl LrcLibClient {
    pub fn new() -> Self {
        let http_client = match reqwest::Client::builder()
            .tls_backend_rustls()
            .timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(15))
            .build()
        {
            Ok(c) => c,
            Err(e) => panic!("Error creating HTTP client: {e}"),
        };

        Self { http_client }
    }

    pub async fn lyrics_from_track_signature(
        &self,
        track: &mut Track,
    ) -> Result<LrcLibLyricsResponse> {
        let url = Url::parse_with_params(
            &API_URL_GET_LYRICS_FROM_TRACK_SIGNATURE,
            &[
                ("track_name", &track.track_name),
                ("artist_name", &track.artist_name),
                ("album_name", &track.album_name),
                ("duration", &track.duration.to_string()),
            ],
        )?;

        debug!("Getting lyrics from lrclib.net for {}", &track);
        trace!("Trying lrclib.net endpoint \"{}\"", &url);

        let response = self.http_client.get(url).send().await?;
        let response_status = response.status();

        if let Ok(api_response) = response.json::<ApiResponse>().await {
            trace!("lrclib.net API response:\n{:#?}", &api_response);

            track.last_api_check_at = Some(now());

            match api_response {
                ApiResponse::Success {
                    instrumental,
                    plain_lyrics,
                    synced_lyrics,
                    ..
                } => {
                    return Ok(LrcLibLyricsResponse {
                        instrumental,
                        plain_lyrics: plain_lyrics.map(|s| Lyrics {
                            lyrics_type: LyricsType::Plain,
                            contents: s,
                        }),
                        synced_lyrics: synced_lyrics.map(|s| Lyrics {
                            lyrics_type: LyricsType::Sync,
                            contents: s,
                        }),
                    });
                }
                ApiResponse::Error {
                    code,
                    name,
                    message,
                } => {
                    let response = format_args!(
                        "lrclib.net API responded with error while getting lyrics for {}:\n\"{code}: {name}: {message}\"",
                        &track
                    );
                    warn!("{response}");
                    return Err(anyhow!("{response}"));
                }
            }
        };

        let error = format_args!(
            "lrclib.net server responded with status code {} while getting lyrics for {}",
            &response_status, &track
        );
        error!("{error}");
        Err(anyhow!("{error}"))
    }
}
