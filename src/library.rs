use std::{collections::HashSet, fmt::Display, time::Instant};

use anyhow::anyhow;
use bon::bon;
use camino::{Utf8Path, Utf8PathBuf};
use chrono::NaiveDateTime;
use diesel::{dsl::insert_into, prelude::*};

use tracing::{error, info, trace, warn};
use walkdir::WalkDir;

use crate::{
  AUDIO_FILE_EXTENSIONS, DB_POOL, Result, SETTINGS,
  lyrics::LyricsType,
  schema::{libraries, tracks},
  settings::Settings,
  track::{self, FetchLyricsOptions, NewTrack, Track},
  util::{self, now},
};

/// Represents a library path.
#[derive(Debug, Default, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::schema::libraries)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Library {
  pub id: i32,
  pub path: String,
  pub name: Option<String>,
  pub added_at: NaiveDateTime,
  pub updated_at: NaiveDateTime,
}

#[derive(Debug, Default, Clone, Insertable)]
#[diesel(table_name = crate::schema::libraries)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct NewLibrary {
  pub path: String,
  pub name: Option<String>,
  pub added_at: NaiveDateTime,
  pub updated_at: NaiveDateTime,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RefreshOptions {
  pub scan_new_only: bool,
  pub scan_options: track::ScanOptions,
}

impl From<&Settings> for RefreshOptions {
  fn from(value: &Settings) -> Self {
    RefreshOptions {
      scan_new_only: value.scan_new_files_only,
      scan_options: track::ScanOptions {
        prefer_lyrics_type: value.prefer_lyrics_type,
        upgrade_lyrics_tag: value.upgrade_lyrics_tag_on_scan,
        delete_sidecar_files: value.delete_sidecar_files_on_scan,
        keep_one_sidecar_file: value.keep_one_sidecar_file_on_scan,
      },
    }
  }
}

#[bon]
impl Library {
  pub fn add(path: &Utf8Path) -> Result<Library> {
    let mut conn = DB_POOL.get()?;

    // Check if path is a directory
    if !path.is_dir() {
      return Err(anyhow!(
        "Library path \"{}\" is not a valid directory",
        &path
      ));
    }

    let existing_libraries = libraries::table.load::<Library>(&mut conn)?;

    // Check for existing `Library` with this path
    if let Some(existing_library) = existing_libraries
      .iter()
      .find(|&lib| lib.path().as_path() == path)
    {
      return Err(anyhow!(
        "Library path already exists as {}",
        existing_library
      ));
    }

    // Check if this path is a subdirectory of an existing `Library`
    if let Some(existing_library) = existing_libraries
      .iter()
      .find(|lib| path.starts_with(&lib.path))
    {
      return Err(anyhow!(
        "Path cannot be a subdirectory an existing Library. Conflicts with {}",
        existing_library
      ));
    }

    // Check if this path is a parent directory of an existing `Library`
    if let Some(existing_library) = existing_libraries
      .iter()
      .find(|lib| lib.path().starts_with(path))
    {
      return Err(anyhow!(
        "Path cannot be a parent directory of an existing Library. Conflicts with {}",
        existing_library
      ));
    }

    let now = now();
    let inserted_library = insert_into(libraries::table)
      .values(NewLibrary {
        path: path.to_string(),
        name: None,
        added_at: now,
        updated_at: now,
      })
      .get_result::<Library>(&mut conn)?;

    info!("Inserted {}", &inserted_library);

    Ok(inserted_library)
  }

  /// Get a library by its ID.
  pub fn get(id: i32) -> Result<Library> {
    let mut conn = DB_POOL.get()?;

    let lib = libraries::table
      .find(id)
      .first::<Library>(&mut conn)
      .inspect_err(|error| {
        error!("Database error while trying to get Library with ID {id}: {error}")
      })?;

    Ok(lib)
  }

  /// Get all libraries.
  pub fn get_all() -> Result<Vec<Library>> {
    let mut conn = DB_POOL.get()?;

    let libs = libraries::table
      .load::<Library>(&mut conn)
      .inspect_err(|error| error!("Database error while trying to get all Libraries: {error}"))?;
    Ok(libs)
  }

  #[builder]
  /// Read metadata for new and existing files in `Library` path and update database.
  pub fn refresh(&self, options: Option<RefreshOptions>) -> Result<usize> {
    let options = {
      let settings = &*SETTINGS.read().map_err(|e| anyhow!("{e}"))?;
      options.unwrap_or_else(|| RefreshOptions::from(settings))
    };
    info!("{} refresh: Started with options: {:?}", &self, options);

    let start = Instant::now();
    let mut conn = DB_POOL.get()?;
    let now = now();
    let paths = self
      .file_paths()
      .iter()
      .map(|p| p.to_string())
      .collect::<HashSet<_>>();
    let mut existing_count = 0_usize;
    let mut existing_refreshed_count = 0_usize;
    let mut new_refreshed_count = 0_usize;
    let mut missing_removed_count = 0_usize;

    let rows = conn.transaction::<usize, _, _>(|conn| {
      // Get existing and missing tracks
      let (existing_tracks, missing_tracks): (Vec<Track>, Vec<Track>) = tracks::table
        .filter(tracks::library_id.eq(&self.id))
        .load::<Track>(conn)?
        .into_iter()
        .partition(|t| paths.contains(&t.path));
      existing_count = existing_tracks.len();

      // Build new tracks from paths not in the database
      let existing_track_paths = existing_tracks
        .iter()
        .map(|t| t.path.clone())
        .collect::<HashSet<_>>();
      let new_paths = paths
        .iter()
        .filter(|&p| !existing_track_paths.contains(p))
        .collect::<HashSet<_>>();
      let new_tracks = new_paths
        .iter()
        .map(|path| NewTrack {
          library_id: self.id,
          path: path.to_string(),
          added_at: now,
          updated_at: now,
          ..Default::default()
        })
        .collect::<Vec<_>>();

      // Insert new tracks (without metadata)
      diesel::insert_into(tracks::table)
        .values(new_tracks)
        .execute(conn)?;

      // Get the tracks we just inserted so we have their IDs
      // We do this because RETURNING is not currently working with SQLite batch queries
      let inserted_new_tracks = tracks::table
        .filter(tracks::added_at.eq(now))
        .load::<Track>(conn)?;

      // Refresh metadata from files & write to database; discard files that fail to update metadata
      let mut new_tracks = vec![];
      let mut errored_new_tracks = vec![];
      for mut track in inserted_new_tracks {
        if track
          .scan_and_update()
          .options(options.scan_options)
          .conn(conn)
          .call()
          .is_ok()
        {
          new_tracks.push(track);
          new_refreshed_count += 1;
        } else {
          errored_new_tracks.push(track);
        }
      }

      if options.scan_new_only {
        // Refresh tracks with changed modified timestamps
        for mut track in existing_tracks {
          if track.file_modified_at != util::file_modified_at().path(&track.path()).call() {
            trace!("{} file has changed and will be re-scanned", &track);
            if track
              .scan_and_update()
              .options(options.scan_options)
              .conn(conn)
              .call()
              .is_ok()
            {
              existing_refreshed_count += 1;
            };
          }
        }
      } else {
        // Refresh existing tracks
        for mut track in existing_tracks {
          if track
            .scan_and_update()
            .options(options.scan_options)
            .conn(conn)
            .call()
            .is_ok()
          {
            existing_refreshed_count += 1;
          };
        }
      }

      // Clean up rows created for new tracks that we failed to get metadata for
      for track in errored_new_tracks {
        track.delete_from_db().conn(conn).call()?;
      }

      // Delete missing tracks (path not found)
      missing_removed_count = missing_tracks.len();
      if missing_removed_count > 0 {
        warn!(
          "{} tracks not found and will be removed: {:?}",
          missing_removed_count,
          missing_tracks.iter().map(|t| &t.path).collect::<Vec<_>>()
        );
        for track in missing_tracks {
          track.delete_from_db().conn(conn).call()?;
        }
      }
      let rows = new_refreshed_count
        .saturating_add(existing_refreshed_count)
        .saturating_add(missing_removed_count);

      Ok::<usize, anyhow::Error>(rows)
    })?;

    info!(
      "{} refresh: Completed in {:.1}s: Scanned {}/{} existing tracks; added {} new tracks; removed {} missing tracks",
      &self,
      start.elapsed().as_secs_f32(),
      existing_refreshed_count,
      existing_count,
      new_refreshed_count,
      missing_removed_count
    );

    Ok(rows)
  }

  #[builder]
  /// Fetch lyrics for tracks in this library, if track lyrics are missing or do not meet the
  /// requirements of `FetchLyricsOptions`.
  pub async fn fetch_lyrics(&self, options: Option<FetchLyricsOptions>) -> Result<usize> {
    // TODO: Use multiple async threads

    let options = {
      let settings = &*SETTINGS.read().map_err(|e| anyhow!("{e}"))?;
      options.unwrap_or_else(|| FetchLyricsOptions::from(settings))
    };
    info!(
      "{} fetch lyrics: Started with options: {:?}",
      &self, options
    );

    let start = Instant::now();
    let mut attempted_fetches = 0_usize;

    let tracks = self.tracks().call()?;

    for mut track in tracks {
      if options.update_lyrics_tag && track.lyrics.is_none()
        || ((!track.lyrics_synchronised && options.prefer_lyrics_type == LyricsType::Sync)
          || (track.lyrics_synchronised && options.prefer_lyrics_type == LyricsType::Plain))
        || options.save_sidecar_file
          && ((track.lyrics_sidecar_lrc_file.is_none()
            && options.prefer_lyrics_type == LyricsType::Sync)
            || (track.lyrics_sidecar_txt_file.is_none()
              && options.prefer_lyrics_type == LyricsType::Plain))
      {
        if track.fetch_lyrics().options(options).call().await.is_ok() {
          attempted_fetches += 1;
        }
      }
    }

    info!(
      "{} fetch lyrics: Completed in {:.1}s: Tried to fetch lyrics for {} tracks",
      &self,
      start.elapsed().as_secs_f32(),
      attempted_fetches
    );

    Ok(1)
  }

  /// Get a `Track` by its ID.
  #[builder]
  pub fn track(&self, id: i32, conn: Option<&mut SqliteConnection>) -> Result<Track> {
    let query = tracks::table
      .filter(tracks::library_id.eq(&self.id))
      .find(id);

    match conn {
      Some(conn) => Ok(query.first::<Track>(conn)?),
      None => {
        let mut conn = DB_POOL.get()?;
        Ok(query.first::<Track>(&mut conn)?)
      }
    }
  }

  /// Get all `Track`s.
  #[builder]
  pub fn tracks(&self, conn: Option<&mut SqliteConnection>) -> Result<Vec<Track>> {
    let query = tracks::table.filter(tracks::library_id.eq(&self.id));

    match conn {
      Some(conn) => Ok(query.load::<Track>(conn)?),
      None => {
        let mut conn = DB_POOL.get()?;
        Ok(query.load::<Track>(&mut conn)?)
      }
    }
  }

  /// Remove a library path and all `Track`s belonging to it (unless the `Track` exists in another
  /// library path). Consumes the `Library`.
  pub fn remove(self) -> Result<()> {
    let mut conn = DB_POOL.get()?;

    // Delete the library
    diesel::delete(libraries::table.filter(libraries::id.eq(self.id))).execute(&mut conn)?;

    info!("Deleted {}", &self);

    Ok(())
  }

  pub fn default_name(&self) -> String {
    self.path().file_name().unwrap_or("(invalid)").into()
  }

  /// The `Library`'s path.
  pub fn path(&self) -> Utf8PathBuf {
    Utf8PathBuf::from(&self.path)
  }

  /// All audio file paths within this `Library`'s path.
  pub fn file_paths(&self) -> HashSet<Utf8PathBuf> {
    WalkDir::new(&self.path)
      .into_iter()
      .filter_map(|e| e.ok())
      .filter(|e| e.file_type().is_file())
      .filter(|e| {
        e.path().extension().is_some_and(|ext| {
          ext
            .to_str()
            .is_some_and(|ext| AUDIO_FILE_EXTENSIONS.contains(&ext))
        })
      })
      .filter_map(|e| Utf8PathBuf::try_from(e.into_path()).ok())
      .collect::<HashSet<_>>()
  }

  /// The number of `Track`s in this library.
  pub fn size(&self) -> Result<usize> {
    let mut conn = DB_POOL.get()?;

    let size = tracks::table
      .filter(tracks::library_id.eq(&self.id))
      .count()
      .first::<i64>(&mut conn)?;

    Ok(size as usize)
  }
}

impl Display for Library {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "Library({})[\"{}\"]", &self.id, &self.path())
  }
}
