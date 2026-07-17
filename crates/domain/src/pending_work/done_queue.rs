use crate::pending_work::{Timestamp, WorkItemId, section_alias};

/// Per-section done-queue caps applied after a close.
const SECTION_CAPS: &[(&str, usize)] =
    &[("General", 6), ("Low-prio", 3), ("Future", 3), ("Human", 3)];

/// Represents one queue link independently of its Markdown source line.
///
/// `completed` distinguishes open and done links. `section` retains the raw H2 label; `"General"`
/// also represents a link above every H2 header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueEntryView {
    pub id: WorkItemId,
    pub completed: Option<Timestamp>,
    pub section: String,
}

/// Describes the closed entry for the application-layer index update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedEntry {
    pub id: WorkItemId,
    pub section_canonical: String,
    pub completed: Timestamp,
}

/// Contains queue mutations produced by closing an item.
///
/// `marked_entry` is [`None`] when the queue has no matching open entry. Header normalization can
/// still be required in that case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseDecisions {
    pub evicted_ids: Vec<WorkItemId>,
    pub normalize_futuro_header: bool,
    pub marked_entry: Option<MarkedEntry>,
}

/// Computes the close-time mark, eviction, and `Futuro` header normalization decisions.
///
/// Separate `section_labels` are required because entries cannot represent an empty H2 section.
pub fn close_decisions(
    entries: &[QueueEntryView],
    section_labels: &[String],
    id: &WorkItemId,
    date: &Timestamp,
) -> CloseDecisions {
    let normalize_futuro_header = section_labels.iter().any(|label| is_futuro_label(label));

    let Some(target) = entries
        .iter()
        .find(|entry| &entry.id == id && entry.completed.is_none())
    else {
        return CloseDecisions {
            evicted_ids: Vec::new(),
            normalize_futuro_header,
            marked_entry: None,
        };
    };

    let section_canonical = resolve_section(&target.section);
    let evicted_ids = evict_beyond_cap(entries, &section_canonical, id, date);
    let marked_entry = MarkedEntry {
        id: id.clone(),
        section_canonical,
        completed: date.clone(),
    };

    CloseDecisions {
        evicted_ids,
        normalize_futuro_header,
        marked_entry: Some(marked_entry),
    }
}

/// Returns excess ids ordered by completion date and id, including the item being closed.
///
/// Empty dates sort first. Sections without a cap never evict.
fn evict_beyond_cap(
    entries: &[QueueEntryView],
    section_canonical: &str,
    id: &WorkItemId,
    date: &Timestamp,
) -> Vec<WorkItemId> {
    let Some(cap) = section_cap(section_canonical) else {
        return Vec::new();
    };

    let mut done: Vec<(&str, &WorkItemId)> = entries
        .iter()
        .filter(|entry| {
            entry.completed.is_some() && resolve_section(&entry.section) == section_canonical
        })
        .map(|entry| {
            (
                entry
                    .completed
                    .as_ref()
                    .expect("filtered on is_some")
                    .as_str(),
                &entry.id,
            )
        })
        .collect();
    done.push((date.as_str(), id));

    if done.len() <= cap {
        return Vec::new();
    }
    let evict_count = done.len() - cap;
    done.sort_by(|left, right| left.0.cmp(right.0).then(left.1.cmp(right.1)));
    done.into_iter()
        .take(evict_count)
        .map(|(_, evicted_id)| evicted_id.clone())
        .collect()
}

/// Selects the queue mutation needed to reopen an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReopenDecision {
    /// Restores an existing done link in place.
    RestoreExisting,
    /// Appends a new open link after the prior link was evicted.
    ReAddEvicted,
    /// Leaves an existing open link unchanged.
    AlreadyOpen,
}

/// Selects the queue mutation needed to reopen `id`.
pub fn reopen_decision(entries: &[QueueEntryView], id: &WorkItemId) -> ReopenDecision {
    match entries.iter().find(|entry| &entry.id == id) {
        Some(entry) if entry.completed.is_some() => ReopenDecision::RestoreExisting,
        Some(_) => ReopenDecision::AlreadyOpen,
        None => ReopenDecision::ReAddEvicted,
    }
}

/// Reports whether a trimmed H2 label is `Futuro`, case-insensitively.
#[must_use]
pub fn is_futuro_label(label: &str) -> bool {
    label.trim().eq_ignore_ascii_case("futuro")
}

/// Returns the cap for a canonical section name, or [`None`] for an uncapped section.
pub fn section_cap(section_canonical: &str) -> Option<usize> {
    SECTION_CAPS
        .iter()
        .find(|(name, _)| *name == section_canonical)
        .map(|(_, cap)| *cap)
}

/// Resolves known aliases and lowercases any other trimmed label.
fn canonical_section(label: &str) -> String {
    section_alias(label).map_or_else(|| label.trim().to_lowercase(), str::to_string)
}

/// Preserves the `"General"` no-header sentinel while canonicalizing real H2 labels.
fn resolve_section(raw: &str) -> String {
    if raw.trim() == "General" {
        "General".to_string()
    } else {
        canonical_section(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> WorkItemId {
        WorkItemId::try_new(raw).unwrap()
    }

    fn done(raw_id: &str, date: &str, section: &str) -> QueueEntryView {
        QueueEntryView {
            id: id(raw_id),
            completed: Some(Timestamp::new(date)),
            section: section.to_string(),
        }
    }

    fn open(raw_id: &str, section: &str) -> QueueEntryView {
        QueueEntryView {
            id: id(raw_id),
            completed: None,
            section: section.to_string(),
        }
    }

    #[test]
    fn cap_boundary_evicts_single_oldest_beyond_cap() {
        let mut entries: Vec<QueueEntryView> = (1..=6)
            .map(|n| {
                done(
                    &format!("PWF-{n:04}"),
                    &format!("2026-01-{n:02}"),
                    "General",
                )
            })
            .collect();
        entries.push(open("PWF-0007", "General"));

        let decisions = close_decisions(
            &entries,
            &[],
            &id("PWF-0007"),
            &Timestamp::new("2026-07-07"),
        );

        assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
        assert_eq!(
            decisions.marked_entry,
            Some(MarkedEntry {
                id: id("PWF-0007"),
                section_canonical: "General".to_string(),
                completed: Timestamp::new("2026-07-07"),
            })
        );
    }

    #[test]
    fn tied_completed_dates_break_by_ascending_id() {
        let entries = vec![
            done("PWF-0002", "2026-01-01", "Human"),
            done("PWF-0001", "2026-01-01", "Human"),
            done("PWF-0003", "2026-01-02", "Human"),
            open("PWF-0004", "Human"),
        ];

        let decisions = close_decisions(
            &entries,
            &[],
            &id("PWF-0004"),
            &Timestamp::new("2026-01-03"),
        );

        assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
    }

    #[test]
    fn missing_completed_date_sorts_before_any_dated_entry() {
        let entries = vec![
            done("PWF-0001", "", "Human"),
            done("PWF-0002", "2026-01-01", "Human"),
            done("PWF-0003", "2026-01-02", "Human"),
            open("PWF-0004", "Human"),
        ];

        let decisions = close_decisions(
            &entries,
            &[],
            &id("PWF-0004"),
            &Timestamp::new("2026-01-03"),
        );

        assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
    }

    #[test]
    fn section_without_a_cap_evicts_nothing() {
        let mut entries: Vec<QueueEntryView> = (1..=9)
            .map(|n| {
                done(
                    &format!("PWF-{n:04}"),
                    &format!("2026-01-{n:02}"),
                    "Someday",
                )
            })
            .collect();
        entries.push(open("PWF-0010", "Someday"));

        let decisions = close_decisions(
            &entries,
            &[],
            &id("PWF-0010"),
            &Timestamp::new("2026-07-07"),
        );

        assert!(decisions.evicted_ids.is_empty());
    }

    #[test]
    fn unknown_section_canonicalization_lowercases_trimmed_label() {
        assert_eq!(canonical_section(" SomeDay "), "someday");
    }

    #[test]
    fn raw_section_label_aliases_before_cap_lookup() {
        let mut entries: Vec<QueueEntryView> = (1..=3)
            .map(|n| done(&format!("PWF-{n:04}"), &format!("2026-01-{n:02}"), "futuro"))
            .collect();
        entries.push(open("PWF-0004", "Futuro"));

        let decisions = close_decisions(
            &entries,
            &[],
            &id("PWF-0004"),
            &Timestamp::new("2026-07-07"),
        );

        assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
        assert_eq!(
            decisions.marked_entry.unwrap().section_canonical,
            "Future".to_string()
        );
    }

    #[test]
    fn futuro_header_normalizes_even_when_target_entry_is_missing() {
        let decisions = close_decisions(
            &[],
            &["Futuro".to_string()],
            &id("PWF-0001"),
            &Timestamp::new("2026-07-07"),
        );

        assert!(decisions.normalize_futuro_header);
        assert!(decisions.marked_entry.is_none());
        assert!(decisions.evicted_ids.is_empty());
    }

    #[test]
    fn futuro_header_check_is_case_insensitive_and_trims_whitespace() {
        let decisions = close_decisions(
            &[],
            &["  FUTURO  ".to_string()],
            &id("PWF-0001"),
            &Timestamp::new("2026-07-07"),
        );

        assert!(decisions.normalize_futuro_header);
    }

    #[test]
    fn no_futuro_label_does_not_normalize() {
        let decisions = close_decisions(
            &[],
            &["Human".to_string(), "Future".to_string()],
            &id("PWF-0001"),
            &Timestamp::new("2026-07-07"),
        );

        assert!(!decisions.normalize_futuro_header);
    }

    #[test]
    fn reopen_restores_existing_done_link() {
        let entries = vec![done("PWF-0001", "2026-07-07", "General")];

        assert_eq!(
            reopen_decision(&entries, &id("PWF-0001")),
            ReopenDecision::RestoreExisting
        );
    }

    #[test]
    fn reopen_re_adds_when_evicted_past_cap() {
        let entries = vec![done("PWF-0002", "2026-07-07", "General")];

        assert_eq!(
            reopen_decision(&entries, &id("PWF-0001")),
            ReopenDecision::ReAddEvicted
        );
    }

    #[test]
    fn reopen_is_a_noop_when_already_open() {
        let entries = vec![open("PWF-0001", "General")];

        assert_eq!(
            reopen_decision(&entries, &id("PWF-0001")),
            ReopenDecision::AlreadyOpen
        );
    }

    #[test]
    fn section_cap_matches_configured_table_exactly() {
        assert_eq!(section_cap("General"), Some(6));
        assert_eq!(section_cap("Low-prio"), Some(3));
        assert_eq!(section_cap("Future"), Some(3));
        assert_eq!(section_cap("Human"), Some(3));
        assert_eq!(section_cap("Someday"), None);
    }
}
