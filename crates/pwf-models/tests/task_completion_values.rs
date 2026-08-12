use pwf_models::task::{CommitRanges, TaskReport};

#[test]
fn task_report_collapses_authored_lines_and_rejects_blank_input() {
    let report = TaskReport::try_new("  first line\n\n  second line  ").unwrap();

    assert_eq!(report.as_ref(), "first line second line");
    assert!(TaskReport::try_new(" \n\t ").is_err());
}

#[test]
fn commit_ranges_normalize_repeated_and_comma_separated_inputs() {
    let inputs = [" a..b,c..d ", "a..b", "", " e..f "].map(str::to_string);

    assert_eq!(
        CommitRanges::from_inputs(&inputs).unwrap().as_ref(),
        "a..b, c..d, e..f"
    );
    assert!(CommitRanges::from_inputs(&[String::new(), "  , ".to_string()]).is_none());

    let parsed = " a..b, c..d, a..b ".parse::<CommitRanges>().unwrap();
    assert_eq!(parsed.as_ref(), "a..b, c..d");
}
