use std::sync::LazyLock;

use diesel::{
    SqliteConnection,
    r2d2::{self, ConnectionManager},
};

use crate::lrclib::LrcLibClient;

pub mod library;
pub mod lrclib;
pub mod lyrics;
pub mod schema;
pub mod track;
pub mod util;

pub type Result<T> = anyhow::Result<T>;
pub type DbPool = r2d2::Pool<ConnectionManager<SqliteConnection>>;

pub static DB_POOL: LazyLock<DbPool> = LazyLock::new(|| {
    let manager = r2d2::ConnectionManager::<SqliteConnection>::new("db.sqlite3");
    let pool = r2d2::Pool::builder()
        .build(manager)
        .expect("error creating database connection pool");
    dbg!(&pool);
    pool
});

pub static LRCLIB_CLIENT: LazyLock<LrcLibClient> = LazyLock::new(|| LrcLibClient::new());

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
