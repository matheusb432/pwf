use std::fs;

use pwf_infra::obsidian::{MarkdownFile, MarkdownFileError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct Metadata {
    node_id: u64,
    title: String,
    related: Vec<String>,
}

#[test]
fn creates_and_reads_typed_frontmatter() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("workspace/nodes/harbor.md");
    let metadata = Metadata {
        node_id: 42,
        title: "Harbor: north".to_string(),
        related: vec!["[[Signal Station]]".to_string()],
    };

    let file = MarkdownFile::create_new(&path, &metadata, "# Harbor\n").unwrap();

    assert_eq!(file.frontmatter::<Metadata>().unwrap(), Some(metadata));
    assert_eq!(file.body(), "\n# Harbor\n");
    assert_eq!(MarkdownFile::open(&path).unwrap().source(), file.source());
}

#[test]
fn reads_frontmatter_without_decoding_the_body() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("harbor.md");
    let mut source = concat!(
        "\u{feff}---\r\n",
        "node_id: 42\r\n",
        "title: 'Harbor: north'\r\n",
        "related: [\"[[Signal Station]]\"]\r\n",
        "---\r\n",
    )
    .as_bytes()
    .to_vec();
    source.extend_from_slice(&[0xff, 0xfe]);
    fs::write(&path, source).unwrap();

    let metadata = MarkdownFile::read_frontmatter::<Metadata>(path).unwrap();

    assert_eq!(
        metadata,
        Some(Metadata {
            node_id: 42,
            title: "Harbor: north".to_string(),
            related: vec!["[[Signal Station]]".to_string()],
        })
    );
}

#[test]
fn bounds_frontmatter_only_reads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oversized.md");
    let source = format!("---\ntitle: {}\n---\n", "x".repeat(1024 * 1024));
    fs::write(&path, source).unwrap();

    let error = MarkdownFile::read_frontmatter::<Metadata>(path).unwrap_err();

    assert!(matches!(
        error,
        MarkdownFileError::FrontmatterTooLarge { .. }
    ));
}

#[test]
fn exposes_one_validated_frontmatter_view() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("harbor.md");
    fs::write(
        &path,
        concat!(
            "---\n",
            "node_id: 42\n",
            "related:\n",
            "  - \"[[Signal Station]]\"\n",
            "title: Harbor\n",
            "---\n\n",
            "# Harbor\n",
        ),
    )
    .unwrap();
    let file = MarkdownFile::open(path).unwrap();

    let frontmatter = file.frontmatter_view().unwrap().unwrap();

    assert_eq!(frontmatter.get("node_id").unwrap(), Some("42"));
    assert_eq!(
        frontmatter.get("related").unwrap(),
        Some("- \"[[Signal Station]]\"")
    );
    assert_eq!(frontmatter.get("missing").unwrap(), None);
}

#[test]
fn updates_one_property_without_rewriting_unrelated_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("harbor.md");
    fs::write(
        &path,
        concat!(
            "\u{feff}---\r\n",
            "node_id: 42\r\n",
            "# retained comment\r\n",
            "related:\r\n",
            "  - \"[[Old Signal]]\"\r\n",
            "title: Harbor\r\n",
            "---\r\n\r\n",
            "# Harbor\r\n",
        ),
    )
    .unwrap();
    let mut file = MarkdownFile::open(&path).unwrap();

    file.set_property("related", &["[[Signal Station]]"])
        .unwrap();
    assert!(file.remove_property("title").unwrap());
    file.save().unwrap();

    assert_eq!(
        fs::read_to_string(path).unwrap(),
        concat!(
            "\u{feff}---\r\n",
            "node_id: 42\r\n",
            "# retained comment\r\n",
            "related: [\"[[Signal Station]]\"]\r\n",
            "---\r\n\r\n",
            "# Harbor\r\n",
        )
    );
}

#[test]
fn create_new_refuses_to_replace_an_existing_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("harbor.md");
    fs::write(&path, "keep me\n").unwrap();
    let metadata = Metadata {
        node_id: 42,
        title: "Harbor".to_string(),
        related: Vec::new(),
    };

    let error = MarkdownFile::create_new(&path, &metadata, "replacement\n").unwrap_err();

    assert!(error.is_already_exists());
    assert_eq!(fs::read_to_string(path).unwrap(), "keep me\n");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn adds_frontmatter_to_a_body_only_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("harbor.md");
    fs::write(&path, "# Harbor\n").unwrap();
    let mut file = MarkdownFile::open(&path).unwrap();

    file.set_property("node_id", &42).unwrap();
    file.save().unwrap();

    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "---\nnode_id: 42\n---\n\n# Harbor\n"
    );
}

#[test]
fn replaces_an_unindented_yaml_sequence_as_one_property() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("harbor.md");
    fs::write(
        &path,
        "---\nrelated:\n- \"[[Old Signal]]\"\ncustom label: retained\ntitle: Harbor\n---\n\n# Harbor\n",
    )
    .unwrap();
    let mut file = MarkdownFile::open(&path).unwrap();

    file.set_property("related", &["[[Signal Station]]"])
        .unwrap();
    file.save().unwrap();

    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "---\nrelated: [\"[[Signal Station]]\"]\ncustom label: retained\ntitle: Harbor\n---\n\n# Harbor\n"
    );
}

#[test]
fn rejects_ambiguous_property_updates() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("harbor.md");
    fs::write(&path, "---\nnode_id: 41\nnode_id: 42\n---\n\n# Harbor\n").unwrap();
    let mut file = MarkdownFile::open(&path).unwrap();

    let error = file.set_property("node_id", &43).unwrap_err();

    assert!(matches!(
        error,
        MarkdownFileError::DuplicateProperty { ref name, .. } if name == "node_id"
    ));
}
