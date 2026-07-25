#[must_use]
pub(super) fn alias(label: &str) -> Option<&'static str> {
    match label.trim().to_ascii_lowercase().as_str() {
        "future" | "futuro" => Some("Future"),
        "human" => Some("Human"),
        "low-prio" | "low-priority" => Some("Low-prio"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::alias;

    #[test]
    fn aliases_map_only_known_labels() {
        assert_eq!(alias(" Futuro "), Some("Future"));
        assert_eq!(alias("future"), Some("Future"));
        assert_eq!(alias("HUMAN"), Some("Human"));
        assert_eq!(alias("low-priority"), Some("Low-prio"));
        assert_eq!(alias("Someday"), None);
    }
}
