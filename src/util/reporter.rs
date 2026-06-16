use std::{
  fmt::Display,
  time::{Duration, Instant},
};

use bon::bon;
use tracing::trace;

#[derive(Debug, Clone)]
pub(crate) struct IntervalReporter<F>
where
  F: Fn(&IntervalReporterStats),
{
  id: String,
  report_interval: Duration,
  report_threshold: Option<usize>,
  callback: Option<F>,

  batch_start: Instant,
  batch_processed: usize,

  stats: IntervalReporterStats,
}

#[bon]
impl<F> IntervalReporter<F>
where
  F: Fn(&IntervalReporterStats),
{
  #[builder]
  pub(crate) fn builder(
    id: Option<&str>,
    report_interval: Duration,
    report_threshold: Option<usize>,
    target: usize,
    callback: Option<F>,
  ) -> Self {
    Self {
      id: id.unwrap_or("Unknown").to_string(),
      report_interval,
      report_threshold,
      callback,
      batch_start: Instant::now(),
      batch_processed: 0,
      stats: IntervalReporterStats::new(target),
    }
  }

  pub(crate) fn tick(&mut self) -> bool {
    // Update tally each tick
    self.batch_processed += 1;
    self.stats.processed += 1;

    if self.batch_start.elapsed() > self.report_interval
      && self
        .report_threshold
        .is_none_or(|rt| self.batch_processed > rt)
    {
      trace!("{self}: Firing");

      // Calculate stats and reset interval
      self.update_and_reset();

      // Run callback
      if let Some(callback) = &self.callback {
        callback(&self.stats);
      }

      true
    } else {
      false
    }
  }

  fn update_and_reset(&mut self) {
    // Recalculate stats
    self.stats.process_rate_per_sec = self.process_rate_per_sec();
    self.stats.percent_processed = self.percent_processed();
    self.stats.time_remaining = self.time_remaining();
    self.stats.human_time_remaining = self.human_time_remaining();

    // Reset batch stats to start next interval
    self.reset_batch();
  }

  fn reset_batch(&mut self) {
    self.batch_start = Instant::now();
    self.batch_processed = 0;
  }

  #[must_use]
  pub(crate) fn human_time_remaining(&self) -> String {
    let accuracy = if self.stats.time_remaining.num_minutes() > 0 {
      chrono_humanize::Accuracy::Rough
    } else {
      chrono_humanize::Accuracy::Precise
    };
    chrono_humanize::HumanTime::from(self.stats.time_remaining)
      .to_text_en(accuracy, chrono_humanize::Tense::Present)
  }

  #[allow(clippy::cast_possible_truncation)]
  #[must_use]
  pub(crate) fn time_remaining(&self) -> chrono::TimeDelta {
    // Fallback to using items processed since start if processed in batch is too small
    let secs_remaining = if self.batch_processed > 0 {
      ((self.stats.target - self.stats.processed) as f64 / self.stats.process_rate_per_sec) as i64
    } else {
      ((self.stats.target - self.stats.processed) as f64 / self.process_rate_per_sec_overall())
        as i64
    };
    chrono::Duration::seconds(secs_remaining)
  }

  #[must_use]
  pub(crate) fn process_rate_per_sec(&self) -> f64 {
    self.batch_processed as f64 / self.batch_start.elapsed().as_secs() as f64
  }

  #[must_use]
  pub(crate) fn process_rate_per_sec_overall(&self) -> f64 {
    self.stats.processed as f64 / self.stats.start.elapsed().as_secs() as f64
  }

  #[must_use]
  pub(crate) fn percent_processed(&self) -> f64 {
    (self.stats.processed.min(self.stats.target) as f64 / self.stats.target as f64) * 100.0
  }
}

impl<F> Display for IntervalReporter<F>
where
  F: Fn(&IntervalReporterStats),
{
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "IntervalReporter({})", self.id)
  }
}

#[derive(Debug, Clone)]
pub(crate) struct IntervalReporterStats {
  pub(crate) start: Instant,
  pub(crate) processed: usize,
  pub(crate) target: usize,

  pub(crate) human_time_remaining: String,
  pub(crate) time_remaining: chrono::TimeDelta,
  pub(crate) process_rate_per_sec: f64,
  pub(crate) percent_processed: f64,
}

impl IntervalReporterStats {
  #[must_use]
  pub(crate) fn new(target: usize) -> Self {
    Self {
      target,
      start: Instant::now(),
      processed: 0,
      human_time_remaining: String::default(),
      time_remaining: chrono::TimeDelta::default(),
      process_rate_per_sec: 0.,
      percent_processed: 0.,
    }
  }
}
