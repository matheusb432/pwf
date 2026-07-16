use crate::pending_work::{Timestamp, WorkItemId};

/// Per-section done-queue caps: oldest links beyond the cap are evicted after a close.
///
/// Exact port of `crates/infra/src/obsidian/done_queue.rs`'s `SECTION_CAPS` table.
const SECTION_CAPS: &[(&str, usize)] =
    &[("General", 6), ("Low-prio", 3), ("Future", 3), ("Human", 3)];

/// One parsed done-queue link, independent of its markdown source line.
///
/// `completed` is `None` for an open link (`- [ ] [[ID]]`) and `Some(date)` for a done
/// link (`- [x] [[ID]] ✅ <date>`). `section` is the entry's raw (uncanonicalized) H2
/// label — e.g. `"Futuro"` or `"General"` when the link sits above any header, mirroring
/// `section_at_line`'s `"General"` fallback in the infra port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueEntryView {
    pub id: WorkItemId,
    pub completed: Option<Timestamp>,
    pub section: String,
}

/// Domain-owned mirror of the just-closed entry.
///
/// Domain cannot see application's `IndexEntry`; the done/cancel handler maps
/// `MarkedEntry` -> `IndexEntry { state: Done(completed) }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedEntry {
    pub id: WorkItemId,
    pub section_canonical: String,
    pub completed: Timestamp,
}

/// Decisions produced by closing an item: which links to evict, whether the legacy
/// `## Futuro` header should be renamed to `## Future`, and what the closed entry
/// itself now looks like.
///
/// `marked_entry` is `None` when `id` has no matching open entry in `entries` — the
/// same "target missing" case `mark_done` handles by applying only the futuro
/// rename and skipping eviction (see module docs on the interface deviation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseDecisions {
    pub evicted_ids: Vec<WorkItemId>,
    pub normalize_futuro_header: bool,
    pub marked_entry: Option<MarkedEntry>,
}

/// Decides eviction, futuro-header normalization, and the marked entry's canonical
/// section for closing `id` as of `date`.
///
/// # Interface deviations from the brief
///
/// - `section_labels` is an addition to the brief's `close_decisions(entries, id, date)` signature.
///   `entries` alone cannot express an empty `## Futuro` section (no done-queue links under it) or
///   a `## Futuro` header that both exists and has no bearing on the entry being closed; the
///   futuro-rename decision in `mark_done` scans every header line in the document unconditionally,
///   regardless of `id`'s section or match state. Passing the raw H2 labels (application already
///   has these cheaply via the `IndexSection` list record) lets `close_decisions` answer that
///   document-wide question honestly.
/// - `marked_entry` is `Option<MarkedEntry>` rather than the brief's plain `MarkedEntry`.
///   `mark_done` returns a "no marking, futuro rename only" result when `id`'s open link is missing
///   from the index (a real, already-exercised path in `mark_done`, not a hypothetical) — an
///   entries-only, non-optional `MarkedEntry` cannot represent "closed an item with no queue link
///   to mark".
///
/// # Examples
///
/// ```
/// use pwf_domain::pending_work::{QueueEntryView, Timestamp, WorkItemId, close_decisions};
///
/// let id = WorkItemId::try_new("PWF-0007").unwrap();
/// let entries = vec![QueueEntryView {
///     id: id.clone(),
///     completed: None,
///     section: "General".to_string(),
/// }];
/// let decisions = close_decisions(&entries, &[], &id, &Timestamp::new("2026-07-07"));
///
/// assert!(decisions.evicted_ids.is_empty());
/// assert!(!decisions.normalize_futuro_header);
/// assert_eq!(decisions.marked_entry.unwrap().section_canonical, "General");
/// ```
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

/// Collects the ids evicted when the section holding `id` (marked done as of `date`)
/// exceeds its cap: done links in `section_canonical` — the just-marked entry included —
/// sorted oldest-first by `(completed, id)` (a missing/empty date sorts first), with the
/// oldest `len - cap` evicted. A section with no configured cap evicts nothing.
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

/// Result of reopening a done/cancelled item back onto the done-queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReopenDecision {
    /// A done link for the id exists in `entries`: flip it back to open in place.
    RestoreExisting,
    /// No link for the id exists in `entries`: it was evicted past the cap, so a
    /// fresh open link must be appended instead of restored.
    ReAddEvicted,
    /// An open link for the id already exists in `entries`: nothing to do.
    AlreadyOpen,
}

/// Decides how to reopen `id` onto the done-queue given the current entries.
///
/// # Examples
///
/// ```
/// use pwf_domain::pending_work::{QueueEntryView, ReopenDecision, WorkItemId, reopen_decision};
///
/// let id = WorkItemId::try_new("PWF-0001").unwrap();
/// let entries = vec![QueueEntryView {
///     id: id.clone(),
///     completed: None,
///     section: "General".to_string(),
/// }];
///
/// assert_eq!(reopen_decision(&entries, &id), ReopenDecision::AlreadyOpen);
/// ```
pub fn reopen_decision(entries: &[QueueEntryView], id: &WorkItemId) -> ReopenDecision {
    match entries.iter().find(|entry| &entry.id == id) {
        Some(entry) if entry.completed.is_some() => ReopenDecision::RestoreExisting,
        Some(_) => ReopenDecision::AlreadyOpen,
        None => ReopenDecision::ReAddEvicted,
    }
}

/// Whether a raw H2 label is a legacy `## Futuro` header that must be normalized
/// to `## Future` on close — case-insensitive, whitespace-trimmed. The single
/// source for the futuro-header question, shared by [`close_decisions`] (the
/// rename *decision*) and the application close handler (which renames each
/// matching label through the section-label write seam).
#[must_use]
pub fn is_futuro_label(label: &str) -> bool {
    label.trim().eq_ignore_ascii_case("futuro")
}

/// Returns the configured done-queue cap for an already-canonicalized section name,
/// or `None` when the section has no cap (nothing is ever evicted from it).
pub fn section_cap(section_canonical: &str) -> Option<usize> {
    SECTION_CAPS
        .iter()
        .find(|(name, _)| *name == section_canonical)
        .map(|(_, cap)| *cap)
}

/// Aliases legacy/loose raw section labels onto their canonical done-queue name
/// (`future`/`futuro` -> `Future`, `human` -> `Human`, `low-prio`/`low-priority` ->
/// `Low-prio`), passing through any other trimmed label unchanged.
///
/// Exact port of `crates/infra/src/obsidian/done_queue.rs`'s `canonical_section`.
fn canonical_section(label: &str) -> String {
    match label.trim().to_lowercase().as_str() {
        "future" | "futuro" => "Future".to_string(),
        "human" => "Human".to_string(),
        "low-prio" | "low-priority" => "Low-prio".to_string(),
        other => other.to_string(),
    }
}

/// Resolves a `QueueEntryView.section` raw label the way the infra port's
/// `section_at_line` resolves an entry's section: a real header label goes through
/// [`canonical_section`], while the literal `"General"` sentinel — the producer's
/// contractual stand-in for "no enclosing header" — passes through unchanged rather
/// than being re-lowercased by `canonical_section`'s passthrough arm.
///
/// `section_at_line` never re-canonicalizes its own `"General"` fallback (the
/// fallback runs in `unwrap_or_else`, entirely outside the `canonical_section` call);
/// `canonical_section` is only ever fed a genuinely matched header label there. This
/// wrapper exists because `QueueEntryView` collapses both cases into one flat
/// string, so it re-splits them here instead of re-lowercasing the sentinel.
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

    /// Transliterated from `complete_item_rotates_done_queue_and_keeps_evicted_notes`
    /// (crates/infra/src/obsidian/store/tests.rs): 6 done General entries + closing a
    /// 7th evicts exactly the oldest one.
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

    /// A section absent from `SECTION_CAPS` never evicts, no matter how many done
    /// links it accumulates.
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

    /// Raw `Futuro`/`futuro`/`low-priority` labels canonicalize onto the aliased name,
    /// and the aliased name's cap (not any per-raw-label cap) governs eviction.
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

    /// The futuro rename decision is document-wide (any `## Futuro` header, from
    /// `section_labels`) and independent of whether `id` matches an entry at all —
    /// mirroring `mark_done`'s unconditional rename loop that runs before it looks
    /// for the target link.
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
