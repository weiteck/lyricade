use std::{collections::HashSet, fmt::Display};

use anyhow::anyhow;
use bon::bon;
use camino::{Utf8Path, Utf8PathBuf};
use chrono::NaiveDateTime;
use diesel::{dsl::insert_into, prelude::*};

use tracing::{error, info, trace, warn};
use walkdir::WalkDir;

use crate::{
    AUDIO_FILE_EXTENSIONS, DB_POOL, Result, lyrics,
    schema::{libraries, tracks},
    track::{self, NewTrack, Track},
    util::now,
};

/// Represents a library path.
#[derive(Debug, Default, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::schema::libraries)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Library {
    pub id: i32,
    pub path: String,
    pub added_at: NaiveDateTime,
}

#[derive(Debug, Default, Clone, Insertable)]
#[diesel(table_name = crate::schema::libraries)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct NewLibrary {
    pub path: String,
    pub added_at: NaiveDateTime,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RefreshOptions {
    pub scan_new_only: bool,
    pub scan_options: track::ScanOptions,
}

#[derive(Debug, Clone, Copy)]
pub struct FetchLyricsOptions {
    pub prefer_lyrics_type: lyrics::LyricsType,
    pub ignore_other_type: bool,
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

        let inserted_library = insert_into(libraries::table)
            .values(NewLibrary {
                path: path.to_string(),
                added_at: now(),
            })
            .get_result::<Library>(&mut conn)?;

        info!("Inserted {}", &inserted_library);

        // Add tracks with metadata
        inserted_library.refresh().call()?;

        Ok(inserted_library)
    }

    /// Get a `Library` by its ID.
    pub fn get(id: i32) -> Result<Library> {
        let mut conn = DB_POOL.get()?;

        let library = libraries::table
            .find(id)
            .first::<Library>(&mut conn)
            .inspect_err(|error| {
                error!("Database error while trying to get Library[{id}]: {error}")
            })?;

        Ok(library)
    }

    #[builder]
    /// Read metadata for new and existing files in `Library` path and update database.
    pub fn refresh(&self, options: Option<RefreshOptions>) -> Result<usize> {
        let options = options.unwrap_or_default();
        trace!("{} refresh started with options:\n{:#?}", &self, options);

        let mut conn = DB_POOL.get()?;
        let now = now();
        let paths = self
            .file_paths()
            .iter()
            .map(|p| p.to_string())
            .collect::<HashSet<_>>();
        let mut existing_refreshed_count = 0_usize;
        let mut new_refreshed_count = 0_usize;
        let mut missing_count = 0_usize;

        let rows = conn.transaction::<usize, _, _>(|conn| {
            // Get existing and missing tracks
            let (existing_tracks, missing_tracks): (Vec<Track>, Vec<Track>) = tracks::table
                .filter(tracks::library_id.eq(&self.id))
                .load::<Track>(conn)?
                .into_iter()
                .partition(|t| paths.contains(&t.path));

            dbg!(&missing_tracks);

            // Build new tracks for non-existing paths
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

            // Refresh existing tracks
            // TODO: This should be optional (i.e. a 'full scan')
            existing_refreshed_count = existing_tracks
                .into_iter()
                .filter_map(|mut t| {
                    t.scan_and_update()
                        .options(options.scan_options)
                        .conn(conn)
                        .call()
                        .ok()
                })
                .count();

            // Clean up rows created for new tracks that we failed to get metadata for
            for track in errored_new_tracks {
                track.delete_from_db().conn(conn).call()?;
            }

            // Delete missing tracks (path not found)
            missing_count = missing_tracks.len();
            if missing_count > 0 {
                warn!(
                    "{} tracks not found and will be removed: {:?}",
                    missing_count,
                    missing_tracks.iter().map(|t| &t.path).collect::<Vec<_>>()
                );
                for track in missing_tracks {
                    track.delete_from_db().conn(conn).call()?;
                }
            }
            let rows = new_refreshed_count
                .saturating_add(existing_refreshed_count)
                .saturating_add(missing_count);

            Ok::<usize, anyhow::Error>(rows)
        })?;

        if rows == paths.len() {
            let args = if missing_count == 0 {
                format_args!(
                    "{}: Updated {} tracks ({} new)",
                    &self, rows, new_refreshed_count
                )
            } else {
                format_args!(
                    "{}: Updated {} tracks ({} new); removed {} missing tracks",
                    &self, rows, new_refreshed_count, missing_count
                )
            };
            info!("{args}");
        } else {
            warn!(
                "{}: Failed to update {} tracks ({} of {} inserted; {} new; removed {} missing tracks)",
                &self,
                paths.len().saturating_sub(rows),
                rows,
                paths.len(),
                new_refreshed_count,
                missing_count
            );
        }

        Ok(rows)
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
                    ext.to_str()
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

    /// Get a `Track` by its ID.
    #[builder]
    pub fn track(&self, track_id: i32, conn: Option<&mut SqliteConnection>) -> Result<Track> {
        let query = tracks::table
            .filter(tracks::library_id.eq(&self.id))
            .find(track_id);

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
}

impl Display for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Library[{}] @ {}", &self.id, &self.path())
    }
}
