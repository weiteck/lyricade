use std::collections::HashSet;

use camino::Utf8PathBuf;

use diesel::connection::SimpleConnection;
use lrc_lyrics::{DB_POOL, library::Library};
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();
    init_db_pool()?;

    // One or more root library paths from args
    let library_paths = std::env::args()
        .skip(1)
        .map(Utf8PathBuf::from)
        .collect::<HashSet<Utf8PathBuf>>();

    for path in library_paths {
        let _added = Library::add(&path)?;
    }
    let _library = Library::get(1)?;
    _library.refresh().call()?;
    // let mut track = library.track(3)?;
    // dbg!(&track);
    // track.fetch_lyrics_from_api(true).await?;
    // dbg!(&track);

    // let x = lyrics::sidecar_lyrics_from_track(&track::Track::default());
    // dbg!(x);

    Ok(())
}

fn init_logging() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default tracing subscriber failed");
}

fn init_db_pool() -> anyhow::Result<()> {
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

    info!(
        "SQLite v{}",
        libsqlite3_sys::SQLITE_VERSION.to_string_lossy()
    );

    Ok(())
}
