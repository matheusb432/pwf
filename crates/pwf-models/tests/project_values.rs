use pwf_models::project::{PROJECT_NAME_CHARACTER_LIMIT, ProjectCreatedAt, ProjectName};

#[test]
fn project_name_is_a_bounded_safe_filename_component() {
    assert_eq!(ProjectName::try_new(" foo ").unwrap().as_ref(), "foo");
    assert!(ProjectName::try_new("p".repeat(PROJECT_NAME_CHARACTER_LIMIT)).is_ok());

    for invalid in [
        "",
        "project",
        "nested/name",
        "nested\\name",
        "control\nname",
    ] {
        assert!(
            ProjectName::try_new(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
    assert!(ProjectName::try_new("p".repeat(PROJECT_NAME_CHARACTER_LIMIT + 1)).is_err());
}

#[test]
fn project_created_at_accepts_only_utc_timestamps() {
    let created_at = ProjectCreatedAt::try_new("2026-08-12T15:30:45.123Z").unwrap();

    assert_eq!(created_at.as_ref(), "2026-08-12T15:30:45.123Z");
    for invalid in [
        "",
        "2026-08-12",
        "2026-08-12T15:30:45",
        "2026-08-12T12:30:45-03:00",
    ] {
        assert!(
            ProjectCreatedAt::try_new(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
}
