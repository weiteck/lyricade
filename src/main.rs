// Prevent console window in addition to Slint window in Windows release builds
// when, e.g., starting the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::collections::HashSet;

use camino::Utf8PathBuf;

use lrc_lyrics::{
    init_app,
    library::{Library, RefreshOptions},
    track::{FetchLyricsOptions, ScanOptions},
    ui,
};
use mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_app()?;

    // One or more root library paths from args
    let library_paths = std::env::args()
        .skip(1)
        .map(Utf8PathBuf::from)
        .collect::<HashSet<Utf8PathBuf>>();

    for path in library_paths {
        let _added = Library::add(&path)?;
    }

    let scan_opts = ScanOptions {
        prefer_lyrics_type: lrc_lyrics::lyrics::LyricsType::Sync,
        upgrade_lyrics_tag: true,
        delete_sidecar_files: true,
        keep_one_sidecar_file: false,
    };

    let _refresh_opts = RefreshOptions {
        scan_new_only: true,
        scan_options: scan_opts,
    };

    if let Ok(lib) = Library::get(0) {
        let _track = lib.track().id(0).call()?;
    }

    let _fetch_opts = FetchLyricsOptions {
        prefer_lyrics_type: lrc_lyrics::lyrics::LyricsType::Sync,
        ignore_plain_lyrics: false,
        update_lyrics_tag: true,
        save_sidecar_file: true,
    };

    // _library.refresh().options(refresh_opts).call()?;
    // _library.fetch_lyrics().options(fetch_opts).call().await?;

    ui::start()?;

    Ok(())
}
