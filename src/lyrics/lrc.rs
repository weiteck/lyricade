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
        _ => Err(anyhow!(
          "Parse LRC tag: Not a known LRC tag prefix: \"{prefix}\""
        )),
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
      LrcTag::ArtistName(_) => "Artist",
      LrcTag::SongTitle(_) => "Track",
      LrcTag::AlbumName(_) => "Album",
      LrcTag::Author(_) => "Author",
      LrcTag::Lyricist(_) => "Lyricist",
      LrcTag::Length(_) => "Length",
      LrcTag::Creator(_) => "Creator",
      LrcTag::Offset(_) => "Offset",
      LrcTag::Editor(_) => "Editor",
      LrcTag::Version(_) => "Version",
    }
    .to_string()
  }

  #[must_use]
  pub fn value(&self) -> String {
    match self {
      LrcTag::ArtistName(value)
      | LrcTag::SongTitle(value)
      | LrcTag::AlbumName(value)
      | LrcTag::Author(value)
      | LrcTag::Lyricist(value)
      | LrcTag::Length(value)
      | LrcTag::Creator(value)
      | LrcTag::Editor(value)
      | LrcTag::Version(value) => value.clone(),
      LrcTag::Offset(value) => format!("{} ms", value),
    }
  }
}
