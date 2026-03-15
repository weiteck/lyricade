use std::sync::LazyLock;

use chrono::Utc;
use diesel::{
    SqliteConnection,
    connection::SimpleConnection,
    r2d2::{self, ConnectionManager},
};
use libsqlite3_sys::SQLITE_VERSION;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

use crate::lrclib::LrcLibClient;

pub mod library;
pub mod lrclib;
pub mod lyrics;
pub mod schema;
pub mod track;
pub mod util;

pub type Result<T> = anyhow::Result<T>;
pub type DbPool = r2d2::Pool<ConnectionManager<SqliteConnection>>;

pub const DB_PATH: &str = "db.sqlite3";

pub static DB_POOL: LazyLock<DbPool> = LazyLock::new(|| {
    let manager = r2d2::ConnectionManager::<SqliteConnection>::new(DB_PATH);
    r2d2::Pool::builder()
        .build(manager)
        .expect("error creating database connection pool")
});

pub static LRCLIB_CLIENT: LazyLock<LrcLibClient> = LazyLock::new(|| LrcLibClient::new());

/// Supported audio file types.
#[rustfmt::skip]
pub static AUDIO_FILE_EXTENSIONS: &[&str] = &[
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

pub fn init_app() -> Result<()> {
    init_logging();
    init_db_pool()?;

    let pkg_name_and_version = format!(
        ":::::::::::: {}  v{} ::::::::::::",
        std::env::var("CARGO_PKG_NAME")?,
        std::env::var("CARGO_PKG_VERSION")?
    );
    let separator = "`".repeat(pkg_name_and_version.len());
    info!(
        r#"
{}
{}

SQLite:   v{}
Database: {}
      "#,
        pkg_name_and_version,
        separator,
        SQLITE_VERSION.to_string_lossy(),
        DB_PATH
    );

    Ok(())
}

fn init_logging() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default tracing subscriber failed");
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

    Ok(())
}
