#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::{
  fs,
  path::Path,
  sync::{LazyLock, RwLock},
};

use anyhow::anyhow;
use diesel::{
  SqliteConnection,
  connection::SimpleConnection,
  r2d2::{self, ConnectionManager},
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use libsqlite3_sys::SQLITE_VERSION;
use mimalloc::MiMalloc;
use tracing::{debug, info, warn};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
  EnvFilter,
  fmt::{self, format::FmtSpan},
  prelude::*,
};

use crate::{
  lrclib::LrcLibClient,
  settings::{APP_DATA_DIR, APP_DB_FILE_PATH, APP_NAME, Settings},
};

pub(crate) mod library;
pub(crate) mod lrclib;
pub(crate) mod lyrics;
pub(crate) mod manage;
pub(crate) mod schema;
pub(crate) mod settings;
pub(crate) mod tags;
pub(crate) mod track;
pub(crate) mod ui;
pub(crate) mod util;

pub(crate) type Result<T> = anyhow::Result<T>;
pub(crate) type DbPool = r2d2::Pool<ConnectionManager<SqliteConnection>>;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

const MAX_LOG_FILES: usize = 10;

static LOG_WORKER_GUARD: LazyLock<WorkerGuard> = LazyLock::new(init_logging);

pub static USER_AGENT: LazyLock<String> = LazyLock::new(|| {
  format!(
    "{} v{} ({})",
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_VERSION"),
    env!("CARGO_PKG_REPOSITORY")
  )
});

pub static NUM_LOCALE: LazyLock<num_format::SystemLocale> = LazyLock::new(|| {
  num_format::SystemLocale::default().unwrap_or_else(|_| {
    num_format::SystemLocale::from_name("en_US").expect("should be a valid locale name")
  })
});

pub(crate) static SETTINGS: LazyLock<RwLock<Settings>> =
  LazyLock::new(|| RwLock::new(Settings::load().expect("Failed to load settings from database")));

pub(crate) static DB_POOL: LazyLock<DbPool> = LazyLock::new(|| {
  let manager = r2d2::ConnectionManager::<SqliteConnection>::new(APP_DB_FILE_PATH.to_string());
  r2d2::Pool::builder()
    .build(manager)
    .expect("error creating database connection pool")
});

pub(crate) static LRCLIB_CLIENT: LazyLock<LrcLibClient> = LazyLock::new(LrcLibClient::new);

/// Supported audio file types.
#[rustfmt::skip]
pub(crate) static AUDIO_FILE_EXTENSIONS: &[&str] = &[
    "aac",
    "ape",
    "aif", "aiff",
    "flac",
    "mp3",
    "mp4", "m4a",
    "mpc",
    "opus",
    "ogg",
    "spx",
    "wav",
    "wv",
];

pub(crate) fn init_app() -> Result<()> {
  // Trigger `LazyLock` to run `init_logging` function. `WorkerGuard` of the log file appender
  // is stored in a static so it is not dropped for the duration of the program
  let _guard = &*LOG_WORKER_GUARD;

  if cfg!(debug_assertions) {
    warn!("Started in DEBUG mode");
  }

  // Create data dir before initialise database
  Settings::create_app_dirs_if_not_exist()?;

  init_db_pool()?;

  let pkg_name_and_version =
    format!(":::::::::::: {}  v{} ::::::::::::", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
  let separator = "`".repeat(pkg_name_and_version.len());
  let settings = &*SETTINGS
    .read()
    .map_err(|_| anyhow!("Settings lock is poisoned"))?;
  info!(
    r"
{}
{}

SQLite:        v{}
Database:      {}
Settings:
{:#?}
      ",
    pkg_name_and_version,
    separator,
    SQLITE_VERSION.to_string_lossy(),
    &*APP_DB_FILE_PATH
      .canonicalize_utf8()
      .unwrap_or("(error while getting full path)".into()),
    settings
  );

  Ok(())
}

// `WorkerGuard` must be held for duration of the program
fn init_logging() -> WorkerGuard {
  let default_log_level = if cfg!(debug_assertions) {
    "debug"
  } else {
    "info"
  };

  let filter =
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_log_level));

  let mut log_name = APP_NAME.to_string();
  log_name.push_str(".log");
  let log_dir = &APP_DATA_DIR.join("logs");
  fs::create_dir_all(log_dir)
    .unwrap_or_else(|error| panic!("failed to create logging directory \"{log_dir}\": {error}"));

  let file_appender = tracing_appender::rolling::daily(log_dir, log_name);
  let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

  let span_events_enabled = std::env::var("SPAN_EVENTS")
    .is_ok_and(|v| v.as_str() == "1" || v.to_lowercase().as_str() == "true");

  // File logging
  let layers =
    tracing_subscriber::registry().with(fmt::layer().with_ansi(false).with_writer(non_blocking));

  // Console logging
  let layers = if span_events_enabled {
    layers.with(
      fmt::layer()
        .with_span_events(FmtSpan::CLOSE)
        .with_ansi(true),
    )
  } else {
    layers.with(fmt::layer().with_ansi(true))
  };

  layers.with(filter).init();

  clean_up_log_files(log_dir).expect("failed to clean up log files");

  guard
}

fn clean_up_log_files(log_dir: impl AsRef<Path>) -> Result<()> {
  let prefix = format!("{}.log", APP_NAME);
  let mut files = fs::read_dir(log_dir)?
    .filter_map(core::result::Result::ok)
    .filter(|de| de.path().is_file() && de.file_name().to_string_lossy().starts_with(&prefix))
    .collect::<Vec<_>>();
  files.sort_by_cached_key(|de| {
    de.metadata()
      .expect("should be able to read log file metadata")
      .modified()
      .expect("should be able to read log file modified date")
  });

  debug!("Found {} existing log files (keeping: {MAX_LOG_FILES})", files.len());

  for file in files.iter().rev().skip(MAX_LOG_FILES) {
    debug!("Deleting old log file \"{}\"", file.file_name().to_string_lossy());

    fs::remove_file(file.path())?;
  }

  Ok(())
}

fn init_db_pool() -> Result<()> {
  let mut conn = DB_POOL.get()?;

  // see https://fractaledmind.github.io/2023/09/07/enhancing-rails-sqlite-fine-tuning/
  // sleep if the database is busy, this corresponds to up to 2 seconds sleeping time.
  conn.batch_execute("PRAGMA busy_timeout = 2000;")?;
  // better write-concurrency
  conn.batch_execute("PRAGMA journal_mode = WAL;")?;
  // fsync only in critical moments
  conn.batch_execute("PRAGMA synchronous = NORMAL;")?;
  // write WAL changes back every 1000 pages, for an in average 1MB WAL file.
  // May affect readers if number is increased
  conn.batch_execute("PRAGMA wal_autocheckpoint = 1000;")?;
  // free some space by truncating possibly massive WAL files from the last run
  conn.batch_execute("PRAGMA wal_checkpoint(TRUNCATE);")?;

  // enforce FK constraint
  conn.batch_execute("PRAGMA foreign_keys = ON;")?;

  // Run database migrations
  if let Ok(pending) = conn
    .pending_migrations(MIGRATIONS)
    .map_err(|error| anyhow!("Failed to get pending database migrations: {error}"))
  {
    for (idx, m) in pending.iter().enumerate() {
      info!("Applying database migration {}/{}", idx + 1, pending.len());
      conn
        .run_migration(m)
        .map_err(|error| anyhow!("Failed to apply database migration: {error}"))?;
    }
  }

  Ok(())
}

fn main() -> Result<()> {
  // Prepare logging, database and settings
  init_app()?;

  // Show the GUI
  ui::app::start();

  Ok(())
}
