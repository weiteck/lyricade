use crate::lyrics::{LyricsType, SYNC_LYRICS_REGEX};

/// Gaps will be normalised and pegged to the nearest multiple of of `1 / value`,
/// i.e. with a value of `4` the gap will be either `0.0`, `0.25`, `0.5`, `0.75`, or `1.0`.
const GAP_TO_PREV_GRANULARITY: f64 = 4.0;

#[derive(Debug, Clone)]
pub struct LyricsLine {
  pub lyrics_type: LyricsType,
  pub contents: String,
  pub timestamp: Option<String>,
  pub gap_to_prev: Option<f64>,
}

#[allow(clippy::cast_possible_truncation)]
impl LyricsLine {
  #[must_use]
  pub fn from_lyrics(lyrics: &str) -> Vec<LyricsLine> {
    let mut line_count = 0_usize;
    let mut prev_ts_secs = 0.0;
    let mut longest_gap_secs = 0.0;

    let timestamp_and_contents = lyrics
      .lines()
      .filter_map(|line| {
        SYNC_LYRICS_REGEX.find(line).map(|m| {
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

    if timestamp_and_contents.is_empty()
      || (timestamp_and_contents.len() as f64) < (0.5 * line_count as f64)
    {
      // Return plain lyrics
      lyrics
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| LyricsLine {
          lyrics_type: LyricsType::Plain,
          contents: line.to_string(),
          timestamp: None,
          gap_to_prev: None,
        })
        .collect()
    } else {
      // Return sync lyrics
      let mut prev_ts_secs = 0.0;

      timestamp_and_contents
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
            timestamp: Some(format!(
              "{}:{:02}",
              (ts_secs as usize / 60),
              ts_secs as usize % 60
            )),
            gap_to_prev: Some(gap_to_prev),
          }
        })
        .collect()
    }
  }
}
