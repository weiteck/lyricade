use std::sync::LazyLock;

use bon::builder;
use camino::Utf8Path;
use chrono::{DateTime, Local, NaiveDateTime, Utc};
use relm4::gtk;
use tracing::{error, trace};

pub(crate) mod reporter;

pub(crate) static UNIX_EPOCH_NDT: LazyLock<NaiveDateTime> = LazyLock::new(|| {
  chrono::DateTime::from_timestamp_secs(0)
    .expect("valid timestamp")
    .naive_utc()
});

/// Get current UTC timestamp as `NaiveDateTime`.
#[must_use]
pub(crate) fn now() -> chrono::NaiveDateTime {
  chrono::Utc::now().naive_utc()
}

/// Convert UTC `NaiveDateTime` to local time.
#[must_use]
pub(crate) fn ndt_utc_to_local_dt(ndt_utc: NaiveDateTime) -> DateTime<Local> {
  let utc_dt: DateTime<Utc> = DateTime::from_naive_utc_and_offset(ndt_utc, Utc);
  let dt_local: DateTime<Local> = DateTime::from(utc_dt);
  dt_local
}

/// Convert UTC `NaiveDateTime` to local timezone formatted according to the locale.
#[must_use]
pub(crate) fn ndt_utc_to_ui_string(ndt_utc: NaiveDateTime) -> String {
  let ts = ndt_utc.and_utc().timestamp();

  gtk::glib::DateTime::from_unix_utc(ts)
    .expect("should be a valid Unix timestamp")
    .to_local()
    .expect("should be able to convert UTC glib datetime to local datetime")
    .format("%x, %X")
    .expect("should be a valid `strftime` format specifier")
    .to_string()
}

/// Convert UTC `NaiveDateTime` to humanised text if recent, e.g. "2 months ago",
/// and local ISO 8601 text with second accuracy if not recent.
#[must_use]
pub(crate) fn ndt_utc_to_humanised_string(ndt_utc: NaiveDateTime) -> String {
  let local_dt = ndt_utc_to_local_dt(ndt_utc);

  if local_dt.years_since(Local::now()).is_some() {
    ndt_utc_to_ui_string(ndt_utc)
  } else {
    // Humanise recent dates
    let ht = chrono_humanize::HumanTime::from(local_dt);
    let s = ht.to_text_en(chrono_humanize::Accuracy::Rough, chrono_humanize::Tense::Past);

    // Clean up `chrono-humanize` output
    // TODO: Consider a different crate
    if &s == "now ago" {
      "Just now".into()
    } else {
      s
    }
  }
}

/// Get file modification timestamp as UTC `NaiveDateTime`. Falls back to Unix epoch on any error.
/// Optionally takes a reference to an existing `File` handle.
#[must_use]
#[builder]
pub(crate) fn file_modified_at(path: &Utf8Path, file: Option<&std::fs::File>) -> NaiveDateTime {
  trace!("Getting modified timestamp for file \"{}\"", path);

  let metadata = if let Some(file) = file {
    file.metadata()
  } else {
    std::fs::File::open(path).and_then(|f| f.metadata())
  }
  .inspect_err(|error| {
    error!("Error while getting modified timestamp for file \"{}\": {error}", path);
  });

  metadata
    .and_then(|m| m.modified())
    .map(chrono::DateTime::<chrono::Utc>::from)
    .map_or(*UNIX_EPOCH_NDT, |dt| dt.naive_utc())
}

/// Scale a value to fit the provided `min` and `max` range.
/// Values over 1.0 will be clamped to the `max` value.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
#[builder]
pub(crate) fn scale(value: f64, min: i32, max: i32) -> i32 {
  (min + (value.abs() * f64::from(max)) as i32).min(max)
}

#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub(crate) fn secs_f64_to_hms(secs: f64) -> String {
  let secs = secs.round() as i64;

  let td = chrono::TimeDelta::try_seconds(secs).unwrap_or_default();

  let hours = td.num_hours();
  let mins = td.num_minutes() % 60;
  let secs = td.num_seconds() % 60;

  format!(
    "{}{mins:02}:{secs:02}",
    if hours > 0 {
      format!("{hours:02}:")
    } else {
      String::new()
    }
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn secs_f64_to_hms_output() {
    assert_eq!(secs_f64_to_hms(32.6), String::from("00:33"));
    assert_eq!(secs_f64_to_hms(92.4), String::from("01:32"));
    assert_eq!(secs_f64_to_hms(7292.4), String::from("02:01:32"));
  }
}
