use std::collections::HashSet;

use tracing::trace;

use crate::track::Track;

#[derive(Debug, Clone, Default)]
pub(super) struct TrackStats {
  instrumental_set: HashSet<i32>,
  not_instrumental_set: HashSet<i32>,
  never_checked_set: HashSet<i32>,
  sync_lyrics_set: HashSet<i32>,
  plain_lyrics_set: HashSet<i32>,
  tagged_lyrics_set: HashSet<i32>,
  sidecar_file_set: HashSet<i32>,

  pub(super) count: usize,
  pub(super) instrumental: usize,
  pub(super) not_instrumental: usize,
  pub(super) never_checked: usize,
  pub(super) sync_lyrics: usize,
  pub(super) plain_lyrics: usize,
  pub(super) tagged_lyrics: usize,
  pub(super) sidecar_file: usize,
}

impl TrackStats {
  pub(super) fn update(&mut self, tracks: &[Track]) {
    trace!("Building TrackStats");

    *self = Self::default();

    self.count = tracks.len();

    for track in tracks {
      if track.last_api_check_at.is_none() {
        self.never_checked_set.insert(track.id);
      }

      if track.instrumental.unwrap_or(false) {
        self.instrumental_set.insert(track.id);
      } else {
        self.not_instrumental_set.insert(track.id);

        if track.lyrics_synchronised || track.lyrics_sidecar_lrc_file.is_some() {
          self.sync_lyrics_set.insert(track.id);
        }

        if !track.lyrics_synchronised
          && (track.lyrics.is_some() || track.lyrics_sidecar_txt_file.is_some())
        {
          self.plain_lyrics_set.insert(track.id);
        }

        if track.lyrics.is_some() {
          self.tagged_lyrics_set.insert(track.id);
        }

        if track.lyrics_sidecar_lrc_file.is_some() || track.lyrics_sidecar_txt_file.is_some() {
          self.sidecar_file_set.insert(track.id);
        }
      }
    }

    self.instrumental = self.instrumental_set.len();
    self.not_instrumental = self.not_instrumental_set.len();
    self.never_checked = self.never_checked_set.len();
    self.sync_lyrics = self.sync_lyrics_set.len();
    self.plain_lyrics = self.plain_lyrics_set.len();
    self.tagged_lyrics = self.tagged_lyrics_set.len();
    self.sidecar_file = self.sidecar_file_set.len();
  }

  pub(super) fn refresh_from_filtered(&mut self, track_ids: &HashSet<i32>) {
    self.instrumental = self.instrumental_set.intersection(track_ids).count();
    self.not_instrumental = self.not_instrumental_set.intersection(track_ids).count();
    self.never_checked = self.never_checked_set.intersection(track_ids).count();
    self.sync_lyrics = self.sync_lyrics_set.intersection(track_ids).count();
    self.plain_lyrics = self.plain_lyrics_set.intersection(track_ids).count();
    self.tagged_lyrics = self.tagged_lyrics_set.intersection(track_ids).count();
    self.sidecar_file = self.sidecar_file_set.intersection(track_ids).count();
  }

  pub(super) fn not_instrumental_percent(&self) -> f64 {
    (self.not_instrumental as f64 / self.count as f64) * 100.0
  }

  pub(super) fn sync_lyrics_percent(&self) -> f64 {
    (self.sync_lyrics as f64 / self.not_instrumental as f64) * 100.0
  }

  pub(super) fn plain_lyrics_percent(&self) -> f64 {
    (self.plain_lyrics as f64 / self.not_instrumental as f64) * 100.0
  }

  pub(super) fn tagged_lyrics_percent(&self) -> f64 {
    (self.tagged_lyrics as f64 / self.not_instrumental as f64) * 100.0
  }

  pub(super) fn sidecar_file_percent(&self) -> f64 {
    (self.sidecar_file as f64 / self.not_instrumental as f64) * 100.0
  }
}
