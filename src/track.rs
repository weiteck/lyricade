use anyhow::anyhow;
use bon::bon;
use camino::Utf8PathBuf;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use lofty::{
  config::{ParseOptions, WriteOptions},
  file::{AudioFile, TaggedFileExt},
  probe::Probe,
  tag::{self, TagExt},
};
use std::{fmt::Display, fs, hash::Hash, io, sync::LazyLock, time::Duration};
use tracing::{debug, error, trace, warn};

use crate::{
  DB_POOL, LRCLIB_CLIENT, Result, SETTINGS,
  lrclib::LrcLibLyricsResponse,
  lyrics::{self, Lyrics, LyricsFile, LyricsFileType, LyricsType},
  schema::tracks,
  settings::Settings,
  util::{self, now},
};

static TAG_PARSE_OPTIONS: LazyLock<ParseOptions> = LazyLock::new(|| {
  ParseOptions::new()
    .read_properties(true)
    .read_tags(true)
    .read_cover_art(false)
    .parsing_mode(lofty::config::ParsingMode::Relaxed)
    .max_junk_bytes(2048)
    .implicit_conversions(true)
});

static TAG_WRITE_OPTIONS: LazyLock<WriteOptions> = LazyLock::new(|| {
  WriteOptions::new()
    .remove_others(false)
    .respect_read_only(false)
    .lossy_text_encoding(true)
});

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
  pub lyrics_synchronised: bool,
  pub lyrics_sidecar_lrc_file: Option<String>,
  pub lyrics_sidecar_txt_file: Option<String>,
  #[diesel(skip_update)]
  pub added_at: NaiveDateTime,
  pub updated_at: NaiveDateTime,
  pub refreshed_at: NaiveDateTime,
  pub last_api_check_at: Option<NaiveDateTime>,
  pub file_modified_at: NaiveDateTime,
}

impl Hash for Track {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.path.hash(state);
  }
}

impl Eq for Track {}
impl PartialEq for Track {
  fn eq(&self, other: &Self) -> bool {
    self.path == other.path
  }
}

impl Display for Track {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "Track({})[\"{}\"]", &self.id, &self.path)
  }
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
  pub lyrics_synchronised: bool,
  pub lyrics_sidecar_lrc_file: Option<String>,
  pub lyrics_sidecar_txt_file: Option<String>,
  pub added_at: NaiveDateTime,
  pub updated_at: NaiveDateTime,
  pub refreshed_at: NaiveDateTime,
  pub last_api_check_at: Option<NaiveDateTime>,
  pub file_modified_at: NaiveDateTime,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CleanUpSidecarFilesOptions {
  /// Used to choose lyrics type to upgrade with or keep as a sidecar file.
  pub prefer_lyrics_type: LyricsType,
  /// Embed sidecar lyrics if lyrics tag is empty or not the preferred type.
  pub upgrade_lyrics_tag: bool,
  /// Delete any _<audio-filename>.lrc_ or _<audio-filename>.txt_ sidecar lyrics files
  /// (optionally after embedding in file).
  pub delete_sidecar_files: bool,
  /// Keep only one sidecar file matching `preferred_lyrics_type`. This option causes the
  /// `delete_sidecar_files` option to be ignored.
  pub keep_one_sidecar_file: bool,
}

impl From<&Settings> for CleanUpSidecarFilesOptions {
  fn from(settings: &Settings) -> Self {
    CleanUpSidecarFilesOptions {
      prefer_lyrics_type: settings.prefer_lyrics_type,
      upgrade_lyrics_tag: settings.upgrade_lyrics_tag_on_scan,
      delete_sidecar_files: settings.delete_sidecar_files_on_scan,
      keep_one_sidecar_file: settings.keep_one_sidecar_file_on_scan,
    }
  }
}

#[derive(Debug, Clone, Copy)]
pub struct FetchLyricsOptions {
  pub prefer_lyrics_type: lyrics::LyricsType,
  pub ignore_plain_lyrics: bool,
  pub update_lyrics_tag: bool,
  pub save_sidecar_file: bool,
}

impl Default for FetchLyricsOptions {
  fn default() -> Self {
    Self {
      prefer_lyrics_type: lyrics::LyricsType::Sync,
      ignore_plain_lyrics: false,
      update_lyrics_tag: false,
      save_sidecar_file: true,
    }
  }
}

impl From<&Settings> for FetchLyricsOptions {
  fn from(settings: &Settings) -> Self {
    FetchLyricsOptions {
      prefer_lyrics_type: settings.prefer_lyrics_type,
      ignore_plain_lyrics: settings.ignore_plain_lyrics_on_fetch,
      update_lyrics_tag: settings.update_lyrics_tag_on_fetch,
      save_sidecar_file: settings.save_sidecar_file_on_fetch,
    }
  }
}

#[bon]
impl Track {
  #[must_use]
  pub fn path(&self) -> Utf8PathBuf {
    Utf8PathBuf::from(&self.path)
  }

  #[builder]
  pub fn scan_and_update(
    &mut self,
    /// Database connection. Intended for use as part of a transaction.
    /// Will obtain a connection from the pool if none passed.
    conn: Option<&mut SqliteConnection>,
  ) -> Result<()> {
    trace!(
      "{} scan: Scan and update: Refreshing metadata and sidecars",
      &self
    );

    ///////////////////////////
    ///// Handle metadata /////
    ///////////////////////////
    let file = fs::File::open(&self.path).inspect_err(|error| error!("{error}"))?;
    let reader = io::BufReader::new(&file);

    let tagged_file = Probe::new(reader)
      .options(*TAG_PARSE_OPTIONS)
      .guess_file_type()
      .inspect_err(|error| warn!("{} scan: {error}", &self))?
      .read()
      .inspect_err(|error| warn!("{} scan: {error}", &self))?;
    let tag = tagged_file
      .primary_tag()
      .ok_or_else(|| anyhow!("{} scan: No primary tag in file", &self))?;

    let duration = tagged_file.properties().duration().as_secs_f32();
    let track_name = tag
      .get_string(tag::ItemKey::TrackTitle)
      .map(ToString::to_string);
    let artist_name = tag
      .get_string(tag::ItemKey::TrackArtist)
      .map(ToString::to_string);
    let album_name = tag
      .get_string(tag::ItemKey::AlbumTitle)
      .map(ToString::to_string);
    // TODO: Read/write to `UnsyncLyrics` tag as appropriate
    // TODO: Also check if ID3v2 tag type and has SYLT (sync) lyrics
    let lyrics = tag
      .get_string(tag::ItemKey::Lyrics)
      .map(ToString::to_string);

    let lyrics_embedded_synchronised = lyrics
      .as_ref()
      .is_some_and(|l| lyrics::lyrics_are_synchronised(l));

    let now = now();

    self.track_name = track_name.unwrap_or_default();
    self.artist_name = artist_name.unwrap_or_default();
    self.album_name = album_name.unwrap_or_default();
    self.duration = duration;
    self.lyrics = lyrics;
    self.lyrics_synchronised = lyrics_embedded_synchronised;
    self.updated_at = now;
    self.refreshed_at = now;
    self.file_modified_at = util::file_modified_at()
      .path(&self.path())
      .file(&file)
      .call();

    ////////////////////////////////
    ///// Handle sidecar files /////
    ////////////////////////////////

    if let Some(sidecar_lyrics) = LyricsFile::from_track(self) {
      // Add sidecar lyrics to `Track`
      for lyrics_file in &sidecar_lyrics {
        match lyrics_file.file_type {
          LyricsFileType::Lrc => {
            self.lyrics_sidecar_lrc_file = Some(lyrics_file.lyrics.contents.clone());
          }
          LyricsFileType::Txt => {
            self.lyrics_sidecar_txt_file = Some(lyrics_file.lyrics.contents.clone());
          }
        }
      }
    }

    // Update database
    match conn {
      Some(conn) => self.write_to_db().conn(conn).call()?,
      None => self.write_to_db().call()?,
    }

    Ok(())
  }

  #[builder]
  pub fn clean_up_sidecar_files(
    &mut self,
    /// Will use `Settings` if `None` passed.
    options: Option<CleanUpSidecarFilesOptions>,
    /// Database connection. Intended for use as part of a transaction.
    /// Will obtain a connection from the pool if none passed.
    conn: Option<&mut SqliteConnection>,
  ) -> Result<()> {
    let options = {
      let settings = &*SETTINGS.read().map_err(|e| anyhow!("{e}"))?;
      options.unwrap_or_else(|| CleanUpSidecarFilesOptions::from(settings))
    };
    trace!("Cleaning Up Sidecar Files for {}", &self);

    // Was the `Track` updated?
    let mut db_requires_update = false;

    // Was the lyrics tag updated?
    let mut file_requires_update = false;

    if let Some(sidecar_lyrics) = LyricsFile::from_track(self) {
      // Add sidecar lyrics to `Track`
      for lyrics_file in &sidecar_lyrics {
        match lyrics_file.file_type {
          LyricsFileType::Lrc => {
            db_requires_update =
              self.lyrics_sidecar_lrc_file.as_ref() != Some(&lyrics_file.lyrics.contents);

            self.lyrics_sidecar_lrc_file = Some(lyrics_file.lyrics.contents.clone());
          }
          LyricsFileType::Txt => {
            db_requires_update =
              self.lyrics_sidecar_txt_file.as_ref() != Some(&lyrics_file.lyrics.contents);

            self.lyrics_sidecar_txt_file = Some(lyrics_file.lyrics.contents.clone());
          }
        }
      }

      if options.upgrade_lyrics_tag {
        match options.prefer_lyrics_type {
          LyricsType::Sync => {
            // Collection is sorted - best sync candidate is first
            if let Some(lf) = sidecar_lyrics.first()
              && (!self.lyrics_synchronised || self.lyrics.is_none())
            {
              let sync = lf.lyrics.lyrics_type == LyricsType::Sync;

              if sync || self.lyrics.is_some() {
                debug!(
                  "{} scan: Upgrade lyrics tag: Inserting lyrics from sidecar file \"{}\"",
                  &self, &lf.path
                );

                self.lyrics = Some(lf.lyrics.contents.clone());
                self.lyrics_synchronised = sync;

                db_requires_update = true;
                file_requires_update = true;
              }
            }
          }

          LyricsType::Plain => {
            // Collection is sorted - best plain candidate is last
            if let Some(lf) = sidecar_lyrics.last()
              && (self.lyrics_synchronised || self.lyrics.is_none())
            {
              // Convert sync lyrics to plain if required
              let lyrics = match lf.lyrics.lyrics_type {
                LyricsType::Sync => lf.lyrics.clone().into_plain().contents,
                LyricsType::Plain => lf.lyrics.clone().contents,
              };

              debug!(
                "{} scan: Upgrade lyrics tag: Inserting lyrics from sidecar file \"{}\"",
                &self, &lf.path
              );

              self.lyrics = Some(lyrics);
              self.lyrics_synchronised = false;

              db_requires_update = true;
              file_requires_update = true;
            }
          }
        }
      }

      if options.keep_one_sidecar_file {
        sidecar_lyrics
          .iter()
          .filter(|&lf| lf.lyrics.lyrics_type != options.prefer_lyrics_type)
          .for_each(|lf| {
            debug!(
              "{} scan: Keep one sidecar file: deleting redundant file \"{}\"",
              &self, &lf.path
            );

            fs::remove_file(&lf.path).unwrap_or_else(|error| error!("{error}"));

            match lf.lyrics.lyrics_type {
              LyricsType::Sync => {
                db_requires_update = self.lyrics_sidecar_lrc_file.take().is_some();
              }
              LyricsType::Plain => {
                db_requires_update = self.lyrics_sidecar_txt_file.take().is_some();
              }
            }
          });
      } else if options.delete_sidecar_files {
        for lyrics_file in &sidecar_lyrics {
          debug!(
            "{} scan: Deleting sidecar files: deleting file \"{}\"",
            &self, lyrics_file.path
          );

          fs::remove_file(&lyrics_file.path).unwrap_or_else(|error| error!("{error}"));

          db_requires_update = true;
        }

        self.lyrics_sidecar_lrc_file = None;
        self.lyrics_sidecar_txt_file = None;
      }
    } else {
      // No sidecar files found; make sure the `Track` matches
      let (lrc_changed, txt_changed) = (
        self.lyrics_sidecar_lrc_file.take().is_some(),
        self.lyrics_sidecar_txt_file.take().is_some(),
      );
      db_requires_update = lrc_changed || txt_changed;
    }

    if file_requires_update {
      // Lyrics tag changed - update file and database
      match conn {
        Some(conn) => self.write_to_file_and_db().conn(conn).call()?,
        None => self.write_to_file_and_db().call()?,
      }
    } else if db_requires_update {
      // Update database
      match conn {
        Some(conn) => self.write_to_db().conn(conn).call()?,
        None => self.write_to_db().call()?,
      }
    }

    Ok(())
  }

  /// Does nothing but sleeps for 1s.
  #[builder]
  pub async fn fetch_lyrics_test(&mut self, _options: Option<FetchLyricsOptions>) -> Result<bool> {
    self.last_api_check_at = Some(now());
    debug!("Running fetch_lyrics_test -- sleeping 200ms");
    tokio::time::sleep(Duration::from_millis(200)).await;
    Ok(false)
  }

  /// Get lyrics from lrclib.net API and optionally embed in lyrics tag and/or save to sidecar file.
  /// Returns `true` if tag was written or sidecar file was saved.
  #[builder]
  pub async fn fetch_lyrics(&mut self, options: Option<FetchLyricsOptions>) -> Result<bool> {
    let options = {
      let settings = &*SETTINGS.read().map_err(|e| anyhow!("{e}"))?;
      options.unwrap_or_else(|| FetchLyricsOptions::from(settings))
    };

    let mut modified = false;
    let mut update_db = true; // default to true to record API check timestamp
    self.last_api_check_at = Some(now());

    let response = LRCLIB_CLIENT.lyrics_from_track_signature(self).await;
    if response.is_err() {
      // Update DB on error to record the API check timestamp
      self.write_to_db().call()?;
      return Ok(false);
    }
    let LrcLibLyricsResponse {
      instrumental,
      plain_lyrics,
      synced_lyrics,
    } = response.expect("checked result ok");

    if instrumental && self.instrumental.is_none_or(|inst| !inst) {
      self.instrumental = Some(true);
    } else {
      // Extract the preferred lyrics type and update tags
      let lyrics = match options.prefer_lyrics_type {
        LyricsType::Sync => synced_lyrics.or(if options.ignore_plain_lyrics {
          None
        } else {
          plain_lyrics
        }),
        LyricsType::Plain => plain_lyrics.or(synced_lyrics.map(Lyrics::into_plain)),
      };

      // Generate sidecar file
      if let Some(lyrics) = &lyrics
        && options.save_sidecar_file
        && ((lyrics.lyrics_type == LyricsType::Sync && self.lyrics_sidecar_lrc_file.is_none())
          || (lyrics.lyrics_type == LyricsType::Plain && self.lyrics_sidecar_txt_file.is_none()))
      {
        let file_type = LyricsFileType::from(lyrics.lyrics_type);
        let path = self.path().with_extension(file_type.file_extension());
        let sidecar_file = LyricsFile {
          lyrics: lyrics.clone(),
          file_type,
          path,
        };
        sidecar_file.save()?;

        match sidecar_file.file_type {
          LyricsFileType::Lrc => self.lyrics_sidecar_lrc_file = Some(lyrics.contents.clone()),
          LyricsFileType::Txt => self.lyrics_sidecar_txt_file = Some(lyrics.contents.clone()),
        }

        modified = true;
      }

      let upgrade_tag = if options.update_lyrics_tag {
        match options.prefer_lyrics_type {
          LyricsType::Sync
            if self.lyrics.is_none()
              || (!self.lyrics_synchronised
                && lyrics
                  .as_ref()
                  .is_some_and(|l| l.lyrics_type == LyricsType::Sync)) =>
          {
            true
          }
          LyricsType::Plain if self.lyrics.is_none() => true,
          _ => false,
        }
      } else {
        false
      };

      if upgrade_tag {
        self.lyrics_synchronised = lyrics
          .as_ref()
          .is_some_and(|l| l.lyrics_type == LyricsType::Sync);
        self.lyrics = lyrics.as_ref().map(|l| l.contents.clone());

        self.write_to_file_and_db().call()?;
        update_db = false;
        modified = true;
      }
    }

    if update_db {
      self.write_to_db().call()?;
    }

    Ok(modified)
  }

  /// Insert or update track in database.
  #[builder]
  pub fn write_to_db(
    &mut self,
    /// Database connection. Intended for use as part of a transaction.
    /// Will obtain a connection from the pool if none passed.
    conn: Option<&mut SqliteConnection>,
  ) -> Result<()> {
    self.updated_at = now();

    // Need to reborrow `self` here to remove the `mut`
    let stmt = diesel::insert_into(tracks::table)
      .values(&*self)
      .on_conflict(tracks::id)
      .do_update()
      .set(&*self);

    let res = if let Some(conn) = conn {
      stmt.execute(conn)?
    } else {
      let mut conn = DB_POOL.get()?;
      stmt.execute(&mut conn)?
    };

    if res == 1 {
      trace!("Updated database entry for {}", &self);
    } else {
      error!("Failed to update database entry for {}", &self);
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
    if let Some(lyrics) = &self.lyrics {
      // Update lyrics tag (the only tag we ever change)
      let file = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&self.path)
        .inspect_err(|error| error!("{error}"))?;
      let reader = io::BufReader::new(&file);
      let mut tagged_file = Probe::new(reader)
        .options(*TAG_PARSE_OPTIONS)
        .guess_file_type()
        .inspect_err(|error| warn!("{} write updated tag: {error}", &self))?
        .read()?;
      let tag = tagged_file
        .primary_tag_mut()
        .ok_or_else(|| anyhow!("{} write updated tag: No primary tag in file", &self))?;

      // Check that lyrics have changed before writing
      if tag
        .get_string(tag::ItemKey::Lyrics)
        .is_some_and(|l| l != lyrics.as_str())
        && (tag.insert_text(lofty::tag::ItemKey::Lyrics, lyrics.clone())
          && tag.save_to_path(&self.path, *TAG_WRITE_OPTIONS).is_ok())
      {
        debug!("{} write updated tag: Lyrics tag updated in file", &self);

        // Update track modified timestamp in DB
        self.file_modified_at = util::file_modified_at()
          .path(&self.path())
          .file(&file)
          .call();
      }
    }

    // Update database
    match conn {
      Some(conn) => self.write_to_db().conn(conn).call()?,
      None => self.write_to_db().call()?,
    }

    Ok(())
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

    let res = if let Some(conn) = conn {
      stmt.execute(conn)?
    } else {
      let mut conn = DB_POOL.get()?;
      stmt.execute(&mut conn)?
    };

    if res == 1 {
      trace!("Deleted database entry for {}", &self);
    } else {
      error!("Failed to delete database entry for {}", &self);
    }

    Ok(())
  }
}
