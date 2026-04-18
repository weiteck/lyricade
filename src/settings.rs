use std::{env, fs, path::PathBuf, sync::LazyLock};

use camino::Utf8PathBuf;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use directories::ProjectDirs;
use tracing::{debug, error, info};

use crate::{DB_POOL, Result, lyrics::LyricsType, schema::settings, util::now};

static PROJECT_DIRS: LazyLock<Option<ProjectDirs>> =
  LazyLock::new(|| ProjectDirs::from("io", "github.weiteck", &APP_NAME));

pub static APP_DATA_DIR: LazyLock<Utf8PathBuf> = LazyLock::new(|| {
  if cfg!(debug_assertions) {
    Utf8PathBuf::from("./dev-data") // use project dir
  } else {
    let path = PROJECT_DIRS
      .as_ref()
      .map(|pd| pd.data_dir().to_path_buf())
      .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from("./data")));
    path
      .try_into()
      .expect("Encountered invalid UTF-8 path while parsing user data directory")
  }
});

pub const APP_ID: &str = "io.github.weiteck.Lyrinc";

/// Application name used in paths, etc.
pub const APP_NAME: &str = "lyrinc";

/// Application name presented in UI.
pub const APP_NAME_PRETTY: &str = "Lyrinc";

pub static APP_DB_FILE_PATH: LazyLock<Utf8PathBuf> = LazyLock::new(|| {
  if cfg!(debug_assertions) {
    APP_DATA_DIR.join("db.dev.sqlite3") // use project dir
  } else {
    APP_DATA_DIR.join("db.sqlite3")
  }
});

/// Maximum concurrent HTTP connections.
pub const CONNECTION_LIMIT: usize = 20;

#[derive(
  Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Insertable, AsChangeset,
)]
#[diesel(table_name = crate::schema::settings)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Settings {
  pub id: i32,

  pub prefer_iso_timestamps: bool,
  pub prefer_lyrics_type: LyricsType,

  pub scan_new_files_only: bool,
  pub upgrade_lyrics_tag_on_scan: bool,
  pub delete_sidecar_files_on_scan: bool,
  pub keep_one_sidecar_file_on_scan: bool,

  pub ignore_plain_lyrics_on_fetch: bool,
  pub update_lyrics_tag_on_fetch: bool,
  pub save_sidecar_file_on_fetch: bool,

  pub window_width: i32,
  pub window_height: i32,

  #[diesel(skip_update)]
  pub added_at: NaiveDateTime,
  pub updated_at: NaiveDateTime,
}

impl Settings {
  pub fn load() -> Result<Self> {
    Self::create_app_dirs_if_not_exist()?;

    let mut conn = DB_POOL.get()?;

    let res = settings::table
      .find(1) // singleton table; always id = 1
      .first::<Settings>(&mut conn);

    let settings = match res {
      Ok(settings) => {
        info!("Loaded settings");
        Ok(settings)
      }
      Err(error) => match error {
        diesel::result::Error::NotFound => {
          info!("Initialising default settings");

          let mut settings = Settings::default();
          settings.save()?;

          Ok(settings)
        }
        _ => {
          error!("Database error while trying to load Settings: {error}");
          Err(error)
        }
      },
    }?;

    Ok(settings)
  }

  pub fn save(&mut self) -> Result<()> {
    let mut conn = DB_POOL.get()?;

    self.updated_at = now();

    if diesel::insert_into(settings::table)
      .values(&*self)
      .on_conflict(settings::id) // singleton table; id = 1 and should always conflict
      .do_update()
      .set(&*self)
      .execute(&mut conn)
      .inspect_err(|error| error!("Database error while trying to save Settings: {error}"))?
      == 1
    {
      debug!("Saved Settings");
    }

    Ok(())
  }

  /// Create data directory.
  pub fn create_app_dirs_if_not_exist() -> Result<()> {
    if !&APP_DATA_DIR.exists() {
      fs::create_dir(&*APP_DATA_DIR)?;
    }
    Ok(())
  }
}

impl Default for Settings {
  fn default() -> Self {
    let now = now();
    Self {
      id: 1,
      prefer_iso_timestamps: false,
      prefer_lyrics_type: LyricsType::Sync,
      scan_new_files_only: false,
      upgrade_lyrics_tag_on_scan: false,
      delete_sidecar_files_on_scan: false,
      keep_one_sidecar_file_on_scan: false,
      ignore_plain_lyrics_on_fetch: false,
      update_lyrics_tag_on_fetch: false,
      save_sidecar_file_on_fetch: true,
      window_width: 1000,
      window_height: 600,
      added_at: now,
      updated_at: now,
    }
  }
}
