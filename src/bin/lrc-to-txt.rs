use std::{collections::HashSet, io::Write};

use camino::Utf8PathBuf;
use lyricade::lyrics::{LyricsFile, LyricsFileType, LyricsType};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

fn main() -> anyhow::Result<()> {
  init_logging();

  let lrc_paths = std::env::args()
    .skip(1)
    .map(Utf8PathBuf::from)
    .filter(|p| p.exists())
    .collect::<HashSet<Utf8PathBuf>>();

  for path in lrc_paths {
    let lf = LyricsFile::try_from_path(path.as_path())?;
    if lf.file_type == LyricsFileType::Lrc && lf.lyrics.lyrics_type == LyricsType::Sync {
      let lyrics = lf.lyrics.into_plain();
      std::fs::File::create(path.with_extension("txt"))?.write_all(lyrics.contents.as_bytes())?;
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
