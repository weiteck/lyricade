use std::collections::HashSet;

use camino::Utf8PathBuf;

use lrc_lyrics::{
    init_app,
    library::{Library, RefreshOptions},
    track::{FetchLyricsOptions, ScanOptions},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_app()?;

    // One or more root library paths from args
    let library_paths = std::env::args()
        .skip(1)
        .map(Utf8PathBuf::from)
        .collect::<HashSet<Utf8PathBuf>>();

    let scan_opts = ScanOptions {
        preferred_lyrics_type: lrc_lyrics::lyrics::LyricsType::Sync,
        upgrade_lyrics_tag: true,
        delete_sidecar_files: true,
        keep_one_sidecar_file: false,
    };

    let refresh_opts = RefreshOptions {
        scan_new_only: true,
        scan_options: scan_opts,
    };

    for path in library_paths {
        let _added = Library::add(&path)?;
    }
    let _library = Library::get(1)?;
    _library.refresh().options(refresh_opts).call()?;

    let fetch_opts = FetchLyricsOptions {
        prefer_lyrics_type: lrc_lyrics::lyrics::LyricsType::Sync,
        ignore_plain_lyrics: false,
        update_lyrics_tag: true,
        save_sidecar_file: true,
    };

    _library.refresh().options(refresh_opts).call()?;
    _library.fetch_lyrics().options(fetch_opts).call().await?;
    // let mut track = library.track(3)?;
    // dbg!(&track);
    // track.fetch_lyrics_from_api(true).await?;
    // dbg!(&track);

    // let x = lyrics::sidecar_lyrics_from_track(&track::Track::default());
    // dbg!(x);

    Ok(())
}
