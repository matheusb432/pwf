use pwf_domain::pending_work::{Timestamp, WorkItemId, section_alias};

use crate::{IndexEntry, IndexEntryState, IndexSection};

const SECTION_CAPS: &[(&str, usize)] =
    &[("General", 6), ("Low-prio", 3), ("Future", 3), ("Human", 3)];

pub(super) struct CloseDecisions {
    pub(super) evicted_ids: Vec<WorkItemId>,
    pub(super) normalize_futuro_header: bool,
    pub(super) mark_target: bool,
}

pub(super) fn close_decisions(
    entries: &[IndexEntry],
    sections: &[IndexSection],
    id: &WorkItemId,
    completed: &Timestamp,
) -> CloseDecisions {
    let normalize_futuro_header = sections
        .iter()
        .any(|section| is_futuro_label(&section.label));

    let Some(target) = entries
        .iter()
        .find(|entry| &entry.id == id && entry.state == IndexEntryState::Open)
    else {
        return CloseDecisions {
            evicted_ids: Vec::new(),
            normalize_futuro_header,
            mark_target: false,
        };
    };

    CloseDecisions {
        evicted_ids: evict_beyond_cap(entries, &target.section, id, completed),
        normalize_futuro_header,
        mark_target: true,
    }
}

fn evict_beyond_cap(
    entries: &[IndexEntry],
    target_section: &str,
    id: &WorkItemId,
    completed: &Timestamp,
) -> Vec<WorkItemId> {
    let target_section = canonical_section(target_section);
    let Some(cap) = section_cap(&target_section) else {
        return Vec::new();
    };

    let mut done: Vec<(&str, &WorkItemId)> = entries
        .iter()
        .filter_map(|entry| match &entry.state {
            IndexEntryState::Done(entry_completed)
                if canonical_section(&entry.section) == target_section =>
            {
                Some((entry_completed.as_str(), &entry.id))
            }
            IndexEntryState::Open | IndexEntryState::Done(_) => None,
        })
        .collect();
    done.push((completed.as_str(), id));

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

pub(super) fn is_futuro_label(label: &str) -> bool {
    label.trim().eq_ignore_ascii_case("futuro")
}

fn section_cap(section_canonical: &str) -> Option<usize> {
    SECTION_CAPS
        .iter()
        .find(|(name, _)| *name == section_canonical)
        .map(|(_, cap)| *cap)
}

fn canonical_section(label: &str) -> String {
    if label.trim().is_empty() || label.trim() == "General" {
        return "General".to_string();
    }
    section_alias(label).map_or_else(|| label.trim().to_lowercase(), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> WorkItemId {
        WorkItemId::try_new(raw).unwrap()
    }

    fn done(raw_id: &str, date: &str, section: &str) -> IndexEntry {
        IndexEntry {
            id: id(raw_id),
            state: IndexEntryState::Done(Timestamp::new(date)),
            section: section.to_string(),
        }
    }

    fn open(raw_id: &str, section: &str) -> IndexEntry {
        IndexEntry {
            id: id(raw_id),
            state: IndexEntryState::Open,
            section: section.to_string(),
        }
    }

    #[test]
    fn cap_boundary_evicts_single_oldest_beyond_cap() {
        let mut entries: Vec<IndexEntry> = (1..=6)
            .map(|number| {
                done(
                    &format!("PWF-{number:04}"),
                    &format!("2026-01-{number:02}"),
                    "",
                )
            })
            .collect();
        entries.push(open("PWF-0007", ""));

        let decisions = close_decisions(
            &entries,
            &[],
            &id("PWF-0007"),
            &Timestamp::new("2026-07-07"),
        );

        assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
        assert!(decisions.mark_target);
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
        let mut entries: Vec<IndexEntry> = (1..=9)
            .map(|number| {
                done(
                    &format!("PWF-{number:04}"),
                    &format!("2026-01-{number:02}"),
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
    fn raw_section_label_aliases_before_cap_lookup() {
        let mut entries: Vec<IndexEntry> = (1..=3)
            .map(|number| {
                done(
                    &format!("PWF-{number:04}"),
                    &format!("2026-01-{number:02}"),
                    "futuro",
                )
            })
            .collect();
        entries.push(open("PWF-0004", "Futuro"));

        let decisions = close_decisions(
            &entries,
            &[],
            &id("PWF-0004"),
            &Timestamp::new("2026-07-07"),
        );

        assert_eq!(decisions.evicted_ids, vec![id("PWF-0001")]);
    }

    #[test]
    fn futuro_header_normalizes_when_target_entry_is_missing() {
        let sections = [IndexSection {
            label: "Futuro".to_string(),
        }];

        let decisions = close_decisions(
            &[],
            &sections,
            &id("PWF-0001"),
            &Timestamp::new("2026-07-07"),
        );

        assert!(decisions.normalize_futuro_header);
        assert!(!decisions.mark_target);
        assert!(decisions.evicted_ids.is_empty());
    }

    #[test]
    fn futuro_header_check_is_case_insensitive_and_trims_whitespace() {
        let sections = [IndexSection {
            label: "  FUTURO  ".to_string(),
        }];

        let decisions = close_decisions(
            &[],
            &sections,
            &id("PWF-0001"),
            &Timestamp::new("2026-07-07"),
        );

        assert!(decisions.normalize_futuro_header);
    }

    #[test]
    fn unrelated_headers_do_not_normalize() {
        let sections = [
            IndexSection {
                label: "Human".to_string(),
            },
            IndexSection {
                label: "Future".to_string(),
            },
        ];

        let decisions = close_decisions(
            &[],
            &sections,
            &id("PWF-0001"),
            &Timestamp::new("2026-07-07"),
        );

        assert!(!decisions.normalize_futuro_header);
    }
}
