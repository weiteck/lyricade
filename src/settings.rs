use std::{env, fs, path::PathBuf, sync::LazyLock};

use camino::Utf8PathBuf;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use directories::ProjectDirs;
use tracing::{debug, error, info};

use crate::{
  DB_POOL, Result, lyrics::LyricsType, schema::settings, ui::app::get_lyrics_menu, util::now,
};

static PROJECT_DIRS: LazyLock<Option<ProjectDirs>> =
  LazyLock::new(|| ProjectDirs::from("io", "github.weiteck", APP_NAME));

pub(crate) static APP_DATA_DIR: LazyLock<Utf8PathBuf> = LazyLock::new(|| {
  if cfg!(debug_assertions) {
    Utf8PathBuf::from("./dev-data") // use project dir
  } else {
    let path = PROJECT_DIRS.as_ref().map_or_else(
      || env::current_dir().unwrap_or_else(|_| PathBuf::from(&format!("./{APP_NAME}-data"))),
      |pd| pd.data_dir().to_path_buf(),
    );
    path
      .try_into()
      .expect("Encountered invalid UTF-8 path while parsing user data directory")
  }
});

pub(crate) const APP_ID: &str = "io.github.weiteck.Lyricade";

/// Application name used in paths, etc.
pub(crate) const APP_NAME: &str = "lyricade";

/// Application name presented in UI.
pub(crate) const APP_NAME_PRETTY: &str = "Lyricade";

pub(crate) static APP_DB_FILE_PATH: LazyLock<Utf8PathBuf> = LazyLock::new(|| {
  if cfg!(debug_assertions) {
    APP_DATA_DIR.join("db.dev.sqlite3") // use project dir
  } else {
    APP_DATA_DIR.join("db.sqlite3")
  }
});

/// Maximum concurrent HTTP connections.
pub(crate) const CONNECTION_LIMIT: usize = 20;

#[expect(clippy::struct_excessive_bools)]
#[derive(
  Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable, Insertable, AsChangeset,
)]
#[diesel(table_name = crate::schema::settings)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub(crate) struct Settings {
  pub(crate) id: i32,

  /// Prefer full datetime or humanised representation (e.g. "5 minutes ago").
  pub(crate) prefer_accurate_timestamps: bool,
  pub(crate) scan_new_files_only: bool,
  pub(crate) plain_lyrics_in_id3v2_uslt_frame: bool,

  pub(crate) prefer_lyrics_type: LyricsType,
  pub(crate) ignore_plain_lyrics_on_fetch: bool,
  pub(crate) update_lyrics_tag_on_fetch: bool,
  pub(crate) save_sidecar_file_on_fetch: bool,

  pub(crate) get_lyrics_menu_lyrics_type: get_lyrics_menu::Type,
  pub(crate) get_lyrics_menu_last_checked: get_lyrics_menu::Checked,
  pub(crate) get_lyrics_menu_target_visible: bool,
  pub(crate) get_lyrics_menu_target_selected: bool,

  // GUI state
  pub(crate) window_width: i32,
  pub(crate) window_height: i32,
  pub(crate) sidebar_pinned: bool,

  #[diesel(skip_update)]
  pub(crate) added_at: NaiveDateTime,
  pub(crate) updated_at: NaiveDateTime,
}

impl Settings {
  pub(crate) fn load() -> Result<Self> {
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
      Err(error) => {
        if error == diesel::result::Error::NotFound {
          info!("Initialising default settings");

          let mut settings = Settings::default();
          settings.save()?;

          Ok(settings)
        } else {
          error!("Database error while trying to load Settings: {error}");
          Err(error)
        }
      }
    }?;

    Ok(settings)
  }

  pub(crate) fn save(&mut self) -> Result<()> {
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
  pub(crate) fn create_app_dirs_if_not_exist() -> Result<()> {
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
      prefer_accurate_timestamps: false,
      scan_new_files_only: true,
      plain_lyrics_in_id3v2_uslt_frame: false,

      prefer_lyrics_type: LyricsType::Sync,
      ignore_plain_lyrics_on_fetch: false,
      update_lyrics_tag_on_fetch: false,
      save_sidecar_file_on_fetch: true,

      get_lyrics_menu_lyrics_type: get_lyrics_menu::Type::default(),
      get_lyrics_menu_last_checked: get_lyrics_menu::Checked::default(),
      get_lyrics_menu_target_visible: false,
      get_lyrics_menu_target_selected: false,

      window_width: 1000,
      window_height: 600,
      sidebar_pinned: false,

      added_at: now,
      updated_at: now,
    }
  }
}
