pub(super) fn slug(value: &str) -> String {
    let mut result = String::new();
    let mut separator_pending = false;
    for character in value.trim().to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if separator_pending && !result.is_empty() {
                result.push('-');
            }
            result.push(character);
            separator_pending = false;
        } else {
            separator_pending = true;
        }
    }
    if result.is_empty() {
        "handoff".to_string()
    } else {
        result
    }
}

pub(super) fn continuation_title(file_name: &str) -> String {
    let stem = file_name.strip_suffix(".md").unwrap_or(file_name);
    let slug = strip_date_prefix(stem);
    let words = slug
        .split(['-', '_'])
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if words.is_empty() {
        "continue handoff".to_string()
    } else {
        format!("continue {}", words.join(" "))
    }
}

pub(super) fn file_name(created: &str, title_or_slug: &str) -> String {
    format!("{created}-{}.md", slug(title_or_slug))
}

fn strip_date_prefix(value: &str) -> &str {
    let bytes = value.as_bytes();
    let is_date_prefix = bytes.len() >= 11
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'-';
    if is_date_prefix { &value[11..] } else { value }
}

#[cfg(test)]
mod tests {
    use super::{continuation_title, slug};

    #[test]
    fn slug_preserves_the_ascii_filename_policy() {
        assert_eq!(slug("Managed Flow"), "managed-flow");
        assert_eq!(slug("  Hello World!! "), "hello-world");
        assert_eq!(slug("déjà vu"), "d-j-vu");
        assert_eq!(slug("---"), "handoff");
    }

    #[test]
    fn continuation_title_strips_a_dated_file_prefix() {
        assert_eq!(
            continuation_title("2026-01-01-api_cleanup.md"),
            "continue api cleanup"
        );
        assert_eq!(continuation_title("2026-01-01-.md"), "continue handoff");
    }
}
