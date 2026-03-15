use std::sync::LazyLock;

use bon::builder;
use camino::Utf8Path;
use chrono::NaiveDateTime;
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
