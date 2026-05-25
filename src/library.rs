use std::{
  collections::HashSet,
  fmt::Display,
  hash::Hash,
  time::{Duration, Instant},
};

use anyhow::anyhow;
use bon::bon;
use camino::{Utf8Path, Utf8PathBuf};
use chrono::NaiveDateTime;
use diesel::{
  dsl::insert_into,
  prelude::*,
  r2d2::{ConnectionManager, PooledConnection},
};

use relm4::tokio::sync::oneshot;
use tracing::{debug, error, info, warn};
use walkdir::WalkDir;

use crate::{
  AUDIO_FILE_EXTENSIONS, DB_POOL, Result, SETTINGS,
  lyrics::LyricsType,
  schema::{libraries, tracks},
  settings::Settings,
  track::{FetchLyricsOptions, NewTrack, Track},
  util::{self, now, reporter::IntervalReporter},
};

/// Represents a library path.
#[derive(
  Debug, Default, Clone, Eq, Queryable, Selectable, Identifiable, Insertable, AsChangeset,
)]
#[diesel(table_name = crate::schema::libraries)]
#[diesel(treat_none_as_null = true)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Library {
  pub id: i32,
  pub path: String,
  pub name: Option<String>,
  pub added_at: NaiveDateTime,
  pub updated_at: NaiveDateTime,
}

impl PartialEq for Library {
  fn eq(&self, other: &Self) -> bool {
    self.id == other.id && self.path == other.path
  }
}

impl Hash for Library {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.id.hash(state);
    self.path.hash(state);
  }
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
  // pub scan_options: track::CleanUpSidecarFilesOptions,
}

impl From<&Settings> for RefreshOptions {
  fn from(value: &Settings) -> Self {
    RefreshOptions {
      scan_new_only: value.scan_new_files_only,
    }
  }
}

#[bon]
impl Library {
  pub fn add(path: &Utf8Path) -> Result<Library> {
    let mut conn = DB_POOL.get()?;

    // Check if path is a directory
    if !path.is_dir() {
      error!("Library path \"{}\" is not a valid directory", &path);
      return Err(anyhow!("Invalid path"));
    }

    let existing_libraries = libraries::table.load::<Library>(&mut conn)?;

    // Check for existing `Library` with this path
    if let Some(existing_library) = existing_libraries
      .iter()
      .find(|&lib| lib.path().as_path() == path)
    {
      error!("Library path already exists as {}", existing_library);
      return Err(anyhow!("A library with this path already exists"));
    }

    // Check if this path is a subdirectory of an existing `Library`
    if let Some(existing_library) = existing_libraries
      .iter()
      .find(|lib| path.starts_with(&lib.path))
    {
      error!(
        "Path cannot be a subdirectory of an existing Library. Conflicts with {}",
        existing_library
      );
      return Err(anyhow!("Cannot be a subdirectory of an existing Library"));
    }

    // Check if this path is a parent directory of an existing `Library`
    if let Some(existing_library) = existing_libraries
      .iter()
      .find(|lib| lib.path().starts_with(path))
    {
      error!(
        "Path cannot be a parent directory of an existing Library. Conflicts with {}",
        existing_library
      );
      return Err(anyhow!("Cannot be a parent directory an existing Library"));
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
        error!("Database error while trying to get Library with ID {id}: {error}");
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
  /// Read metadata for new and (optionally) existing files in `Library` path and update database.
  pub fn refresh<F>(
    &self,
    on_progress: F,
    cancel_on_close: &mut oneshot::Receiver<()>,
  ) -> Result<usize>
  where
    F: Fn(String) + Send + 'static,
  {
    let scan_new_only = SETTINGS
      .read()
      .inspect_err(|_| error!("Settings lock was poisoned while refreshing {}", &self))
      .map_or_else(|_| Settings::default().scan_new_files_only, |g| g.scan_new_files_only);

    if scan_new_only {
      info!("{} refresh: Started (new/modified files only)", &self);
    } else {
      info!("{} refresh: Started", &self);
    }

    let start = Instant::now();
    let mut conn = DB_POOL.get()?;
    let now = now();

    on_progress("Discovering paths…\n".into());
    let paths = self
      .file_paths()
      .iter()
      .map(Utf8PathBuf::to_string)
      .collect::<HashSet<_>>();

    let mut existing_count = 0_usize;
    let mut existing_refreshed_count = 0_usize;
    let mut new_refreshed_count = 0_usize;
    let mut missing_removed_count = 0_usize;

    let rows = conn.immediate_transaction::<usize, _, _>(|conn| {
      // Get existing and missing tracks
      let (mut existing_tracks, missing_tracks): (Vec<Track>, Vec<Track>) = tracks::table
        .filter(tracks::library_id.eq(&self.id))
        .load::<Track>(conn)?
        .into_iter()
        .partition(|t| paths.contains(&t.path));
      existing_count = existing_tracks.len();

      // Discard unchanged files
      existing_tracks = if scan_new_only {
        on_progress("Checking for modified files…\n".into());

        let mut reporter = IntervalReporter::builder()
          .id("CheckFilesModified")
          .target(existing_count)
          .report_interval(Duration::from_secs(1))
          .report_threshold(5)
          .callback(|stats| {
            on_progress(format!(
              "Checking for modified files… {:.0} %\n(about {} remaining)",
              stats.percent_processed, stats.human_time_remaining
            ));
          })
          .build();

        let mut modified_tracks = Vec::with_capacity(existing_count);

        for track in existing_tracks {
          // Cancelled?
          if cancel_on_close
            .try_recv()
            .is_err_and(|error| error == oneshot::error::TryRecvError::Closed)
          {
            return Err(
              anyhow!(diesel::result::Error::RollbackTransaction)
                .context("User cancelled the library refresh operation"),
            );
          }

          if track.file_modified_at != util::file_modified_at().path(&track.path()).call() {
            modified_tracks.push(track);
          }

          // Track progress and report time remaining
          reporter.tick();
        }

        modified_tracks
      } else {
        existing_tracks
      };

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
        .map(|&path| NewTrack {
          library_id: self.id,
          path: path.clone(),
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

      let mut reporter = IntervalReporter::builder()
        .id("RefreshTracks")
        .target(inserted_new_tracks.len() + existing_tracks.len())
        .report_interval(Duration::from_secs(1))
        .report_threshold(5)
        .callback(|stats| {
          on_progress(format!(
            "Scanning tracks… {:.0} %\n(about {} remaining)",
            stats.percent_processed, stats.human_time_remaining
          ));
        })
        .build();

      on_progress("Scanning tracks…\n".into());

      // Get metadata from new files and write to database;
      // delete DB row where metadata read fails
      for mut track in inserted_new_tracks {
        // Task cancelled?
        if cancel_on_close
          .try_recv()
          .is_err_and(|error| error == oneshot::error::TryRecvError::Closed)
        {
          return Err(
            anyhow!(diesel::result::Error::RollbackTransaction)
              .context("User cancelled the library refresh operation"),
          );
        }

        if track.scan_and_update().conn(conn).call().is_ok() {
          new_refreshed_count += 1;
        } else {
          track.delete_from_db().conn(conn).call()?;
        }

        // Track progress and report time remaining
        reporter.tick();
      }

      // Refresh existing tracks
      for mut track in existing_tracks {
        // Task cancelled?
        if cancel_on_close
          .try_recv()
          .is_err_and(|error| error == oneshot::error::TryRecvError::Closed)
        {
          return Err(
            anyhow!(diesel::result::Error::RollbackTransaction)
              .context("User cancelled the library refresh operation"),
          );
        }

        if track.scan_and_update().conn(conn).call().is_ok() {
          existing_refreshed_count += 1;
        }

        // Track progress and report time remaining
        reporter.tick();
      }

      // Delete missing tracks (path not found)
      missing_removed_count = missing_tracks.len();
      if missing_removed_count > 0 {
        warn!("{} tracks not found will be removed", missing_removed_count);
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
    let options = {
      let settings = &*SETTINGS.read().map_err(|e| anyhow!("{e}"))?;
      options.unwrap_or_else(|| FetchLyricsOptions::from(settings))
    };
    info!("{} fetch lyrics: Started with options: {:?}", &self, options);

    let start = Instant::now();
    let mut attempted_fetches = 0_usize;

    let tracks = self.tracks().call()?;

    // Fetch if `Track` lyrics state does not match target state in `FetchLyricsOptions`
    for mut track in tracks {
      if (options.update_lyrics_tag && track.lyrics.is_none()
        || ((!track.lyrics_synchronised && options.prefer_lyrics_type == LyricsType::Sync)
          || (track.lyrics_synchronised && options.prefer_lyrics_type == LyricsType::Plain))
        || options.save_sidecar_file
          && ((track.lyrics_sidecar_lrc_file.is_none()
            && options.prefer_lyrics_type == LyricsType::Sync)
            || (track.lyrics_sidecar_txt_file.is_none()
              && options.prefer_lyrics_type == LyricsType::Plain)))
        && track.fetch_lyrics().options(options).call().await.is_ok()
      {
        attempted_fetches += 1;
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

    if let Some(conn) = conn {
      Ok(query.first::<Track>(conn)?)
    } else {
      let mut conn = DB_POOL.get()?;
      Ok(query.first::<Track>(&mut conn)?)
    }
  }

  /// Get all `Track`s.
  #[builder]
  pub fn tracks(
    &self,
    conn: Option<&mut PooledConnection<ConnectionManager<SqliteConnection>>>,
  ) -> Result<Vec<Track>> {
    let query = tracks::table.filter(tracks::library_id.eq(&self.id));

    if let Some(conn) = conn {
      Ok(query.load::<Track>(conn)?)
    } else {
      let mut conn = DB_POOL.get()?;
      Ok(query.load::<Track>(&mut conn)?)
    }
  }

  /// Remove a library path and all `Track`s belonging to it.
  pub fn remove(&self) -> Result<()> {
    let mut conn = DB_POOL.get()?;

    // Delete the library
    diesel::delete(libraries::table.filter(libraries::id.eq(self.id))).execute(&mut conn)?;

    info!("Deleted {}", &self);

    Ok(())
  }

  /// Get the `Library`'s name. Returns the `default_name` if none is set.
  #[must_use]
  pub fn name(&self) -> String {
    self.name.clone().unwrap_or_else(|| self.default_name())
  }

  /// The `Library`'s directory name.
  #[must_use]
  pub fn default_name(&self) -> String {
    self.path().file_name().unwrap_or("(invalid)").into()
  }

  /// Get the `Library`'s path.
  #[must_use]
  pub fn path(&self) -> Utf8PathBuf {
    Utf8PathBuf::from(&self.path)
  }

  /// Set the `Library`'s path.
  pub fn set_path(&mut self, new_path: &Utf8Path) -> Result<()> {
    let mut conn = DB_POOL.get()?;

    let existing_libraries = libraries::table.load::<Library>(&mut conn)?;

    // Check for existing `Library` with this path
    if let Some(existing_library) = existing_libraries
      .iter()
      .find(|&lib| lib.id != self.id && lib.path().as_path() == new_path)
    {
      error!("Path conflicts with existing Library: {}", existing_library);
      return Err(anyhow!("A library with this path already exists"));
    }

    // Check if this path is a subdirectory of an existing `Library`
    if let Some(existing_library) = existing_libraries
      .iter()
      .find(|&lib| lib.id != self.id && new_path.starts_with(&lib.path))
    {
      error!(
        "Path cannot be a subdirectory of an existing Library. Conflicts with {}",
        existing_library
      );
      return Err(anyhow!("Cannot be a subdirectory of an existing Library"));
    }

    // Check if this path is a parent directory of an existing `Library`
    if let Some(existing_library) = existing_libraries
      .iter()
      .find(|&lib| lib.id != self.id && lib.path().starts_with(new_path))
    {
      error!(
        "Path cannot be a parent directory of an existing Library. Conflicts with {}",
        existing_library
      );
      return Err(anyhow!("Cannot be a parent directory an existing Library"));
    }

    self.path = new_path.to_string();

    Ok(())
  }

  /// All audio file paths within this `Library`'s path.
  pub fn file_paths(&self) -> HashSet<Utf8PathBuf> {
    WalkDir::new(&self.path)
      .into_iter()
      .filter_map(core::result::Result::ok)
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

    #[expect(clippy::cast_possible_truncation)]
    Ok(size as usize)
  }

  /// Insert or update library in database.
  #[builder]
  pub fn write_to_db(&mut self) -> Result<()> {
    let mut conn = DB_POOL.get()?;

    self.updated_at = now();

    insert_into(libraries::table)
      .values(&*self)
      .on_conflict(libraries::id)
      .do_update()
      .set(&*self)
      .execute(&mut conn)?;

    debug!("Updated database entry for {}", &self);

    Ok(())
  }
}

impl Display for Library {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "Library({})[\"{}\"]", &self.id, &self.path())
  }
}
