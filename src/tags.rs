use std::{borrow::Cow, fmt::Write};

use lofty::id3::v2::{
  BinaryFrame, Frame, FrameId, Id3v2Tag, Id3v2Version, SyncTextContentType, SynchronizedTextFrame,
  TimestampFormat, UnsynchronizedTextFrame,
};
use tracing::{debug, trace, warn};

use crate::lyrics::{Lyrics, LyricsType, lrc::LRC_LYRICS_REGEX};

const ID3V2_USLT_FRAME_ID: FrameId<'static> = FrameId::Valid(Cow::Borrowed("USLT"));
const ID3V2_SYLT_FRAME_ID: FrameId<'static> = FrameId::Valid(Cow::Borrowed("SYLT"));

#[allow(clippy::doc_markdown)]
/// Convert ID3v2 tag SYLT frame (synchronous text) to LRC-formatted lyrics.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn lrc_lyrics_from_id3v2(tag: &Id3v2Tag, mpeg_sample_rate: Option<u32>) -> Option<Lyrics> {
  if let Some(Frame::Binary(frame)) = tag.get(&ID3V2_SYLT_FRAME_ID)
    && let Ok(sylt) = SynchronizedTextFrame::parse(frame.as_bytes().as_slice(), frame.flags())
    && sylt.content_type == SyncTextContentType::Lyrics
  {
    let lines = match sylt.timestamp_format {
      TimestampFormat::MS => sylt.content,

      // This is only ever likely for MP3 files
      TimestampFormat::MPEG => {
        trace!("Converting MPEG frame indices to millisecond timestamps");

        if mpeg_sample_rate.is_none() {
          warn!(
            "No sample rate provided while decoding ID3v2 SYLT frame with MPEG frame indices timestamps (using default: 44,100)"
          );
        }
        let sample_rate = mpeg_sample_rate.unwrap_or(44_100);
        let samples_per_frame = 1_152; // assuming MPEG-1 Layer III standard

        sylt
          .content
          .into_iter()
          .map(|(frame_idx, line)| {
            let secs =
              (f64::from(frame_idx) * f64::from(samples_per_frame)) / f64::from(sample_rate);
            let ms = (secs * 1_000.0).round();
            (ms as u32, line)
          })
          .collect()
      }
    };

    let contents = lines.iter().fold(String::new(), |mut buf, (ms, line)| {
      let total_secs = ms / 1_000;
      let mins = total_secs / 60;
      let secs = total_secs % 60;
      let hundredths = (ms % 1_000) / 10;
      let _ = write!(buf, "[{mins:02}:{secs:02}.{hundredths:02}] {line}")
        .inspect_err(|error| warn!("{error}"));
      buf
    });

    if contents.is_empty() {
      warn!("Decoded SYLT frame from MP3 ID3v2 tag but failed to convert to LRC format lyrics");
    } else {
      debug!("Decoded SYLT frame from MP3 ID3v2 tag and converted to LRC format lyrics");
      return Some(Lyrics {
        lyrics_type: crate::lyrics::LyricsType::Sync,
        contents,
      });
    }
  }

  trace!("SYLT frame not found or unable to be decoded from MPEG file");
  None
}

#[allow(clippy::doc_markdown)]
/// Inserts sync or plain lyrics into the respective USLT or SYLT ID3v2 tag frames.
/// For sync lyrics, additionally inserts LRC-formatted lyrics into the USLT frame or
/// optionally converts to plain lyrics.
///
/// If `lyrics` contains an empty `String`, both USLT and SYLT frames are removed.
pub fn insert_lyrics_into_id3v2(
  lyrics: Lyrics,
  plain_lyrics_in_uslt_frame: bool,
  tag: &mut Id3v2Tag,
) -> bool {
  let _ = tag.remove(&ID3V2_USLT_FRAME_ID);
  let _ = tag.remove(&ID3V2_SYLT_FRAME_ID);

  trace!("Removed existing USLT and SYLT frames from ID3v2 tag");

  // Only remove lyrics frames if `lyrics` is empty
  if lyrics.contents.is_empty() {
    return true;
  }

  match lyrics.lyrics_type {
    LyricsType::Sync => {
      // Prepare expected SYLT format with ms timestamps
      let sync_lines = lyrics
        .contents
        .lines()
        .filter_map(|line| {
          LRC_LYRICS_REGEX.find(line).map(|m| {
            let lyric_line = line[m.end()..].trim();

            if lyric_line.is_empty() {
              None
            } else {
              let timestamp = m.as_str().trim_matches(['[', ']']);
              let ms = timestamp
                .split([':', '.'])
                .filter_map(|s| s.parse::<u32>().ok())
                .enumerate()
                .fold(0, |acc, (idx, v)| match idx {
                  0 => acc + (v * 60) * 1_000, // mins
                  1 => acc + v * 1_000,        // secs
                  2 => acc + (v * 10),         // hundredths
                  3.. => acc,                  // unreachable if LRC is properly formatted
                });

              Some((ms, format!("{}\n", lyric_line)))
            }
          })
        })
        .flatten()
        .collect::<Vec<(u32, String)>>();

      // Prepare sync SYLT frame
      let sylt_lyrics_frame = SynchronizedTextFrame::new(
        lofty::TextEncoding::UTF8,
        *b"XXX", // language - 'XXX' for unknown
        TimestampFormat::MS,
        SyncTextContentType::Lyrics,
        Some(String::from("Lyrics")), // description
        sync_lines,
      );

      // Keep V4 if already used
      let use_id3v2_v3 = match tag.original_version() {
        Id3v2Version::V2 | Id3v2Version::V3 => true,
        Id3v2Version::V4 => false,
      };
      let write_opts = lofty::config::WriteOptions::new()
        .lossy_text_encoding(true)
        .use_id3v23(use_id3v2_v3);
      if let Ok(bytes) = sylt_lyrics_frame.as_bytes(write_opts) {
        // Also insert into the unsync USLT frame as fallback
        let uslt_lyrics = if plain_lyrics_in_uslt_frame {
          trace!("Using converted plain lyrics for fallback USLT frame");
          lyrics.into_plain().contents
        } else {
          lyrics.contents
        };

        let uslt_frame = UnsynchronizedTextFrame::new(
          lofty::TextEncoding::UTF8,
          *b"XXX",                // language - 'XXX' for unknown
          Cow::from("Lyrics"),    // description
          Cow::from(uslt_lyrics), // lyrics
        );

        tag.insert(uslt_frame.into());
        trace!("Inserted fallback USLT frame into ID3v2 tag before inserting SYLT frame");

        // Insert into sync SYLT frame
        let sylt_frame = BinaryFrame::new(ID3V2_SYLT_FRAME_ID, bytes);
        tag.insert(sylt_frame.into());

        debug!("Inserted SYLT frame with sync lyrics into ID3v2 tag");
        true
      } else {
        warn!("Failed to insert SYLT frame with sync lyrics into ID3v2 tag");
        false
      }
    }

    LyricsType::Plain => {
      let lyrics_frame = UnsynchronizedTextFrame::new(
        lofty::TextEncoding::UTF8,
        *b"XXX",                    // language - 'XXX' for unknown
        Cow::from("Lyrics"),        // description
        Cow::from(lyrics.contents), // lyrics
      );

      tag.insert(lyrics_frame.into());

      debug!("Inserted USLT frame with plain lyrics into ID3v2 tag");
      true
    }
  }
}
