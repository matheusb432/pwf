use pwf_models::{
    AppDate,
    task::{PriorityTier, TaskPrompt, TaskSection, TaskTimestamp},
};

#[test]
fn app_date_accepts_only_canonical_civil_dates() {
    let leap_day = "2024-02-29".parse::<AppDate>().unwrap();

    assert_eq!(leap_day.to_string(), "2024-02-29");
    assert_eq!(AppDate::from_calendar_date(2024, 2, 29).unwrap(), leap_day);
    assert!("2023-02-29".parse::<AppDate>().is_err());
    assert!(AppDate::from_calendar_date(2023, 2, 29).is_err());
    assert!("2024-2-29".parse::<AppDate>().is_err());
    assert!("2024-02-29T00:00:00Z".parse::<AppDate>().is_err());
}

#[test]
fn task_timestamp_accepts_only_utc_second_precision() {
    let timestamp = "2026-08-29T18:42:07Z".parse::<TaskTimestamp>().unwrap();

    assert_eq!(timestamp.to_string(), "2026-08-29T18:42:07Z");
    assert_eq!(timestamp.date(), "2026-08-29".parse::<AppDate>().unwrap());
    assert!("2026-08-29T18:42:07.123Z".parse::<TaskTimestamp>().is_err());
    assert!(
        "2026-08-29T18:42:07+00:00"
            .parse::<TaskTimestamp>()
            .is_err()
    );
    assert!("2026-08-29 18:42:07Z".parse::<TaskTimestamp>().is_err());
    assert!("2026-08-29".parse::<TaskTimestamp>().is_err());
}

#[test]
fn task_timestamp_converts_dates_and_truncates_external_instants() {
    let date = "2026-08-29".parse::<AppDate>().unwrap();
    let timestamp = "2026-08-29T18:42:07.987654321Z"
        .parse::<jiff::Timestamp>()
        .unwrap();

    assert_eq!(
        TaskTimestamp::at_midnight_utc(date).unwrap().to_string(),
        "2026-08-29T00:00:00Z"
    );
    assert_eq!(
        TaskTimestamp::from_timestamp(timestamp)
            .unwrap()
            .to_string(),
        "2026-08-29T18:42:07Z"
    );
}

#[test]
fn task_prompt_preserves_authored_text() {
    let prompt = TaskPrompt::new("  ship it /c preserve spacing  ");

    assert_eq!(prompt.as_ref(), "  ship it /c preserve spacing  ");
}

#[test]
fn task_section_is_non_empty_and_single_line() {
    let section = TaskSection::try_new("  Waiting on API  ").unwrap();

    assert_eq!(section.as_ref(), "Waiting on API");
    assert_eq!(section.case_insensitive_key(), "waiting on api");
    assert!(TaskSection::try_new("  ").is_err());
    assert!(TaskSection::try_new("Waiting\nBlocked").is_err());
}

#[test]
fn priority_tier_accepts_only_canonical_names() {
    for (raw, expected) in [
        ("low", PriorityTier::Low),
        ("medium", PriorityTier::Medium),
        ("high", PriorityTier::High),
        ("highest", PriorityTier::Highest),
    ] {
        let priority = raw.parse::<PriorityTier>().unwrap();

        assert_eq!(priority, expected);
        assert_eq!(priority.to_string(), raw);
    }

    for raw in ["1", "2", "3", "4", "urgent"] {
        assert!(raw.parse::<PriorityTier>().is_err(), "{raw} was accepted");
    }
}
