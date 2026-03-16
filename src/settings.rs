use std::{env, fs, io::Write, path::PathBuf, sync::LazyLock};

use camino::Utf8PathBuf;
use config::{Config, Environment, File};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{Result, library::RefreshOptions, track::FetchLyricsOptions};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Settings {
    refresh_library: RefreshOptions,
    fetch_lyrics: FetchLyricsOptions,
}

static PROJECT_DIRS: LazyLock<Option<ProjectDirs>> =
    LazyLock::new(|| ProjectDirs::from("io", "github.weiteck", &APP_NAME));

pub static APP_CONFIG_DIR: LazyLock<Utf8PathBuf> = LazyLock::new(|| {
    if cfg!(debug_assertions) {
        Utf8PathBuf::from("./config") // use project dir
    } else {
        let path = PROJECT_DIRS
            .as_ref()
            .map(|pd| pd.config_dir().to_path_buf())
            .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from("./config")));
        path.try_into()
            .expect("Encountered invalid UTF-8 path while parsing user config directory")
    }
});

pub static APP_DATA_DIR: LazyLock<Utf8PathBuf> = LazyLock::new(|| {
    if cfg!(debug_assertions) {
        Utf8PathBuf::from("./data") // use project dir
    } else {
        let path = PROJECT_DIRS
            .as_ref()
            .map(|pd| pd.data_dir().to_path_buf())
            .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from("./data")));
        path.try_into()
            .expect("Encountered invalid UTF-8 path while parsing user data directory")
    }
});

pub static APP_NAME: LazyLock<String> = LazyLock::new(|| env!("CARGO_PKG_NAME").into());

pub static APP_SETTINGS_FILE_PATH: LazyLock<Utf8PathBuf> =
    LazyLock::new(|| APP_CONFIG_DIR.join("settings.toml"));

pub static APP_DB_FILE_PATH: LazyLock<Utf8PathBuf> = LazyLock::new(|| {
    if cfg!(debug_assertions) {
        APP_DATA_DIR.join("db.dev.sqlite3") // use project dir
    } else {
        APP_DATA_DIR.join("db.sqlite3")
    }
});

impl Settings {
    pub fn init_or_load() -> Result<Self> {
        let settings_file_base = APP_SETTINGS_FILE_PATH.with_extension("");

        match Config::builder()
            .add_source(File::with_name(settings_file_base.as_str()))
            .add_source(Environment::with_prefix(&APP_NAME).separator("__"))
            .build()
        {
            Ok(config) => match config.try_deserialize() {
                Ok(settings) => {
                    info!("Loaded settings in \"{}\"", &*APP_SETTINGS_FILE_PATH);
                    Ok(settings)
                }
                Err(error) => {
                    warn!("Settings parse error: {error}");
                    warn!(
                        "Failed to load settings from \"{}\" - initialising with default settings",
                        &*APP_SETTINGS_FILE_PATH
                    );
                    Settings::init()
                }
            },
            Err(error) => match error {
                _ => {
                    warn!(
                        "Failed to load settings from \"{}\" - initialising with default settings",
                        &*APP_SETTINGS_FILE_PATH
                    );
                    Settings::init()
                }
            },
        }
    }

    pub fn save(&self) -> Result<()> {
        Settings::create_app_dirs_if_not_exist()?;
        let toml = toml::to_string_pretty(&self)?;
        let mut file = fs::File::create(&*APP_SETTINGS_FILE_PATH)?;
        file.write_all(toml.as_bytes())?;
        Ok(())
    }

    /// Create config and data dirs. Call before `init_or_load` of `Settings`.
    pub fn create_app_dirs_if_not_exist() -> Result<()> {
        if !&APP_CONFIG_DIR.exists() {
            fs::create_dir(&*APP_CONFIG_DIR)?;
        }
        if !&APP_DATA_DIR.exists() {
            fs::create_dir(&*APP_DATA_DIR)?;
        }
        Ok(())
    }

    fn init() -> Result<Self> {
        let settings = Self::default();
        settings.save()?;
        Ok(settings)
    }
}
