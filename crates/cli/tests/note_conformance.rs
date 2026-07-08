//! Cross-engine conformance: the pending-work task engine and the note engine
//! never conflict. (PWF-0081)

use pwf::engines::pending_work::get_project_tasks;

#[test]
fn task_parser_ignores_note_links_in_the_notes_section() {
    let dir = tempfile::TempDir::new().unwrap();
    let index = dir.path().join("pwf.md");
    std::fs::write(
        &index,
        "- [ ] [[PWF-0001|real task]]\n\n### Notes\n- [[PWF-NOTE-0001]]\n- [[PWF-NOTE-0002]]\n",
    )
    .unwrap();

    let items = get_project_tasks("pwf", Some("/repo"), &index);
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert!(ids.contains(&"PWF-0001"), "real task missing: {ids:?}");
    assert!(
        ids.iter().all(|id| !id.contains("NOTE")),
        "a note id leaked into the task list: {ids:?}"
    );
}
