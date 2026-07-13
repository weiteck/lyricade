use anyhow::anyhow;
use bon::bon;
use camino::Utf8PathBuf;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use lofty::{
  config::{ParseOptions, WriteOptions},
  file::{AudioFile, TaggedFileExt},
  id3::v2::Id3v2Tag,
  mpeg,
  probe::Probe,
  tag::{self, Accessor, TagExt},
};
use std::{
  fmt::Display,
  fs,
  hash::Hash,
  io::{self, Seek},
  sync::LazyLock,
  time::Duration,
};
use tracing::{debug, error, trace, warn};

use crate::{
  DB_POOL, LRCLIB_CLIENT, Result, SETTINGS,
  lrclib::LrcLibLyricsResponse,
  lyrics::{self, Lyrics, LyricsFile, LyricsFileType, LyricsType, lyrics_are_synchronised},
  schema::tracks,
  settings::Settings,
  tags::{insert_lyrics_into_id3v2, lrc_lyrics_from_id3v2},
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
pub(crate) struct Track {
  #[diesel(skip_update)]
  pub(crate) id: i32,
  #[diesel(skip_update)]
  pub(crate) library_id: i32,
  #[diesel(skip_update)]
  pub(crate) path: String,
  #[allow(clippy::struct_field_names)]
  pub(crate) track_name: String,
  pub(crate) artist_name: String,
  pub(crate) album_name: String,
  pub(crate) duration: f32,
  pub(crate) instrumental: Option<bool>,
  pub(crate) lyrics: Option<String>,
  pub(crate) lyrics_synchronised: bool,
  pub(crate) lyrics_sidecar_lrc_file: Option<String>,
  pub(crate) lyrics_sidecar_txt_file: Option<String>,
  #[diesel(skip_update)]
  pub(crate) added_at: NaiveDateTime,
  pub(crate) updated_at: NaiveDateTime,
  pub(crate) refreshed_at: NaiveDateTime,
  pub(crate) last_api_check_at: Option<NaiveDateTime>,
  pub(crate) file_modified_at: NaiveDateTime,
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
    write!(f, "Track({})[\"{}\"]", self.id, self.path)
  }
}

/// An insertable audio `Track` DTO.
#[derive(Debug, Default, Clone, Insertable)]
#[diesel(table_name = crate::schema::tracks)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub(crate) struct NewTrack {
  pub(crate) library_id: i32,
  pub(crate) path: String,
  pub(crate) track_name: String,
  pub(crate) artist_name: String,
  pub(crate) album_name: String,
  pub(crate) duration: f32,
  pub(crate) instrumental: Option<bool>,
  pub(crate) lyrics: Option<String>,
  pub(crate) lyrics_synchronised: bool,
  pub(crate) lyrics_sidecar_lrc_file: Option<String>,
  pub(crate) lyrics_sidecar_txt_file: Option<String>,
  pub(crate) added_at: NaiveDateTime,
  pub(crate) updated_at: NaiveDateTime,
  pub(crate) refreshed_at: NaiveDateTime,
  pub(crate) last_api_check_at: Option<NaiveDateTime>,
  pub(crate) file_modified_at: NaiveDateTime,
}

#[bon]
impl Track {
  #[must_use]
  pub(crate) fn path(&self) -> Utf8PathBuf {
    Utf8PathBuf::from(&self.path)
  }

  #[must_use]
  pub(crate) fn lrc_file_path(&self) -> Option<Utf8PathBuf> {
    if self.lyrics_sidecar_lrc_file.is_some() {
      let mut path = self.path();
      path.set_extension("lrc");
      Some(path)
    } else {
      None
    }
  }

  #[must_use]
  pub(crate) fn txt_file_path(&self) -> Option<Utf8PathBuf> {
    if self.lyrics_sidecar_txt_file.is_some() {
      let mut path = self.path();
      path.set_extension("txt");
      Some(path)
    } else {
      None
    }
  }

  #[builder]
  pub(crate) fn scan_and_update(
    &mut self,
    /// Database connection. Intended for use as part of a transaction.
    /// Will obtain a connection from the pool if none passed.
    conn: Option<&mut SqliteConnection>,
  ) -> Result<()> {
    trace!("{self} scan: Scan and update: Refreshing metadata and sidecars");

    ///////////////////////////
    ///// Handle metadata /////
    ///////////////////////////
    let file = fs::File::open(self.path()).inspect_err(|error| error!("{error}"))?;
    let mut reader = io::BufReader::new(&file);
    let probe = Probe::new(&mut reader)
      .options(*TAG_PARSE_OPTIONS)
      .guess_file_type()
      .inspect_err(|error| warn!("{self} scan: {error}"))?;

    let mut tag_read = false;

    let mut read_id3v2_tag = |tag: &Id3v2Tag, mpeg_sample_rate: Option<u32>| {
      tag_read = true;

      self.artist_name = tag.artist().map(String::from).unwrap_or_default();
      self.album_name = tag.album().map(String::from).unwrap_or_default();
      self.track_name = tag.title().map(String::from).unwrap_or_default();

      self.lyrics = lrc_lyrics_from_id3v2(tag, mpeg_sample_rate)
        .map(|l| l.contents)
        .or(
          tag
            .unsync_text()
            .filter(|frame| !frame.content.is_empty())
            .map(|frame| frame.content.to_string())
            .next(),
        )
        .filter(|s| !s.is_empty());
    };

    if let Some(file_type) = probe.file_type() {
      // Try to get concrete ID3v2 tag type from supported formats so sync lyrics frames can be handled
      match file_type {
        lofty::file::FileType::Aac => {
          if let Ok(file) = lofty::aac::AacFile::read_from(&mut reader, *TAG_PARSE_OPTIONS) {
            self.duration = file.properties().duration().as_secs_f32();
            file.id3v2().inspect(|tag| read_id3v2_tag(tag, None));
          }
        }

        lofty::file::FileType::Aiff => {
          if let Ok(file) = lofty::iff::aiff::AiffFile::read_from(&mut reader, *TAG_PARSE_OPTIONS) {
            self.duration = file.properties().duration().as_secs_f32();
            file.id3v2().inspect(|tag| read_id3v2_tag(tag, None));
          }
        }

        lofty::file::FileType::Mpeg => {
          if let Ok(file) = lofty::mpeg::MpegFile::read_from(&mut reader, *TAG_PARSE_OPTIONS) {
            self.duration = file.properties().duration().as_secs_f32();
            file
              .id3v2()
              .inspect(|tag| read_id3v2_tag(tag, Some(file.properties().sample_rate())));
          }
        }

        lofty::file::FileType::Wav => {
          if let Ok(file) = lofty::iff::wav::WavFile::read_from(&mut reader, *TAG_PARSE_OPTIONS) {
            self.duration = file.properties().duration().as_secs_f32();
            file.id3v2().inspect(|tag| read_id3v2_tag(tag, None));
          }
        }

        // Get generic `Tag` type for other formats
        _ => {
          let tag = probe
            .read()
            .inspect_err(|error| warn!("{self} scan: {error}"))
            .ok()
            .and_then(|tf| {
              self.duration = tf.properties().duration().as_secs_f32();
              tf.primary_tag().or(tf.first_tag()).cloned()
            })
            .ok_or(anyhow!("{self} scan: Failed to read metadata tag"))?;

          tag_read = true;

          self.artist_name = tag.artist().map(String::from).unwrap_or_default();
          self.album_name = tag.album().map(String::from).unwrap_or_default();
          self.track_name = tag.title().map(String::from).unwrap_or_default();

          self.lyrics = tag
            .get_string(tag::ItemKey::Lyrics)
            .or(tag.get_string(tag::ItemKey::UnsyncLyrics))
            .map(ToString::to_string)
            .filter(|s| !s.is_empty());
        }
      }
    }

    // Read the file again to get just the generic `Tag` type if it failed to read above,
    // e.g. if an AIFF file had a tag that wasn't ID3v2 type
    if !tag_read {
      debug!(
        "{self} scan: Failed to read tag on first pass; ignoring ID3v2 and trying to extract primary tag"
      );

      let tag = Probe::new(&mut reader)
        .options(*TAG_PARSE_OPTIONS)
        .guess_file_type()
        .inspect_err(|error| warn!("{self} scan: {error}"))?
        .read()
        .inspect_err(|error| warn!("{self} scan: {error}"))
        .ok()
        .and_then(|tf| {
          self.duration = tf.properties().duration().as_secs_f32();
          tf.primary_tag().or(tf.first_tag()).cloned()
        })
        .ok_or(anyhow!("{self} scan: Failed to read metadata tag"))?;

      self.artist_name = tag.artist().map(String::from).unwrap_or_default();
      self.album_name = tag.album().map(String::from).unwrap_or_default();
      self.track_name = tag.title().map(String::from).unwrap_or_default();

      self.lyrics = tag
        .get_string(tag::ItemKey::Lyrics)
        .or(tag.get_string(tag::ItemKey::UnsyncLyrics))
        .map(ToString::to_string)
        .filter(|s| !s.is_empty());
    }

    self.lyrics_synchronised = self
      .lyrics
      .as_ref()
      .is_some_and(|l| lyrics_are_synchronised(l));

    let now = now();
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
      Some(conn) => self.write_to_db().conn(conn).call(),
      None => self.write_to_db().call(),
    }
  }

  /// Does nothing but sleeps for 500ms.
  #[builder]
  #[allow(unused)]
  pub(crate) async fn fetch_lyrics_test(
    &mut self,
    options: Option<FetchLyricsOptions>,
  ) -> Result<bool> {
    self.last_api_check_at = Some(now());
    debug!("Running fetch_lyrics_test -- sleeping 500ms");
    relm4::tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(false)
  }

  /// Get lyrics from lrclib.net API and optionally embed in lyrics tag and/or save to sidecar file.
  /// Returns `true` if tag was written or sidecar file was saved.
  #[builder]
  pub(crate) async fn fetch_lyrics(&mut self, options: Option<FetchLyricsOptions>) -> Result<bool> {
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
        self.save_sidecar_file(lyrics)?;

        match lyrics.lyrics_type {
          LyricsType::Sync => self.lyrics_sidecar_lrc_file = Some(lyrics.contents.clone()),
          LyricsType::Plain => self.lyrics_sidecar_txt_file = Some(lyrics.contents.clone()),
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

        self
          .write_to_file_and_db()
          .plain_lyrics_in_id3v2_uslt_frame(options.plain_lyrics_in_id3v2_uslt_frame)
          .call()?;
        update_db = false;
        modified = true;
      }
    }

    if update_db {
      self.write_to_db().call()?;
    }

    Ok(modified)
  }

  pub(crate) fn save_sidecar_file(&self, lyrics: &Lyrics) -> Result<()> {
    let file_type = LyricsFileType::from(lyrics.lyrics_type);
    let path = self.path().with_extension(file_type.file_extension());
    let sidecar_file = LyricsFile {
      lyrics: lyrics.clone(),
      file_type,
      path,
    };
    sidecar_file.save()?;

    Ok(())
  }

  /// Insert or update track in database.
  #[builder]
  pub(crate) fn write_to_db(
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
  pub(crate) fn write_to_file_and_db(
    &mut self,
    plain_lyrics_in_id3v2_uslt_frame: bool,
    /// Database connection. Intended for use as part of a transaction.
    /// Will obtain a connection from the pool if none passed.
    conn: Option<&mut SqliteConnection>,
  ) -> Result<()> {
    // Update lyrics tag (the only tag we ever change)
    let mut file = std::fs::File::options()
      .read(true)
      .write(true)
      .open(&self.path)
      .inspect_err(|error| error!("{error}"))?;
    let mut reader = io::BufReader::new(&file);

    // First check if MP3 w/ ID3v2 tag and try to extract synchronised lyrics
    // (ID3v2 has a specific 'SYLT' frame for this, unlike other tag formats)
    if let Some("mp3") = &self.path().extension()
      && let Ok(mut mpeg_file) = mpeg::MpegFile::read_from(&mut reader, *TAG_PARSE_OPTIONS)
      && mpeg_file.contains_tag_type(tag::TagType::Id3v2)
    {
      trace!("{self} write updated tag: Detected MP3 with ID3v2 type tag");

      let sample_rate = mpeg_file.properties().sample_rate();

      if let Some(tag) = mpeg_file.id3v2_mut() {
        let file_sync_lyrics = lrc_lyrics_from_id3v2(tag, Some(sample_rate)).map(|l| l.contents);

        let file_plain_lyrics = tag
          .unsync_text()
          .filter(|frame| !frame.content.is_empty())
          .map(|frame| frame.content.to_string())
          .next();

        if (self.lyrics_synchronised && self.lyrics != file_sync_lyrics)
          || self.lyrics != file_plain_lyrics
          || (self.lyrics.is_none() && file_sync_lyrics.or(file_plain_lyrics).is_some())
        {
          trace!("{self} write updated tag: Lyrics do not match; write required");

          let lyrics = Lyrics {
            lyrics_type: if self.lyrics_synchronised {
              LyricsType::Sync
            } else {
              LyricsType::Plain
            },
            contents: self.lyrics.clone().unwrap_or_default(),
          };

          // Tag frames will be removed if lyrics is empty
          insert_lyrics_into_id3v2(lyrics, plain_lyrics_in_id3v2_uslt_frame, tag);

          drop(reader);

          // Rewind the file cursor to the beginning before probing/saving
          if let Err(e) = file.rewind() {
            return Err(anyhow!("{self} write updated tag: Failed to rewind file: {e}"));
          }

          match tag.save_to(&mut file, *TAG_WRITE_OPTIONS) {
            Ok(()) => {
              debug!("{self} write updated tag: Lyrics tag updated in file");

              // Update track modified timestamp in DB
              self.file_modified_at = util::file_modified_at()
                .path(&self.path())
                .file(&file)
                .call();
            }
            Err(e) => {
              return Err(anyhow!("{self} write updated tag (MP3): Failed: {e}"))
                .inspect_err(|e| error!("{e}"));
            }
          }
        }
      }
    } else {
      // Probe file type and use abstract `TaggedFile`
      let mut tagged_file = Probe::new(reader)
        .options(*TAG_PARSE_OPTIONS)
        .guess_file_type()
        .inspect_err(|error| warn!("{self} scan: {error}"))?
        .read()
        .inspect_err(|error| warn!("{self} scan: {error}"))?;

      let tag = tagged_file
        .primary_tag_mut()
        .ok_or_else(|| anyhow!("{self} scan: No primary tag in file"))?;

      trace!("{self} write updated tag: Detected \"{:?}\" type tag", tag.tag_type());

      tag.remove_key(tag::ItemKey::Lyrics);

      if self.lyrics.is_some() {
        tag.insert_text(tag::ItemKey::Lyrics, self.lyrics.clone().unwrap_or_default());
      }

      // Rewind the file cursor to the beginning before probing/saving
      if let Err(e) = file.rewind() {
        return Err(anyhow!("{self} write updated tag: Failed to rewind file: {e}"));
      }

      match tag.save_to(&mut file, *TAG_WRITE_OPTIONS) {
        Ok(()) => {
          debug!("{self} write updated tag: Lyrics tag updated in file");

          // Update track modified timestamp in DB
          self.file_modified_at = util::file_modified_at()
            .path(&self.path())
            .file(&file)
            .call();
        }
        Err(e) => {
          return Err(anyhow!("{self} write updated tag: Failed: {e}"))
            .inspect_err(|e| error!("{e}"));
        }
      }
    }

    // Update database
    match conn {
      Some(conn) => self.write_to_db().conn(conn).call(),
      None => self.write_to_db().call(),
    }
  }

  /// Delete track row in database.
  #[builder]
  pub(crate) fn delete_from_db(
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

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct FetchLyricsOptions {
  pub(crate) prefer_lyrics_type: lyrics::LyricsType,
  pub(crate) ignore_plain_lyrics: bool,
  pub(crate) save_sidecar_file: bool,
  pub(crate) update_lyrics_tag: bool,
  pub(crate) plain_lyrics_in_id3v2_uslt_frame: bool,
}

impl Default for FetchLyricsOptions {
  fn default() -> Self {
    Self {
      prefer_lyrics_type: lyrics::LyricsType::Sync,
      ignore_plain_lyrics: false,
      save_sidecar_file: true,
      update_lyrics_tag: false,
      plain_lyrics_in_id3v2_uslt_frame: false,
    }
  }
}

impl From<&Settings> for FetchLyricsOptions {
  fn from(settings: &Settings) -> Self {
    FetchLyricsOptions {
      prefer_lyrics_type: settings.prefer_lyrics_type,
      ignore_plain_lyrics: settings.ignore_plain_lyrics_on_fetch,
      save_sidecar_file: settings.save_sidecar_file_on_fetch,
      update_lyrics_tag: settings.update_lyrics_tag_on_fetch,
      plain_lyrics_in_id3v2_uslt_frame: settings.plain_lyrics_in_id3v2_uslt_frame,
    }
  }
}
