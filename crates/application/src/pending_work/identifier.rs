use pwf_models::pending_work::WorkItemId;

pub(crate) fn normalize(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_uppercase();
    match split_compact(&trimmed) {
        Some((code, number)) => format!("{code}-{number:04}"),
        None => trimmed,
    }
}

pub(crate) fn parse(raw: &str) -> Option<WorkItemId> {
    WorkItemId::try_new(normalize(raw)).ok()
}

fn split_compact(raw: &str) -> Option<(&str, u32)> {
    let digit_start = raw.find(|character: char| character.is_ascii_digit())?;
    let (code, digits) = raw.split_at(digit_start);
    let code = code.strip_suffix('-').unwrap_or(code);
    if !(2..=4).contains(&code.len())
        || !code.chars().all(|character| character.is_ascii_uppercase())
    {
        return None;
    }
    if digits.is_empty()
        || digits.len() > 4
        || !digits.chars().all(|character| character.is_ascii_digit())
    {
        return None;
    }
    Some((code, digits.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn loose_identifiers_normalize_all_accepted_forms() {
        assert_eq!(normalize("PWF-0098"), "PWF-0098");
        assert_eq!(normalize("pwf-0098"), "PWF-0098");
        assert_eq!(normalize("  pwf-0047  "), "PWF-0047");
        assert_eq!(normalize("pwf-98"), "PWF-0098");
        assert_eq!(normalize("cfg57"), "CFG-0057");
        assert_eq!(normalize("CFG57"), "CFG-0057");
        assert_eq!(normalize("cfg-57"), "CFG-0057");
        assert_eq!(normalize("garbage"), "GARBAGE");
        assert_eq!(normalize("PWF-0047-extra"), "PWF-0047-EXTRA");
        assert_eq!(normalize("toolong-0047"), "TOOLONG-0047");
    }
}
