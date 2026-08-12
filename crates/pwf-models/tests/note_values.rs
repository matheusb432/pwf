use pwf_models::{
    note::{
        NOTE_TITLE_CHARACTER_LIMIT, NoteContent, NoteDomain, NoteSelector, NoteSource, NoteTag,
        NoteTitle, NoteVerification, NoteWhy,
    },
    project::ProjectId,
};

#[test]
fn note_title_normalizes_inline_whitespace_and_enforces_its_bound() {
    let title = NoteTitle::try_new("  Keep  the\nuseful detail  ").unwrap();

    assert_eq!(title.as_ref(), "Keep the useful detail");
    assert!(NoteTitle::try_new(" \t ").is_err());
    assert!(NoteTitle::try_new("e".repeat(NOTE_TITLE_CHARACTER_LIMIT)).is_ok());
    assert!(NoteTitle::try_new("e".repeat(NOTE_TITLE_CHARACTER_LIMIT + 1)).is_err());
}

#[test]
fn note_content_and_metadata_construct_only_meaningful_values() {
    assert_eq!(
        NoteContent::try_new("  first line\n\nsecond line  ")
            .unwrap()
            .as_ref(),
        "first line\n\nsecond line"
    );
    assert_eq!(
        NoteWhy::try_new("  preserve\nthis block  ")
            .unwrap()
            .as_ref(),
        "preserve\nthis block"
    );
    assert_eq!(
        NoteDomain::try_new("  testing   strategy ")
            .unwrap()
            .as_ref(),
        "testing strategy"
    );
    assert_eq!(NoteTag::try_new(" cli ").unwrap().as_ref(), "cli");
    assert_eq!(
        NoteSource::try_new("  issue   PWF-0001 ").unwrap().as_ref(),
        "issue PWF-0001"
    );
    assert_eq!(
        NoteVerification::try_new("  checked   locally ")
            .unwrap()
            .as_ref(),
        "checked locally"
    );

    assert!(NoteContent::try_new(" \n ").is_err());
    assert!(NoteWhy::try_new(" \n ").is_err());
    assert!(NoteDomain::try_new(" \n ").is_err());
    assert!(NoteTag::try_new(" \n ").is_err());
    assert!(NoteSource::try_new(" \n ").is_err());
    assert!(NoteVerification::try_new(" \n ").is_err());
}

#[test]
fn note_selector_resolves_supported_aliases_for_one_project() {
    let project_id = ProjectId::try_new("PWF").unwrap();

    for raw in ["PWF-NOTE-0007", "pwf-note-0007", "NOTE-0007", "7"] {
        let selector = raw.parse::<NoteSelector>().unwrap();
        assert_eq!(
            selector.resolve(&project_id).unwrap().as_ref(),
            "PWF-NOTE-0007"
        );
    }

    let other = "FOO-NOTE-0007".parse::<NoteSelector>().unwrap();
    assert!(other.resolve(&project_id).is_none());
    for invalid in ["", "NOTE-", "NOTE-10000", "PWF-NOTE-007", "bad"] {
        assert!(
            invalid.parse::<NoteSelector>().is_err(),
            "accepted {invalid:?}"
        );
    }
}
