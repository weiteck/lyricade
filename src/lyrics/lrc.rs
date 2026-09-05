use std::{fmt::Display, sync::LazyLock};

use anyhow::anyhow;
use regex::Regex;

/// Regex to match "\[00:00.000]\" or "\[0:00.0]\", indicating synchronised lyrics.
pub static LRC_LYRICS_REGEX: LazyLock<Regex> = LazyLock::new(|| {
  regex::Regex::new(r"\[(\d+):(\d{2})(?:\.(\d{1,3}))?\]").expect("should be valid regex")
});

/// Regex to match "\[00:00.000]\" or "\[0:00.0]\" followed by 0 or more whitespace chars ("[ \t]*").
pub static LRC_LYRICS_STRIP_REGEX: LazyLock<Regex> = LazyLock::new(|| {
  regex::Regex::new(r"\[(\d+):(\d{2})(?:\.(\d{1,3}))?\][ \t]*").expect("should be valid regex")
});

/// Regex to match "\[xx:xxx...xxx]\".
pub static LRC_TAG_REGEX: LazyLock<Regex> = LazyLock::new(|| {
  regex::Regex::new(r"\[([a-zA-Z]+):(.+)\][ \t]*$").expect("should be valid regex")
});

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LrcTag {
  /// `[ar:Artist Name]` - Song artist
  ArtistName(String),
  /// `[ti:Song Title]` - Song title
  SongTitle(String),
  /// `[al:Album Name]` - Album name
  AlbumName(String),
  /// `[au:Author]` - Song author/composer
  Author(String),
  /// `[lr:Lyricist]` - Lyricist
  Lyricist(String),
  /// `[length:Lyricist]` - Length of the song (mm:ss)
  Length(String),
  /// `[by:Creator]` - LRC file creator
  Creator(String),
  /// `[offset:+/-ms]` - Time offset in milliseconds
  Offset(i32),
  /// `[re:Editor]` - LRC editor software
  Editor(String),
  /// `[ve:Version]` - LRC format version
  Version(String),
}

impl TryFrom<&str> for LrcTag {
  type Error = anyhow::Error;

  fn try_from(value: &str) -> Result<Self, Self::Error> {
    if let Some((prefix, suffix)) = value
      .trim()
      .trim_start_matches('[')
      .trim_end_matches(']')
      .split_once(':')
    {
      let prefix = prefix.trim().to_lowercase();
      let suffix = suffix.trim();

      if suffix.is_empty() {
        return Err(anyhow!("Parse LRC tag: \"{prefix}\" value is empty"));
      }

      return match prefix.as_str() {
        "ar" => Ok(Self::ArtistName(suffix.to_string())),
        "ti" => Ok(Self::SongTitle(suffix.to_string())),
        "al" => Ok(Self::AlbumName(suffix.to_string())),
        "au" => Ok(Self::Author(suffix.to_string())),
        "lr" => Ok(Self::Lyricist(suffix.to_string())),
        "length" => Ok(Self::Length(suffix.to_string())),
        "by" => Ok(Self::Creator(suffix.to_string())),
        "offset" => {
          let offset = suffix.parse::<i32>().map_err(|_| {
            anyhow!("Parse LRC tag: Could not parse number from LRC offset value: \"{suffix}\"")
          })?;
          Ok(Self::Offset(offset))
        }
        "re" | "tool" => Ok(Self::Editor(suffix.to_string())),
        "ve" => Ok(Self::Version(suffix.to_string())),
        _ => Err(anyhow!("Parse LRC tag: Not a known LRC tag prefix: \"{prefix}\"")),
      };
    }

    Err(anyhow!("Parse LRC tag: Not an LRC tag"))
  }
}

impl Display for LrcTag {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}: {}", self.tag(), self.value())
  }
}

impl LrcTag {
  #[must_use]
  pub fn tag(&self) -> String {
    match self {
      Self::ArtistName(_) => "Artist",
      Self::SongTitle(_) => "Track",
      Self::AlbumName(_) => "Album",
      Self::Author(_) => "Author",
      Self::Lyricist(_) => "Lyricist",
      Self::Length(_) => "Length",
      Self::Creator(_) => "Creator",
      Self::Offset(_) => "Offset",
      Self::Editor(_) => "Editor",
      Self::Version(_) => "Version",
    }
    .to_string()
  }

  #[must_use]
  pub fn value(&self) -> String {
    match self {
      Self::ArtistName(value)
      | Self::SongTitle(value)
      | Self::AlbumName(value)
      | Self::Author(value)
      | Self::Lyricist(value)
      | Self::Length(value)
      | Self::Creator(value)
      | Self::Editor(value)
      | Self::Version(value) => value.clone(),
      Self::Offset(value) => {
        format!("{}{} ms", if value.is_positive() { "+" } else { "" }, value)
      }
    }
  }
}
