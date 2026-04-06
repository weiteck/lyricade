use std::sync::LazyLock;

use bon::builder;
use camino::Utf8Path;
use chrono::{DateTime, Local, NaiveDateTime, Utc};
use tracing::{error, trace};

pub static UNIX_EPOCH_NDT: LazyLock<NaiveDateTime> = LazyLock::new(|| {
  chrono::DateTime::from_timestamp_secs(0)
    .expect("valid timestamp")
    .naive_utc()
});

/// Get current UTC timestamp as `NaiveDateTime`.
pub fn now() -> chrono::NaiveDateTime {
  chrono::Utc::now().naive_utc()
}

/// Convert UTC `NaiveDateTime` to local time.
pub fn ndt_utc_to_local_dt(ndt_utc: NaiveDateTime) -> DateTime<Local> {
  let utc_dt: DateTime<Utc> = DateTime::from_naive_utc_and_offset(ndt_utc, Utc);
  let dt_local: DateTime<Local> = DateTime::from(utc_dt);
  dt_local
}

/// Convert UTC `NaiveDateTime` to local ISO 8601 text with second accuracy.
pub fn ndt_utc_to_ui_string(ndt_utc: NaiveDateTime) -> String {
  let local_dt = ndt_utc_to_local_dt(ndt_utc);
  local_dt.format("%F %T").to_string()
}

/// Convert UTC `NaiveDateTime` to humanised text if recent, e.g. "2 months ago",
/// and local ISO 8601 text with second accuracy if not recent.
pub fn ndt_utc_to_humanised_string(ndt_utc: NaiveDateTime) -> String {
  let local_dt = ndt_utc_to_local_dt(ndt_utc);

  if local_dt.years_since(Local::now()).is_some() {
    local_dt.format("%F %T").to_string()
  } else {
    // Humanise recent dates
    let ht = chrono_humanize::HumanTime::from(local_dt);
    ht.to_text_en(
      chrono_humanize::Accuracy::Rough,
      chrono_humanize::Tense::Past,
    )
  }
}

/// Get file modification timestamp as UTC `NaiveDateTime`. Falls back to Unix epoch on any error.
/// Optionally takes a reference to an existing `File` handle.
#[builder]
pub fn file_modified_at(path: &Utf8Path, file: Option<&std::fs::File>) -> NaiveDateTime {
  trace!("Getting modified timestamp for file \"{}\"", path);

  let metadata = if let Some(file) = file {
    file.metadata()
  } else {
    std::fs::File::open(path).and_then(|f| f.metadata())
  }
  .inspect_err(|error| {
    error!(
      "Error while getting modified timestamp for file \"{}\": {error}",
      path
    )
  });

  metadata
    .and_then(|m| m.modified())
    .map(chrono::DateTime::<chrono::Utc>::from)
    .map(|dt| dt.naive_utc())
    .unwrap_or(*UNIX_EPOCH_NDT)
}
