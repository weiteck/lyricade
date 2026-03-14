const API_BASE_URL: &str = "https://lrclib.net/api";
static API_URL_GET_LYRICS_FROM_TRACK_SIGNATURE: LazyLock<String> =
    LazyLock::new(|| format!("{}/get", API_BASE_URL));

use std::{io::Write, sync::LazyLock, time::Duration};

use anyhow::anyhow;
use reqwest::Url;
use serde::Deserialize;

use crate::{Result, track::Track, util::now};

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ApiResponse {
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

    pub async fn get_lyrics_from_track_signature(
        &self,
        track: &mut Track,
        embed: bool,
    ) -> Result<()> {
        let url = Url::parse_with_params(
            &API_URL_GET_LYRICS_FROM_TRACK_SIGNATURE,
            &[
                ("track_name", &track.track_name),
                ("artist_name", &track.artist_name),
                ("album_name", &track.album_name),
                ("duration", &track.duration.to_string()),
            ],
        )?;

        let response = self.http_client.get(url).send().await?;
        let response_status = response.status();

        if let Ok(api_response) = response.json::<ApiResponse>().await {
            track.last_api_check_at = Some(now());

            match api_response {
                ApiResponse::Success {
                    plain_lyrics,
                    synced_lyrics,
                    ..
                } => {
                    let (sidecar_file_path, lyrics, lyrics_synchronised) =
                        if synced_lyrics.is_some() {
                            let path = Some(track.path().with_extension("lrc"));
                            track.lyrics_sidecar_lrc_file = synced_lyrics.clone();
                            (path, synced_lyrics, true)
                        } else if plain_lyrics.is_some() {
                            let path = Some(track.path().with_extension("txt"));
                            track.lyrics_sidecar_txt_file = plain_lyrics.clone();
                            (path, plain_lyrics, false)
                        } else {
                            (None, None, track.lyrics_embedded_synchronised)
                        };

                    if let (Some(sidecar_file_path), Some(lyrics)) = (sidecar_file_path, lyrics) {
                        let mut file = std::fs::File::create(sidecar_file_path)?;
                        file.write_all(lyrics.as_bytes())?;

                        // Embed lyrics in file metadata
                        if embed {
                            track.lyrics = Some(lyrics);
                            track.lyrics_embedded_synchronised = lyrics_synchronised;
                            track.write_to_file_and_db().call()?;
                        } else {
                            // Update track in database
                            track.write_to_db().call()?;
                        }
                    }

                    return Ok(());
                }
                ApiResponse::Error {
                    code,
                    name,
                    message,
                } => {
                    return Err(anyhow!(
                        "API server responded with error: \"{code}: {name}: {message}\""
                    ));
                }
            }
        };

        Err(anyhow!(
            "API server responded with status code {}",
            response_status
        ))
    }
}
