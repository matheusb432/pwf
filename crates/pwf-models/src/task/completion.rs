use nutype::nutype;

/// Stores one non-empty, single-line task completion report.
#[nutype(
    sanitize(with = normalize_report),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, FromStr),
)]
pub struct TaskReport(String);

/// Stores normalized, de-duplicated task commit ranges.
#[nutype(
    sanitize(with = normalize_commit_ranges),
    validate(not_empty),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, FromStr)
)]
pub struct CommitRanges(String);

impl CommitRanges {
    /// Constructs ranges from repeated and comma-separated CLI values.
    #[must_use]
    pub fn from_inputs(values: &[String]) -> Option<Self> {
        Self::try_new(values.join(",")).ok()
    }
}

fn normalize_commit_ranges(value: String) -> String {
    let mut ranges = Vec::new();
    for range in value
        .split(',')
        .map(str::trim)
        .filter(|range| !range.is_empty())
    {
        if !ranges.contains(&range) {
            ranges.push(range);
        }
    }
    let normalized = ranges.iter().enumerate().flat_map(|(index, range)| {
        (if index == 0 { "" } else { ", " })
            .bytes()
            .chain(range.bytes())
    });
    if normalized.eq(value.bytes()) {
        return value;
    }
    ranges.join(", ")
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "nutype string sanitizers receive owned values"
)]
fn normalize_report(value: String) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
