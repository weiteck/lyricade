use std::{io::Read, sync::LazyLock};

use anyhow::anyhow;
use camino::{Utf8Path, Utf8PathBuf};
use derive_where::derive_where;
use regex::Regex;

use crate::{Result, track::Track};

/// Regex to match "\[00:00.000\]" and similar, indicating synchronised lyrics.
pub static SYNCHRONISED_LYRICS_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\[(\d+):(\d{2})(?:\.(\d{1,3}))?\]").expect("should be valid regex")
});

#[derive(Debug, Clone)]
#[derive_where(PartialOrd, Ord, Eq, PartialEq)]
pub struct Lyrics {
    pub lyrics_type: LyricsType,
    // Ignore lyrics when sorting so `LyricsType` + `LyricsFileType` controls order
    #[derive_where(skip)]
    pub contents: String,
}

// Variants are given discriminants for sorting (lower values sorted first)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LyricsType {
    Sync = 1,
    Plain = 2,
}

// Variants are given discriminants for sorting (lower values sorted first)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LyricsFileType {
    // `Lrc` must be first for ordering when `Vec` is sorted
    Lrc = 1,
    Txt = 2,
}

impl TryFrom<&Utf8Path> for LyricsFileType {
    type Error = anyhow::Error;

    fn try_from(path: &Utf8Path) -> std::result::Result<Self, Self::Error> {
        match path.extension() {
            Some("lrc") => Ok(LyricsFileType::Lrc),
            Some("txt") => Ok(LyricsFileType::Txt),
            Some(ext) => Err(anyhow!(
                "\"{}\" is not a supported lyrics sidecar file extension (\"lrc\", \"txt\")",
                ext
            )),
            _ => Err(anyhow!("lyrics sidecar file must have an extension")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LyricsFile {
    pub lyrics_type: LyricsType,   // Sort by type..
    pub file_type: LyricsFileType, // ..then by extension (path is otherwise identical)
    pub path: Utf8PathBuf,
    pub contents: String,
}

impl TryFrom<&Utf8Path> for LyricsFile {
    type Error = anyhow::Error;

    fn try_from(path: &Utf8Path) -> std::result::Result<Self, Self::Error> {
        Self::try_from_path(path)
    }
}

impl TryFrom<Utf8PathBuf> for LyricsFile {
    type Error = anyhow::Error;

    fn try_from(path: Utf8PathBuf) -> std::result::Result<Self, Self::Error> {
        Self::try_from_path(path.as_path())
    }
}

impl LyricsFile {
    /// Try to parse a file as a sync or plain `LyricsFiles`.
    pub fn try_from_path(path: &Utf8Path) -> Result<Self> {
        let mut file = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&path)?;

        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_ok_and(|u| u != 0) {
            let lyrics_type = if lyrics_are_synchronised(&contents) {
                LyricsType::Sync
            } else {
                LyricsType::Plain
            };

            let file_type = LyricsFileType::try_from(path)?;

            return Ok(LyricsFile {
                lyrics_type,
                contents,
                file_type,
                path: path.into(),
            });
        }

        Err(anyhow!(""))
    }

    /// Find and return sidecar lyrics files that are alongside the `Track` file.
    /// Collection is sorted so best 'sync' candidate is yielded first and plain lyrics last.
    pub fn from_track(track: &Track) -> Option<Vec<Self>> {
        let track_path = Utf8PathBuf::from(&track.path());

        let mut vec = ["lrc", "txt"]
            .into_iter()
            .map(|ext| track_path.with_extension(ext))
            .filter_map(|p| LyricsFile::try_from_path(&p).ok())
            .collect::<Vec<_>>();

        if !vec.is_empty() {
            // First item should be best sync candidate based on type (from regex test) and file extension
            vec.sort();
            Some(vec)
        } else {
            None
        }
    }
}

/// Check if lyrics are synchronised using regex.
pub fn lyrics_are_synchronised(lyrics: &str) -> bool {
    SYNCHRONISED_LYRICS_REGEX.find(lyrics).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_lyrics_sorting() {
        // Best candidate - sync type + .lrc extension
        let best_sl_candidate = LyricsFile {
            lyrics_type: LyricsType::Sync,
            file_type: LyricsFileType::Lrc,
            path: Utf8PathBuf::from("/x/y.lrc"),
            contents: "4".into(),
        };
        // Worst candidate - non-sync type + .txt extension
        let worst_sl_candidate = LyricsFile {
            lyrics_type: LyricsType::Plain,
            file_type: LyricsFileType::Txt,
            path: Utf8PathBuf::from("/x/y.txt"),
            contents: "1".into(),
        };

        let mut vec = (0..2)
            .flat_map(|_| {
                [
                    LyricsFile {
                        lyrics_type: LyricsType::Plain,
                        file_type: LyricsFileType::Lrc,
                        path: Utf8PathBuf::from("/x/y.lrc"),
                        contents: "3".into(),
                    },
                    worst_sl_candidate.clone(),
                    best_sl_candidate.clone(),
                    LyricsFile {
                        lyrics_type: LyricsType::Sync,
                        file_type: LyricsFileType::Txt,
                        path: Utf8PathBuf::from("/x/y.txt"),
                        contents: "2".into(),
                    },
                ]
            })
            .collect::<Vec<_>>();
        vec.sort();

        let sl = vec.first();
        assert_eq!(
            best_sl_candidate,
            *sl.unwrap(),
            "best sync lyrics candidate should be sorted first"
        );
        let sl = vec.last();
        assert_eq!(
            worst_sl_candidate,
            *sl.unwrap(),
            "worst sync lyrics candidate should be sorted last"
        );
    }
}
