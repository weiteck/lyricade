use std::{
  fmt::Display,
  io::{Read, Write},
};

use anyhow::anyhow;
use camino::{Utf8Path, Utf8PathBuf};
use derive_where::derive_where;
use diesel::{
  backend::Backend,
  deserialize::{FromSql, FromSqlRow},
  expression::AsExpression,
  serialize::ToSql,
  sql_types::Text,
  sqlite::Sqlite,
};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::{
  Result,
  lyrics::lrc::{LRC_LYRICS_REGEX, LRC_LYRICS_STRIP_REGEX, LRC_TAG_REGEX},
  track::Track,
};

pub mod lrc;
pub mod lyrics_line;

#[derive(Debug, Clone)]
#[derive_where(PartialOrd, Ord, Eq, PartialEq)]
pub struct Lyrics {
  pub lyrics_type: LyricsType,
  // Ignore lyrics when sorting so `LyricsType` + `LyricsFileType` controls order
  #[derive_where(skip)]
  pub contents: String,
}

impl Lyrics {
  /// If lyrics are synchronous, remove timestamps, tags, and comments.
  /// Noop for `LyricsType::Plain`.
  #[must_use]
  pub fn into_plain(mut self) -> Self {
    if self.lyrics_type == LyricsType::Sync {
      self.contents = self
        .contents
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::trim)
        // Skip comments and tags
        .filter(|line| !(line.starts_with('#') || LRC_TAG_REGEX.is_match(line)))
        // Remove timestamps; add new line
        .fold(String::with_capacity(self.contents.len()), |mut buffer, line| {
          let stripped = LRC_LYRICS_STRIP_REGEX.replace(line, "");
          buffer.push_str(&stripped);
          buffer.push('\n');
          buffer
        });

      self.lyrics_type = LyricsType::Plain;
    }

    self
  }
}

// Variants are given discriminants for sorting (lower values sorted first)
#[derive(
  Debug,
  Clone,
  Copy,
  Default,
  PartialEq,
  Eq,
  PartialOrd,
  Ord,
  Serialize,
  Deserialize,
  AsExpression,
  FromSqlRow,
)]
#[diesel(sql_type = Text)]
pub enum LyricsType {
  #[default]
  Sync = 1,
  Plain = 2,
}

impl FromSql<Text, Sqlite> for LyricsType {
  fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> diesel::deserialize::Result<Self> {
    let s = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?;
    ron::from_str(&s).map_err(|error| {
      error!("Error deserializing enum `LyricsType` from database value \"{s}\": {error}");
      error.into()
    })
  }
}

impl ToSql<Text, Sqlite> for LyricsType {
  fn to_sql<'b>(
    &'b self,
    out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
  ) -> diesel::serialize::Result {
    out.set_value(ron::to_string(&self)?);
    Ok(diesel::serialize::IsNull::No)
  }
}

impl Display for LyricsType {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let s = match self {
      LyricsType::Sync => "Sync",
      LyricsType::Plain => "Plain",
    };
    write!(f, "{s}")
  }
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
        "\"{ext}\" is not a supported lyrics sidecar file extension (\"lrc\", \"txt\")"
      )),
      _ => Err(anyhow!("lyrics sidecar file must have an extension")),
    }
  }
}

impl LyricsFileType {
  #[must_use]
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
    let mut file = std::fs::File::options().read(true).write(true).open(path)?;

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
  #[must_use]
  pub fn from_track(track: &Track) -> Option<Vec<Self>> {
    let track_path = Utf8PathBuf::from(&track.path());

    let mut vec = ["lrc", "txt"]
      .into_iter()
      .map(|ext| track_path.with_extension(ext))
      .filter_map(|p| LyricsFile::try_from_path(&p).ok())
      .collect::<Vec<_>>();

    if vec.is_empty() {
      None
    } else {
      // First item should be best sync candidate based on type (from regex test) and file extension
      vec.sort();
      Some(vec)
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
  LRC_LYRICS_REGEX.find(lyrics).is_some()
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

  #[test]
  fn sync_lyrics_to_plain() {
    let sync_lyrics = Lyrics {
      lyrics_type: LyricsType::Sync,
      contents: r"
# comment preceded by empty line
[ar:Artist Name]
[ti:Song Title]
[al:Album Name]
[by:Creator Name]
#another comment


[1:12.34]line of lyrics, 1x minute digit
[01:18.50]  line of lyrics prefixed with whitespace
[01:20.00]
[02:25.20]line of lyrics, preceded by retained empty line
[1001:25.20]line of lyrics, 4x minute digits

"
      .to_string(),
    };

    let plain_lyrics = Lyrics {
      lyrics_type: LyricsType::Plain,
      contents: r"line of lyrics, 1x minute digit
line of lyrics prefixed with whitespace

line of lyrics, preceded by retained empty line
line of lyrics, 4x minute digits
"
      .to_string(),
    };

    assert_eq!(sync_lyrics.into_plain().contents, plain_lyrics.contents);
  }
}
