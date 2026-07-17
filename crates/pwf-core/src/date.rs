//! Local date stamping with an optional explicit override.

use chrono::Local;

/// Returns the override or today's local date in `YYYY-MM-DD` form.
pub fn stamp_date(date: Option<&str>) -> String {
    match date {
        Some(d) => d.to_string(),
        None => Local::now().format("%Y-%m-%d").to_string(),
    }
}
