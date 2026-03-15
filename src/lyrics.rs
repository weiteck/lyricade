use std::{
    io::{Read, Write},
    sync::LazyLock,
};

use anyhow::anyhow;
use camino::{Utf8Path, Utf8PathBuf};
use derive_where::derive_where;
use regex::Regex;

use crate::{Result, track::Track};

/// Regex to match "\[00:00.000]\" or "\[0:00.0]\", indicating synchronised lyrics.
pub static SYNC_LYRICS_MATCH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\[(\d+):(\d{2})(?:\.(\d{1,3}))?\]").expect("should be valid regex")
});

/// Regex to match "\[00:00.000]\" or "\[0:00.0]\" followed by 0 or more whitespace chars ("[ \t]*").
pub static SYNC_LYRICS_STRIP_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\[(\d+):(\d{2})(?:\.(\d{1,3}))?\][ \t]*").expect("should be valid regex")
});

#[derive(Debug, Clone)]
#[derive_where(PartialOrd, Ord, Eq, PartialEq)]
pub struct Lyrics {
    pub lyrics_type: LyricsType,
    // Ignore lyrics when sorting so `LyricsType` + `LyricsFileType` controls order
    #[derive_where(skip)]
    pub contents: String,
}

impl Lyrics {
    /// If lyrics are synchronous, remove timestamps.
    pub fn into_plain(mut self) -> Self {
        if self.lyrics_type == LyricsType::Sync {
            self.contents = SYNC_LYRICS_STRIP_REGEX
                .replace_all(&self.contents, "")
                .to_string();
            self.lyrics_type = LyricsType::Plain;
        };
        self
    }
}

// Variants are given discriminants for sorting (lower values sorted first)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum LyricsType {
    #[default]
    Sync = 1,
    Plain = 2,
}

impl From<LyricsFileType> for LyricsType {
    fn from(value: LyricsFileType) -> Self {
        match value {
            LyricsFileType::Lrc => LyricsType::Sync,
            LyricsFileType::Txt => LyricsType::Plain,
        }
    }
}

// Variants are given discriminants for sorting (lower values sorted first)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LyricsFileType {
    // `Lrc` must be first for ordering when `Vec` is sorted
    Lrc = 1,
    Txt = 2,
}

impl From<LyricsType> for LyricsFileType {
    fn from(value: LyricsType) -> Self {
        match value {
            LyricsType::Sync => LyricsFileType::Lrc,
            LyricsType::Plain => LyricsFileType::Txt,
        }
    }
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

impl LyricsFileType {
    pub fn file_extension(&self) -> String {
        match &self {
            LyricsFileType::Lrc => "lrc".into(),
            LyricsFileType::Txt => "txt".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LyricsFile {
    pub lyrics: Lyrics,
    pub file_type: LyricsFileType,
    pub path: Utf8PathBuf,
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
                lyrics: Lyrics {
                    lyrics_type,
                    contents,
                },
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

    /// Write `lyrics.contents` to `path`. Any existing file will be overwritten.
    pub fn save(&self) -> Result<()> {
        let mut file = std::fs::File::create(&self.path)?;
        file.write_all(self.lyrics.contents.as_bytes())?;
        Ok(())
    }
}

/// Check if lyrics are synchronised using regex.
pub fn lyrics_are_synchronised(lyrics: &str) -> bool {
    SYNC_LYRICS_MATCH_REGEX.find(lyrics).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_lyrics_sorting() {
        // Best candidate - sync type + .lrc extension
        let best_sl_candidate = LyricsFile {
            lyrics: Lyrics {
                lyrics_type: LyricsType::Sync,
                contents: "4".into(),
            },
            file_type: LyricsFileType::Lrc,
            path: Utf8PathBuf::from("/x/y.lrc"),
        };
        // Worst candidate - non-sync type + .txt extension
        let worst_sl_candidate = LyricsFile {
            lyrics: Lyrics {
                lyrics_type: LyricsType::Plain,
                contents: "1".into(),
            },
            file_type: LyricsFileType::Txt,
            path: Utf8PathBuf::from("/x/y.txt"),
        };

        let mut vec = (0..2)
            .flat_map(|_| {
                [
                    LyricsFile {
                        lyrics: Lyrics {
                            lyrics_type: LyricsType::Plain,
                            contents: "3".into(),
                        },
                        file_type: LyricsFileType::Lrc,
                        path: Utf8PathBuf::from("/x/y.lrc"),
                    },
                    worst_sl_candidate.clone(),
                    best_sl_candidate.clone(),
                    LyricsFile {
                        lyrics: Lyrics {
                            lyrics_type: LyricsType::Sync,
                            contents: "2".into(),
                        },
                        file_type: LyricsFileType::Txt,
                        path: Utf8PathBuf::from("/x/y.txt"),
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
