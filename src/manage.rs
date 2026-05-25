use std::{fs, time::Duration};

use anyhow::anyhow;
use relm4::tokio::sync::oneshot;
use tracing::{debug, error, info, trace};

use crate::{
  DB_POOL, Result,
  lyrics::{Lyrics, LyricsType, convert_sync_lyrics_to_plain},
  track::Track,
  util::reporter::IntervalReporter,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManageLyricsOptions {
  pub tags: TagOptions,
  pub sidecars: SidecarOptions,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagOptions {
  pub delete: ManageLyricsTarget,
  pub copy: ManageLyricsTarget,
  pub convert_to_plain: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SidecarOptions {
  pub delete: ManageLyricsTarget,
  pub copy: ManageLyricsTarget,
  pub convert_to_plain: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ManageLyricsTarget {
  #[default]
  None = 0,
  Plain = 1,
  Sync = 2,
  All = 3,
}

impl From<u32> for ManageLyricsTarget {
  fn from(value: u32) -> Self {
    match value {
      0 | 4.. => Self::None,
      1 => Self::Plain,
      2 => Self::Sync,
      3 => Self::All,
    }
  }
}

/// Whether the `Track` needs to be updated in the database or, optionally,
/// written to disk (i.e. the lyrics tag was changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ManageLyricsResult {
  NoAction = 0,
  WriteToDb = 1,
  WriteToFileAndDb = 2,
}

impl ManageLyricsOptions {
  pub fn apply<F>(
    &self,
    tracks: Vec<Track>,
    on_progress: F,
    cancel_on_close: &mut oneshot::Receiver<()>,
  ) -> Result<()>
  where
    F: Fn(String) + Send + 'static,
  {
    info!("ManageLyrics: Applying changes to {} tracks", tracks.len());

    let mut reporter = IntervalReporter::builder()
      .id("ManageLyrics")
      .target(tracks.len())
      .report_interval(Duration::from_secs(2))
      .callback(|stats| {
        on_progress(format!(
          "Applying Manage Lyrics changes… {:.0} %\n(about {} remaining)",
          stats.percent_processed, stats.human_time_remaining
        ));
      })
      .build();

    let mut conn = DB_POOL.get()?;

    // Not done inside a transaction to keep database rows consistent with file changes
    for mut track in tracks {
      // Cancelled?
      if cancel_on_close
        .try_recv()
        .is_err_and(|error| error == oneshot::error::TryRecvError::Closed)
      {
        return Err(
          anyhow!(diesel::result::Error::RollbackTransaction)
            .context("User cancelled the ManageLyrics operation"),
        );
      }

      match [
        self.delete_lyrics_tag(&mut track),
        self.copy_from_sidecar_to_lyrics_tag(&mut track),
        self.convert_sync_lyrics_tag_to_plain(&mut track),
        self.delete_sidecar_files(&mut track),
        self.copy_from_lyrics_tag_to_sidecar(&mut track),
        self.convert_sync_sidecar_to_plain(&mut track),
      ]
      .iter()
      .max()
      .expect("not empty")
      {
        ManageLyricsResult::NoAction => trace!("ManageLyrics: {track} unchanged"),
        ManageLyricsResult::WriteToDb => {
          debug!("ManageLyrics: {track} changed: Writing to database");

          track.write_to_db().conn(&mut conn).call()?;
        }
        ManageLyricsResult::WriteToFileAndDb => {
          debug!("ManageLyrics: {track} lyrics tag changed: Writing to file and database");

          track.write_to_file_and_db().conn(&mut conn).call()?;
        }
      }

      reporter.tick();
    }

    Ok(())
  }

  /// Delete the 'lyrics' tag.
  fn delete_lyrics_tag(self, track: &mut Track) -> ManageLyricsResult {
    match self.tags.delete {
      ManageLyricsTarget::Plain if track.lyrics.is_some() && !track.lyrics_synchronised => {
        track.lyrics = None;
        ManageLyricsResult::WriteToFileAndDb
      }

      ManageLyricsTarget::Sync if track.lyrics.is_some() && track.lyrics_synchronised => {
        track.lyrics = None;
        track.lyrics_synchronised = false;
        ManageLyricsResult::WriteToFileAndDb
      }

      ManageLyricsTarget::All if track.lyrics.is_some() => {
        track.lyrics = None;
        track.lyrics_synchronised = false;
        ManageLyricsResult::WriteToFileAndDb
      }

      _ => ManageLyricsResult::NoAction,
    }
  }

  /// Delete sidecar lyrics files.
  /// Deletes all sidecar files is no `target` provided.
  fn delete_sidecar_files(self, track: &mut Track) -> ManageLyricsResult {
    match self.sidecars.delete {
      ManageLyricsTarget::Plain if let Some(path) = track.txt_file_path() => {
        debug!("{}: Deleting sidecar file: \"{}\"", track, &path);

        if fs::remove_file(&path)
          .inspect_err(|error| error!("{error}"))
          .is_ok()
        {
          track.lyrics_sidecar_txt_file = None;
        }

        ManageLyricsResult::WriteToDb
      }

      ManageLyricsTarget::Sync if let Some(path) = track.lrc_file_path() => {
        debug!("{}: Deleting sidecar file: \"{}\"", track, &path);

        if fs::remove_file(&path)
          .inspect_err(|error| error!("{error}"))
          .is_ok()
        {
          track.lyrics_sidecar_lrc_file = None;
        }

        ManageLyricsResult::WriteToDb
      }

      ManageLyricsTarget::All => {
        let result = if let Some(path) = track.lrc_file_path() {
          debug!("{}: Deleting sidecar file: \"{}\"", track, &path);

          if fs::remove_file(&path)
            .inspect_err(|error| error!("{error}"))
            .is_ok()
          {
            track.lyrics_sidecar_lrc_file = None;
          }

          ManageLyricsResult::WriteToDb
        } else {
          ManageLyricsResult::NoAction
        };

        if let Some(path) = track.txt_file_path() {
          debug!("{}: Deleting sidecar file: \"{}\"", track, &path);

          if fs::remove_file(&path)
            .inspect_err(|error| error!("{error}"))
            .is_ok()
          {
            track.lyrics_sidecar_txt_file = None;
          }

          ManageLyricsResult::WriteToDb
        } else {
          ManageLyricsResult::NoAction
        }
        .max(result) // return most actionable result
      }

      _ => ManageLyricsResult::NoAction,
    }
  }

  /// Copy from sidecar lyrics file to the 'lyrics' tag.
  /// Will copy from any sidecar file if no `source` provided (sync preferred).
  fn copy_from_sidecar_to_lyrics_tag(self, track: &mut Track) -> ManageLyricsResult {
    let mut process = |source: ManageLyricsTarget| match source {
      ManageLyricsTarget::Plain
        if let Some(sidecar_lyrics) = track.lyrics_sidecar_txt_file.as_ref()
          && track.lyrics.as_ref().is_none_or(|l| l != sidecar_lyrics) =>
      {
        debug!("{}: Copying to lyrics tag from TXT sidecar file", track);

        track.lyrics = Some(sidecar_lyrics.clone());
        track.lyrics_synchronised = false;

        ManageLyricsResult::WriteToFileAndDb
      }

      // Fallback to converting sync to plain lyrics
      ManageLyricsTarget::Plain
        if let Some(sidecar_lyrics) = track.lyrics_sidecar_lrc_file.as_ref()
          && let converted_lyrics = convert_sync_lyrics_to_plain(sidecar_lyrics)
          && track.lyrics.as_ref().is_none_or(|l| l != &converted_lyrics) =>
      {
        debug!("{}: Copying to lyrics tag from LRC sidecar file (converted to plain)", track);

        track.lyrics = Some(converted_lyrics);
        track.lyrics_synchronised = false;

        ManageLyricsResult::WriteToFileAndDb
      }

      ManageLyricsTarget::Sync
        if let Some(sidecar_lyrics) = track.lyrics_sidecar_lrc_file.as_ref()
          && track.lyrics.as_ref().is_none_or(|l| l != sidecar_lyrics) =>
      {
        debug!("{}: Copying to lyrics tag from LRC sidecar file", track);

        track.lyrics = Some(sidecar_lyrics.clone());
        track.lyrics_synchronised = true;

        ManageLyricsResult::WriteToFileAndDb
      }

      ManageLyricsTarget::All => unreachable!(),

      _ => ManageLyricsResult::NoAction,
    };

    // Copy sync sidecar file if `All` provided
    if self.tags.copy == ManageLyricsTarget::All {
      let source = ManageLyricsTarget::Sync;
      if process(source) == ManageLyricsResult::WriteToFileAndDb {
        ManageLyricsResult::WriteToFileAndDb
      } else {
        // Fallback to plain
        let source = ManageLyricsTarget::Plain;
        process(source)
      }
    } else {
      process(self.tags.copy)
    }
  }

  /// Copy from lyrics tag to sidecar lyrics file.
  /// Will copy any lyrics type if no `source` provided (sync preferred).
  fn copy_from_lyrics_tag_to_sidecar(self, track: &mut Track) -> ManageLyricsResult {
    // TODO: This should be done as an atomic operation outside of a transaction so the db stays in sync if cancelled

    match self.sidecars.copy {
      ManageLyricsTarget::Sync | ManageLyricsTarget::All
        if track.lyrics_synchronised
          && let Some(lyrics) = track.lyrics.as_ref() =>
      {
        let lyrics = Lyrics {
          lyrics_type: LyricsType::Sync,
          contents: lyrics.clone(),
        };
        if track.save_sidecar_file(&lyrics).is_ok() {
          track.lyrics_sidecar_lrc_file = track.lyrics.clone();
          ManageLyricsResult::WriteToDb
        } else {
          ManageLyricsResult::NoAction
        }
      }

      ManageLyricsTarget::Plain | ManageLyricsTarget::All
        if !track.lyrics_synchronised
          && let Some(lyrics) = track.lyrics.as_ref() =>
      {
        let lyrics = Lyrics {
          lyrics_type: LyricsType::Plain,
          contents: lyrics.clone(),
        };
        if track.save_sidecar_file(&lyrics).is_ok() {
          track.lyrics_sidecar_txt_file = track.lyrics.clone();
          ManageLyricsResult::WriteToDb
        } else {
          ManageLyricsResult::NoAction
        }
      }

      _ => ManageLyricsResult::NoAction,
    }
  }

  /// If lyrics tag is sync type (LRC), convert to plain text.
  fn convert_sync_lyrics_tag_to_plain(self, track: &mut Track) -> ManageLyricsResult {
    if self.tags.convert_to_plain && track.lyrics.is_some() && track.lyrics_synchronised {
      track.lyrics = track
        .lyrics
        .as_deref_mut()
        .map(|l| convert_sync_lyrics_to_plain(l));
      track.lyrics_synchronised = false;
      ManageLyricsResult::WriteToFileAndDb
    } else {
      ManageLyricsResult::NoAction
    }
  }

  /// If sidecar lyrics file is sync type (LRC), convert to plain text.
  /// This will change the `.lrc` extension to `.txt`, overwriting any existing file with that name.
  fn convert_sync_sidecar_to_plain(self, track: &mut Track) -> ManageLyricsResult {
    // TODO: This should be done as an atomic operation outside of a transaction so the db stays in sync if cancelled

    if self.sidecars.convert_to_plain
      && let Some(lyrics) = track.lyrics_sidecar_lrc_file.as_ref()
    {
      let contents = convert_sync_lyrics_to_plain(lyrics);
      let lyrics = Lyrics {
        lyrics_type: LyricsType::Plain,
        contents,
      };

      // Write out TXT file
      let result = if track.save_sidecar_file(&lyrics).is_ok() {
        track.lyrics_sidecar_txt_file = Some(lyrics.contents);

        ManageLyricsResult::WriteToDb
      } else {
        ManageLyricsResult::NoAction
      };

      // Delete LRC file
      if let Some(path) = track.lrc_file_path()
        && fs::remove_file(&path)
          .inspect_err(|error| error!("{error}"))
          .is_ok()
      {
        track.lyrics_sidecar_lrc_file = None;

        ManageLyricsResult::WriteToDb
      } else {
        ManageLyricsResult::NoAction
      }
      .max(result) // return most actionable result
    } else {
      ManageLyricsResult::NoAction
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn manage_lyrics_result_ordering() {
    let results = [
      ManageLyricsResult::WriteToDb,
      ManageLyricsResult::WriteToFileAndDb,
      ManageLyricsResult::NoAction,
    ];

    assert_eq!(&ManageLyricsResult::WriteToFileAndDb, results.iter().max().unwrap());
    assert_eq!(&ManageLyricsResult::NoAction, results.iter().min().unwrap());
  }
}
