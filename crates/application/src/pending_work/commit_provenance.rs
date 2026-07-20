pub(super) fn normalize(values: &[String]) -> Option<String> {
    let mut ranges = Vec::new();
    for range in values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|range| !range.is_empty())
    {
        if !ranges.contains(&range) {
            ranges.push(range);
        }
    }
    (!ranges.is_empty()).then(|| ranges.join(", "))
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn repeated_raw_ranges_are_normalized_in_first_seen_order() {
        let values = [" a..b,c..d ", "a..b", "", " e..f "]
            .map(str::to_string)
            .to_vec();

        assert_eq!(normalize(&values).as_deref(), Some("a..b, c..d, e..f"));
        assert_eq!(normalize(&[String::new(), "  , ".to_string()]), None);
    }
}
