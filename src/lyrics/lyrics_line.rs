use std::collections::BTreeSet;

use tracing::trace;

use crate::lyrics::{LRC_LYRICS_REGEX, LyricsType, lrc::LrcTag};

/// Gaps will be normalised and pegged to the nearest multiple of of `1 / value`,
/// i.e. with a value of `4` the gap will be either `0.0`, `0.25`, `0.5`, `0.75`, or `1.0`.
const GAP_TO_PREV_GRANULARITY: f64 = 4.0;

/// Represents a single line of lyrics. For sync lyrics, it contains the timestamp
/// and the normalised relative gap to the previous line of lyrics. A `gap_to_prev
/// == 0.5` would mean this gap is about 50% that of the longest gap in all the lyrics.
#[derive(Debug, Clone)]
pub(crate) struct LyricsLine {
  #[allow(unused)]
  pub(crate) lyrics_type: LyricsType,
  pub(crate) contents: String,
  pub(crate) timestamp: Option<String>,
  pub(crate) gap_to_prev: Option<f64>,
}

#[allow(clippy::cast_possible_truncation)]
impl LyricsLine {
  #[must_use]
  pub(crate) fn from_lyrics(lyrics: &str) -> (Vec<LyricsLine>, Option<BTreeSet<LrcTag>>) {
    let mut line_count = 0_usize;
    let mut prev_ts_secs = 0.0;
    let mut longest_gap_secs = 0.0;

    let mut tags: BTreeSet<LrcTag> = BTreeSet::new();

    let timestamp_and_contents = lyrics
      .lines()
      .filter_map(|line| {
        // Try to parse tags if sync lyrics not yet encountered
        if line_count == 0
          && let Ok(tag) = LrcTag::try_from(line)
        {
          trace!("Parsed LRC tag: \"{tag}\"");
          tags.insert(tag);
        }

        LRC_LYRICS_REGEX.find(line).map(|m| {
          let content = line[m.end()..].trim();

          if content.is_empty() {
            None
          } else {
            line_count += 1;

            let timestamp = m.as_str().trim_matches(['[', ']']);
            let ts_secs = timestamp
              .split([':', '.'])
              .filter_map(|s| s.parse::<f64>().ok())
              .enumerate()
              .fold(0.0, |acc, (idx, v)| match idx {
                0 => acc + (v * 60.0),  // mins
                1 => acc + v,           // secs
                2 => acc + (v / 100.0), // hundredths
                3.. => acc,             // unreachable if LRC is properly formatted
              });

            if prev_ts_secs != 0.0 {
              let gap_to_prev_secs = ts_secs - prev_ts_secs;
              if gap_to_prev_secs > longest_gap_secs {
                longest_gap_secs = gap_to_prev_secs;
              }
            }
            prev_ts_secs = ts_secs;

            Some((ts_secs, content))
          }
        })
      })
      .flatten()
      .filter(|(secs, content)| !content.is_empty() || *secs == 0.0)
      .collect::<Vec<(_, _)>>();

    // Some LRC files can have _only_ tags, e.g. instrumentals,
    // so check if we have more tags than lines before returning plain lyrics
    if tags.len() <= line_count
      && (timestamp_and_contents.is_empty() || timestamp_and_contents.len() < (line_count / 2))
    {
      // Return plain lyrics
      let lyrics_lines = lyrics
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| LyricsLine {
          lyrics_type: LyricsType::Plain,
          contents: line.to_string(),
          timestamp: None,
          gap_to_prev: None,
        })
        .collect();

      (lyrics_lines, None)
    } else {
      // Return sync lyrics
      let mut prev_ts_secs = 0.0;

      let lyrics_lines = timestamp_and_contents
        .into_iter()
        .map(|(ts_secs, contents)| {
          let gap_to_prev = if prev_ts_secs == 0.0 {
            0.0
          } else {
            ((((ts_secs - prev_ts_secs) / longest_gap_secs) * GAP_TO_PREV_GRANULARITY).floor())
              / GAP_TO_PREV_GRANULARITY
          };
          prev_ts_secs = ts_secs;

          let ts_secs = ts_secs.round();
          LyricsLine {
            lyrics_type: LyricsType::Sync,
            contents: contents.to_string(),
            timestamp: Some(format!("{}:{:02}", (ts_secs as usize / 60), ts_secs as usize % 60)),
            gap_to_prev: Some(gap_to_prev),
          }
        })
        .collect();

      let tags = if tags.is_empty() { None } else { Some(tags) };
      (lyrics_lines, tags)
    }
  }
}
