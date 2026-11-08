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
use tracing::{error, trace};

use crate::{
  Result,
  lyrics::lrc::{
    ENHANCED_LRC_LYRICS_REGEX, LRC_LYRICS_REGEX, LRC_LYRICS_STRIP_REGEX, LRC_TAG_REGEX,
  },
  track::Track,
  util::short_string,
};

pub mod lrc;
pub mod lyrics_line;
pub mod ttml;

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
      self.contents = convert_lrc_lyrics_to_plain(&self.contents);
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
      Self::Sync => "Sync",
      Self::Plain => "Plain",
    };
    write!(f, "{s}")
  }
}

impl From<LyricsFileType> for LyricsType {
  fn from(value: LyricsFileType) -> Self {
    match value {
      LyricsFileType::Lrc => Self::Sync,
      LyricsFileType::Txt => Self::Plain,
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
      LyricsType::Sync => Self::Lrc,
      LyricsType::Plain => Self::Txt,
    }
  }
}

impl TryFrom<&Utf8Path> for LyricsFileType {
  type Error = anyhow::Error;

  fn try_from(path: &Utf8Path) -> std::result::Result<Self, Self::Error> {
    match path.extension() {
      Some("lrc") => Ok(Self::Lrc),
      Some("txt") => Ok(Self::Txt),
      Some(ext) => Err(anyhow!(
        "\"{ext}\" is not a supported lyrics sidecar file extension (\"lrc\", \"txt\")"
      )),
      _ => Err(anyhow!("lyrics sidecar file must have an extension")),
    }
  }
}

impl LyricsFileType {
  #[must_use]
  pub fn file_extension(self) -> String {
    match self {
      Self::Lrc => "lrc".to_string(),
      Self::Txt => "txt".to_string(),
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
      let lyrics_type = if lyrics_are_lrc(&contents) {
        LyricsType::Sync
      } else {
        LyricsType::Plain
      };

      let file_type = LyricsFileType::try_from(path)?;

      return Ok(Self {
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
      .filter_map(|p| Self::try_from_path(&p).ok())
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
    let mut file = std::fs::File::create(&self.path).inspect_err(|error| error!("{error}"))?;
    file
      .write_all(self.lyrics.contents.as_bytes())
      .inspect_err(|error| error!("{error}"))?;
    Ok(())
  }
}

/// Check if lyrics are synchronised using regex.
pub fn lyrics_are_lrc(lyrics: &str) -> bool {
  LRC_LYRICS_REGEX.is_match(lyrics)
}

/// Check if lyrics contain word-level timestamps (enhanced LRC) using regex.
pub fn lyrics_are_enhanced_lrc(lyrics: &str) -> bool {
  ENHANCED_LRC_LYRICS_REGEX.is_match(lyrics)
}

/// Convert sync lyrics (LRC) to plain text.
#[must_use]
pub fn convert_lrc_lyrics_to_plain(lyrics: &str) -> String {
  // lrc.red can return plain lyrics with LRC-style tags, so we also handle these
  if lyrics_are_lrc(lyrics) || LRC_TAG_REGEX.is_match(lyrics) {
    trace!("Converting from LRC lyrics \"{} ...\" to plain lyrics", short_string(lyrics));
    lyrics
      .lines()
      .filter(|line| !line.is_empty())
      // Skip comments
      .filter(|line| !line.starts_with('#'))
      // Skip tags
      .filter(|line| !LRC_TAG_REGEX.is_match(line))
      // Remove timestamps; add new line
      .fold(String::with_capacity(lyrics.len()), |mut buffer, line| {
        let stripped = LRC_LYRICS_STRIP_REGEX.replace(line, "");
        let stripped = ENHANCED_LRC_LYRICS_REGEX.replace_all(&stripped, "");
        buffer.push_str(stripped.trim());
        buffer.push('\n');
        buffer
      })
  } else {
    trace!("Skipping conversion of plain lyrics \"{} ...\"", short_string(lyrics));
    lyrics.to_string()
  }
}

/// Convert enhanced LRC sync lyrics with word-level timestamps
/// to regular LRC sync lyrics with line-level timestamps.
#[must_use]
pub fn convert_enhanced_lrc_lyrics_to_lrc(lyrics: &str) -> String {
  if lyrics_are_lrc(lyrics) && lyrics_are_enhanced_lrc(lyrics) {
    lyrics
      .lines()
      .fold(String::with_capacity(lyrics.len()), |mut buffer, line| {
        let stripped = ENHANCED_LRC_LYRICS_REGEX.replace_all(line, "");
        buffer.push_str(stripped.trim());
        buffer.push('\n');
        buffer
      })
  } else {
    lyrics.to_string()
  }
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
  fn lrc_lyrics_to_plain() {
    let lrc_lyrics = Lyrics {
      lyrics_type: LyricsType::Sync,
      contents: r"
# comment preceded by empty line
[ar:Artist Name]
[ti:Song Title]
[al:Album Name]
[by:Creator Name]
#another comment


[1:12.343]line of lyrics, 1x minute digit
[01:18.5]  line of lyrics prefixed with whitespace
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

    assert_eq!(lrc_lyrics.into_plain().contents, plain_lyrics.contents);
  }

  #[test]
  fn enhanced_lrc_lyrics_to_plain() {
    let enhanced_lrc_lyrics = Lyrics {
      lyrics_type: LyricsType::Sync,
      contents: r"
# comment preceded by empty line
[ar:Artist Name]
[ti:Song Title]
[al:Album Name]
[by:Creator Name]
#another comment


[1:12.343]<1:12.343>line <01:13.11>of <01:14.21>lyrics, <01:15.31>1x <01:16.41>minute <01:17.51>digit
[01:18.5]  <01:18.5>line <01:18.60>of <01:18.65>lyrics <01:18.70>prefixed <01:18.75>with <01:18.80>whitespace
[01:20.00]
[02:25.20]<02:25.20>line <02:26.20>of <02:27.20>lyrics, <02:28.20>preceded <02:29.20>by <02:30.20>retained <02:31.20>empty <02:32.20>line
[1001:25.20]<1001:25.20>line <1001:26.20>of <1001:27.20>lyrics, <1001:28.20>4x <1001:29.20>minute <1001:30.20>digits

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

    assert_eq!(enhanced_lrc_lyrics.into_plain().contents, plain_lyrics.contents);
  }

  #[test]
  fn enhanced_lrc_lyrics_to_lrc() {
    let enhanced_lrc_lyrics = r"
# comment preceded by empty line
[ar:Artist Name]
[ti:Song Title]
[al:Album Name]
[by:Creator Name]
#another comment


[1:12.343]<1:12.343>line <01:13.11>of <01:14.21>lyrics, <01:15.31>1x <01:16.41>minute <01:17.51>digit
[01:18.5]  <01:18.5>line <01:18.60>of <01:18.65>lyrics <01:18.70>prefixed <01:18.75>with <01:18.80>whitespace
[01:20.00]
[02:25.20]<02:25.20>line <02:26.20>of <02:27.20>lyrics, <02:28.20>preceded <02:29.20>by <02:30.20>retained <02:31.20>empty <02:32.20>line
[1001:25.20]<1001:25.20>line <1001:26.20>of <1001:27.20>lyrics, <1001:28.20>4x <1001:29.20>minute <1001:30.20>digits
";

    let lrc_lyrics = r"
# comment preceded by empty line
[ar:Artist Name]
[ti:Song Title]
[al:Album Name]
[by:Creator Name]
#another comment


[1:12.343]line of lyrics, 1x minute digit
[01:18.5]  line of lyrics prefixed with whitespace
[01:20.00]
[02:25.20]line of lyrics, preceded by retained empty line
[1001:25.20]line of lyrics, 4x minute digits
";

    assert_eq!(convert_enhanced_lrc_lyrics_to_lrc(enhanced_lrc_lyrics), lrc_lyrics);
  }
}
