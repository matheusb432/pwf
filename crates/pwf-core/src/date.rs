//! Date stamping: the supplied `--date` override, else today (local).

use chrono::Local;

/// `date` when `Some`, else today's `YYYY-MM-DD` in local time.
pub fn stamp_date(date: &Option<String>) -> String {
    match date {
        Some(d) => d.clone(),
        None => Local::now().format("%Y-%m-%d").to_string(),
    }
}
