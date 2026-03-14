use std::{collections::HashSet, io::Write};

use camino::Utf8PathBuf;
use lrc_lyrics::lyrics::{LyricsFile, LyricsFileType, LyricsType};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

fn main() -> anyhow::Result<()> {
    init_logging();

    let lrc_paths = std::env::args()
        .skip(1)
        .map(Utf8PathBuf::from)
        .filter(|p| p.exists())
        .collect::<HashSet<Utf8PathBuf>>();

    // Regex to match "[00:00.000]" or "[0:00.0]" followed by 0 or more whitespace chars ("[ \t]*")
    let re = regex::Regex::new(r"\[(\d+):(\d{2})(?:\.(\d{1,3}))?\][ \t]*")
        .expect("should be valid regex");

    for path in lrc_paths {
        let lf = LyricsFile::try_from_path(path.as_path())?;
        if lf.file_type == LyricsFileType::Lrc && lf.lyrics_type == LyricsType::Sync {
            let stripped = re.replace_all(&lf.contents, "");
            std::fs::File::create(path.with_extension("txt"))?.write_all(stripped.as_bytes())?;
        }
    }

    Ok(())
}

fn init_logging() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default tracing subscriber failed");
}
