use std::{fmt::Display, hash::Hash};

use anyhow::anyhow;
use bon::bon;
use camino::Utf8PathBuf;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use lofty::{
    file::{AudioFile, TaggedFileExt},
    tag::TagExt,
};
use tracing::{debug, error, trace, warn};

use crate::{
    DB_POOL, LRCLIB_CLIENT, Result,
    lyrics::{self, LyricsFile, LyricsType},
    schema::tracks,
    util::{self, now},
};

/// An audio `Track` with metadata persisted to the database.
#[derive(Debug, Default, Clone, Queryable, Selectable, Identifiable, AsChangeset, Insertable)]
#[diesel(table_name = crate::schema::tracks)]
#[diesel(treat_none_as_null = true)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Track {
    #[diesel(skip_update)]
    pub id: i32,
    #[diesel(skip_update)]
    pub library_id: i32,
    #[diesel(skip_update)]
    pub path: String,
    pub track_name: String,
    pub artist_name: String,
    pub album_name: String,
    pub duration: f32,
    pub instrumental: Option<bool>,
    pub lyrics: Option<String>,
    pub lyrics_sidecar_lrc_file: Option<String>,
    pub lyrics_sidecar_txt_file: Option<String>,
    pub lyrics_embedded_synchronised: bool,
    #[diesel(skip_update)]
    pub added_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub refreshed_at: NaiveDateTime,
    pub last_api_check_at: Option<NaiveDateTime>,
    pub file_modified_at: NaiveDateTime,
}

/// An insertable audio `Track` DTO.
#[derive(Debug, Default, Clone, Insertable)]
#[diesel(table_name = crate::schema::tracks)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct NewTrack {
    pub library_id: i32,
    pub path: String,
    pub track_name: String,
    pub artist_name: String,
    pub album_name: String,
    pub duration: f32,
    pub instrumental: Option<bool>,
    pub lyrics: Option<String>,
    pub lyrics_sidecar_lrc_file: Option<String>,
    pub lyrics_sidecar_txt_file: Option<String>,
    pub lyrics_embedded_synchronised: bool,
    pub added_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub refreshed_at: NaiveDateTime,
    pub last_api_check_at: Option<NaiveDateTime>,
    pub file_modified_at: NaiveDateTime,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ScanOptions {
    /// Embed synchronised sidecar lyrics if lyrics tag is empty or plain.
    /// Otherwise, embed plain sidecar lyrics if lyrics tag is empty.
    embed_sidecar_if_plain_or_empty: bool,
    /// Keep only one sidecar file, favouring synchronous lyrics.
    keep_one_sidecar: bool,
    /// Delete synchronous sidecar lyrics files.
    delete_sidecar_sync: bool,
    /// Delete plain sidecar lyrics files.
    delete_sidecar_plain: bool,
}

#[bon]
impl Track {
    pub fn path(&self) -> Utf8PathBuf {
        Utf8PathBuf::from(&self.path)
    }

    #[builder]
    pub fn scan_and_update(
        &mut self,
        options: Option<ScanOptions>,
        /// Database connection. Intended for use as part of a transaction.
        /// Will obtain a connection from the pool if none passed.
        conn: Option<&mut SqliteConnection>,
    ) -> Result<()> {
        let options = options.unwrap_or_default();
        trace!(
            "{} scan: Scan/update file with options:\n{:#?}",
            &self, options
        );

        let mut track_file = std::fs::File::options()
            .read(true)
            .write(false)
            .open(&self.path)
            .inspect_err(|error| error!("{error}"))?;

        // ///////////////////////////
        // ///// Handle metadata /////
        // ///////////////////////////
        // TODO: This relies on file extension; try probing for tag if it fails
        if let Ok(tagged_file) = lofty::read_from(&mut track_file)
            && let Some(tag) = tagged_file.primary_tag()
        {
            let track_name = tag
                .get_string(lofty::tag::ItemKey::TrackTitle)
                .map(ToString::to_string);
            let artist_name = tag
                .get_string(lofty::tag::ItemKey::TrackArtist)
                .map(ToString::to_string);
            let album_name = tag
                .get_string(lofty::tag::ItemKey::AlbumTitle)
                .map(ToString::to_string);
            let lyrics = tag
                .get_string(lofty::tag::ItemKey::Lyrics)
                .or_else(|| tag.get_string(lofty::tag::ItemKey::UnsyncLyrics))
                .map(ToString::to_string);
            let duration = tagged_file.properties().duration().as_secs_f32();

            // TODO: Also check if ID3v2 tag type and has SYLT (sync) lyrics
            let lyrics_embedded_synchronised = lyrics
                .as_ref()
                .is_some_and(|l| lyrics::lyrics_are_synchronised(l));

            let now = now();

            self.track_name = track_name.unwrap_or_default();
            self.artist_name = artist_name.unwrap_or_default();
            self.album_name = album_name.unwrap_or_default();
            self.duration = duration;
            self.lyrics = lyrics.clone();
            self.lyrics_embedded_synchronised = lyrics_embedded_synchronised;
            self.updated_at = now;
            self.refreshed_at = now;
            self.file_modified_at = util::file_modified_at(&track_file);

            // ////////////////////////////////
            // ///// Handle sidecar files /////
            // ////////////////////////////////
            let mut file_requires_update = false;
            if let Some(sidecar_lyrics) = LyricsFile::from_track(&self) {
                if options.embed_sidecar_if_plain_or_empty
                    && (!lyrics_embedded_synchronised || self.lyrics.is_none())
                {
                    // Collection is sorted - best (sync) candidate is first
                    if let Some(sl) = sidecar_lyrics.first() {
                        match sl.lyrics_type {
                            // Embed sync lyrics if embedded is plain or missing
                            LyricsType::Sync => {
                                debug!(
                                    "{} scan: Embed sidecar lyrics if plain or empty: Inserting synchronised lyrics from sidecar file \"{}\"",
                                    &self, &sl.path
                                );
                                self.lyrics = Some(sl.contents.clone());
                                self.lyrics_embedded_synchronised = true;
                                file_requires_update = true;
                            }
                            // Embed plain lyrics if embedded is missing
                            LyricsType::Plain if self.lyrics.is_none() => {
                                debug!(
                                    "{} scan: Embed sidecar lyrics if plain or empty: Inserting plain lyrics from sidecar file \"{}\"",
                                    &self, &sl.path
                                );
                                self.lyrics = Some(sl.contents.clone());
                                file_requires_update = true;
                            }
                            _ => (),
                        };
                    }
                }

                if options.keep_one_sidecar {
                    // Collection is sorted - best (sync) candidate is first, so we skip 1
                    sidecar_lyrics
                        .iter()
                        .skip(1)
                        .map(|sl| match &sl.lyrics_type {
                            LyricsType::Sync => sl.path.clone(),
                            LyricsType::Plain => sl.path.clone(),
                        })
                        .for_each(|p| {
                            debug!("{} scan: Keeping one sidecar file: deleting \"{p}\"", &self);
                            std::fs::remove_file(p).unwrap_or_else(|error| error!("{error}"))
                        });
                }

                if options.delete_sidecar_plain {
                    sidecar_lyrics
                        .iter()
                        .filter_map(|sl| match &sl.lyrics_type {
                            LyricsType::Sync => None,
                            LyricsType::Plain => Some(sl.path.clone()),
                        })
                        .for_each(|p| {
                            debug!(
                                "{} scan: Deleting plain sidecar files: deleting \"{p}\"",
                                &self
                            );
                            std::fs::remove_file(p).unwrap_or_else(|error| error!("{error}"))
                        });
                }

                if options.delete_sidecar_sync {
                    sidecar_lyrics
                        .iter()
                        .filter_map(|sl| match &sl.lyrics_type {
                            LyricsType::Sync => Some(sl.path.clone()),
                            LyricsType::Plain => None,
                        })
                        .for_each(|p| {
                            debug!(
                                "{} scan: Deleting sync sidecar files: deleting \"{p}\"",
                                &self
                            );
                            std::fs::remove_file(p).unwrap_or_else(|error| error!("{error}"))
                        });
                }
            }

            if file_requires_update {
                // Update file and database
                match conn {
                    Some(conn) => self.write_to_file_and_db().conn(conn).call()?,
                    None => self.write_to_file_and_db().call()?,
                }
            } else {
                // Update database
                match conn {
                    Some(conn) => self.write_to_db().conn(conn).call()?,
                    None => self.write_to_db().call()?,
                }
            }

            return Ok(());
        }

        Err(anyhow!(format!(
            "{} scan: Failed to read metadata from file",
            &self
        )))
        .inspect_err(|error| warn!("{error}"))
    }

    /// Insert or update track in database.
    #[builder]
    pub fn write_to_db(
        &self,
        /// Database connection. Intended for use as part of a transaction.
        /// Will obtain a connection from the pool if none passed.
        conn: Option<&mut SqliteConnection>,
    ) -> Result<()> {
        let stmt = diesel::insert_into(tracks::table)
            .values(self)
            .on_conflict(tracks::id)
            .do_update()
            .set(self);

        if 1 == match conn {
            Some(conn) => stmt.execute(conn)?,
            None => {
                let mut conn = DB_POOL.get()?;
                stmt.execute(&mut conn)?
            }
        } {
            trace!("Updated database entry for {}", &self);
        }

        Ok(())
    }

    /// Write lyrics tag to file.
    #[builder]
    pub fn write_to_file_and_db(
        &mut self,
        /// Database connection. Intended for use as part of a transaction.
        /// Will obtain a connection from the pool if none passed.
        conn: Option<&mut SqliteConnection>,
    ) -> Result<()> {
        let mut file = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&self.path)
            .inspect_err(|error| error!("{error}"))?;

        if let Ok(mut tagged_file) = lofty::read_from(&mut file)
            && let Some(tag) = tagged_file.primary_tag_mut()
            && let Some(lyrics) = &self.lyrics
        {
            let write_options = lofty::config::WriteOptions::new().lossy_text_encoding(true);
            if tag.insert_text(lofty::tag::ItemKey::Lyrics, lyrics.clone())
                && tag.save_to_path(&self.path, write_options).is_ok()
            {
                trace!("Wrote metadata to file \"{}\"", &self.path);

                // Update track modified timestamp in DB
                self.file_modified_at = util::file_modified_at(&file);
                match conn {
                    Some(conn) => self.write_to_db().conn(conn).call()?,
                    None => self.write_to_db().call()?,
                }

                return Ok(());
            }
        }

        Err(anyhow!(format!(
            "Failed to write metadata to file for {}",
            &self
        )))
        .inspect_err(|error| warn!("{error}"))
    }

    /// Delete track row in database.
    #[builder]
    pub fn delete_from_db(
        &self,
        /// Database connection. Intended for use as part of a transaction.
        /// Will obtain a connection from the pool if none passed.
        conn: Option<&mut SqliteConnection>,
    ) -> Result<()> {
        let stmt = diesel::delete(&self);

        if 1 == match conn {
            Some(conn) => stmt.execute(conn)?,
            None => {
                let mut conn = DB_POOL.get()?;
                stmt.execute(&mut conn)?
            }
        } {
            trace!("Deleted database entry for {}", &self);
        }

        Ok(())
    }

    pub async fn fetch_lyrics_from_api(&mut self, embed: bool) -> Result<()> {
        LRCLIB_CLIENT
            .get_lyrics_from_track_signature(self, embed)
            .await?;

        Ok(())
    }
}

impl Hash for Track {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}

impl PartialEq for Track {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Display for Track {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Track[{}] @ {}", &self.id, &self.path)
    }
}
