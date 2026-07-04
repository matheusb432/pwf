//! Date stamping: the supplied `--date` override, else today (local).

use chrono::Local;

/// `date` when `Some`, else today's `YYYY-MM-DD` in local time.
pub fn stamp_date(date: Option<&str>) -> String {
    match date {
        Some(d) => d.to_string(),
        None => Local::now().format("%Y-%m-%d").to_string(),
    }
}
