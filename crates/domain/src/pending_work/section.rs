/// Returns the canonical label for a recognized pending-work section alias.
///
/// Unknown labels return [`None`] so each consumer can retain its established fallback.
///
/// # Examples
///
/// ```
/// use pwf_domain::pending_work::section_alias;
///
/// assert_eq!(section_alias(" Futuro "), Some("Future"));
/// assert_eq!(section_alias("Someday"), None);
/// ```
#[must_use]
pub fn section_alias(label: &str) -> Option<&'static str> {
    match label.trim().to_lowercase().as_str() {
        "future" | "futuro" => Some("Future"),
        "human" => Some("Human"),
        "low-prio" | "low-priority" => Some("Low-prio"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::section_alias;

    #[test]
    fn section_alias_maps_only_known_labels() {
        assert_eq!(section_alias(" Futuro "), Some("Future"));
        assert_eq!(section_alias("future"), Some("Future"));
        assert_eq!(section_alias("HUMAN"), Some("Human"));
        assert_eq!(section_alias("low-priority"), Some("Low-prio"));
        assert_eq!(section_alias("Someday"), None);
    }
}
