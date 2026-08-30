use crate::support::{assert_failure, command};

#[test]
fn project_edit_requires_a_source_update() {
    let output = command().args(["project", "edit", "foo"]).output().unwrap();

    assert_failure(output, &["--source <SOURCE>"]).unwrap();
}
