use chrono::NaiveDateTime;

/// Get current UTC timestamp as `NaiveDateTime`.
pub fn now() -> chrono::NaiveDateTime {
    chrono::Utc::now().naive_utc()
}

/// Get file modification timestamp as UTC `NaiveDateTime`. Falls back to Unix epoch on any error.
pub fn file_modified_at(file: &std::fs::File) -> NaiveDateTime {
    file.metadata()
        .and_then(|m| m.modified())
        .map(chrono::DateTime::<chrono::Utc>::from)
        .unwrap_or_else(|_| chrono::DateTime::from_timestamp_secs(0).expect("valid timestamp"))
        .naive_utc()
}
