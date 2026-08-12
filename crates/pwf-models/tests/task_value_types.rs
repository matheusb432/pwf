use pwf_models::{
    AppDate,
    task::{IndexSection, TaskPrompt, TaskSection},
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
fn task_prompt_preserves_authored_text() {
    let prompt = TaskPrompt::new("  ship it /c preserve spacing  ");

    assert_eq!(prompt.as_ref(), "  ship it /c preserve spacing  ");
}

#[test]
fn task_section_is_non_empty_and_single_line() {
    assert_eq!(TaskSection::try_new("  Human  ").unwrap().as_ref(), "Human");
    assert!(TaskSection::try_new("  ").is_err());
    assert!(TaskSection::try_new("Human\nFuture").is_err());
}

#[test]
fn index_section_maps_the_add_choice_to_a_task_section() {
    assert_eq!(IndexSection::default(), IndexSection::General);
    assert_eq!(IndexSection::default().task_section(), None);
    assert_eq!(
        IndexSection::Human.task_section().unwrap().as_ref(),
        "Human"
    );
}
