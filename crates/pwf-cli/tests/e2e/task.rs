use std::{fs, path::Path};

use assert_cmd::prelude::OutputAssertExt as _;
use serde_json::{Value, json};

use crate::support::{ManagedProject, ProjectFixture, project_id, task_id, task_json};

fn write_task_note(path: &Path, id: &str, project: &str, title: &str) -> std::io::Result<()> {
    fs::write(
        path,
        format!(
            "---\nid: {id}\nstatus: active\ntitle: {title}\nproject: {project}\ncreated_at: 2026-09-03T12:00:00Z\n---\n\n## Goals\n\n- exercise section listing\n"
        ),
    )
}

#[test]
fn task_list_filters_and_globally_groups_arbitrary_h2_sections() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let fixture = ProjectFixture::new()?;
    let alpha_tasks = directory.path().join("tasks/alpha");
    let beta_tasks = directory.path().join("tasks/beta");
    let alpha_project = directory.path().join("projects/alpha");
    let beta_project = directory.path().join("projects/beta");
    for path in [&alpha_tasks, &beta_tasks, &alpha_project, &beta_project] {
        fs::create_dir_all(path)?;
    }
    fs::write(
        alpha_tasks.join("alpha.md"),
        concat!(
            "---\nid: aaa\ntitle: alpha\n---\n\n",
            "- [ ] [[AAA-0001]]\n\n",
            "## Zulu\n- [ ] [[AAA-0002]]\n\n",
            "## Waiting on API\n- [ ] [[AAA-0003]]\n\n",
            "### Notes\n- [[AAA-NOTE-0001]]\n",
        ),
    )?;
    fs::write(
        beta_tasks.join("beta.md"),
        concat!(
            "---\nid: bbb\ntitle: beta\n---\n\n",
            "## waiting on api\n- [ ] [[BBB-0001]]\n\n",
            "## Blocked\n- [ ] [[BBB-0002]]\n",
        ),
    )?;
    for (directory, id, project, title) in [
        (&alpha_tasks, "AAA-0001", "alpha", "unsectioned"),
        (&alpha_tasks, "AAA-0002", "alpha", "zulu task"),
        (&alpha_tasks, "AAA-0003", "alpha", "waiting alpha"),
        (&beta_tasks, "BBB-0001", "beta", "waiting beta"),
        (&beta_tasks, "BBB-0002", "beta", "blocked task"),
    ] {
        write_task_note(&directory.join(format!("{id}.md")), id, project, title)?;
    }
    fixture.add(
        &project_id("AAA")?,
        "alpha",
        alpha_project.to_str().unwrap(),
        alpha_tasks.to_str().unwrap(),
    )?;
    fixture.add(
        &project_id("BBB")?,
        "beta",
        beta_project.to_str().unwrap(),
        beta_tasks.to_str().unwrap(),
    )?;

    let all = fixture.run(&["task", "list", "--all", "--order", "project-id"])?;
    assert!(
        all.status.success(),
        "{}",
        String::from_utf8_lossy(&all.stderr)
    );
    let all = String::from_utf8(all.stdout)?;
    let ordered_fragments = [
        "AAA-0001",
        "Blocked\n",
        "BBB-0002",
        "Waiting on API\n",
        "AAA-0003",
        "BBB-0001",
        "Zulu\n",
        "AAA-0002",
    ];
    let mut previous = 0;
    for fragment in ordered_fragments {
        let Some(offset) = all[previous..].find(fragment) else {
            anyhow::bail!("missing {fragment:?}:\n{all}");
        };
        let index = previous + offset;
        previous = index + fragment.len();
    }
    assert!(!all.contains("Other\n"), "{all}");
    assert_eq!(all.matches("Waiting on API\n").count(), 1, "{all}");

    let filtered = fixture.run(&[
        "task",
        "list",
        "--section",
        "WAITING ON API",
        "--order",
        "project-id",
    ])?;
    assert!(
        filtered.status.success(),
        "{}",
        String::from_utf8_lossy(&filtered.stderr)
    );
    let filtered = String::from_utf8(filtered.stdout)?;
    assert!(filtered.contains("AAA-0003"), "{filtered}");
    assert!(filtered.contains("BBB-0001"), "{filtered}");
    assert!(!filtered.contains("AAA-0001"), "{filtered}");
    assert!(!filtered.contains("AAA-0002"), "{filtered}");
    assert!(!filtered.contains("BBB-0002"), "{filtered}");
    Ok(())
}

#[test]
fn task_lifecycle_is_observable_through_json() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "--title",
            "blocker",
            "--goal",
            "blocking work",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "--title",
            "just done",
            "--goal",
            "complete the work",
            "--blocked-by",
            "FOO-0001",
            "--effort",
            "high",
            "--priority",
            "highest",
            "--tag",
            "cli",
            "--tag",
            "sqlite",
        ])
        .assert()
        .success();

    let active = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
    assert_eq!(active["id"], "FOO-0002");
    assert_eq!(active["project"], "foo-bar");
    assert_eq!(active["title"], "just done");
    assert_eq!(active["status"], "active");
    assert_eq!(active["tags"], json!(["cli", "sqlite"]));
    assert_eq!(active["effort"], "high");
    assert_eq!(active["priority"], "highest");
    assert_eq!(active["blocked_by"], json!(["FOO-0001"]));

    fixture
        .database
        .command()
        .args([
            "edit",
            "FOO-0002",
            "--prompt",
            "ship it / preserve the revised goal",
            "--remove-tags",
            "--add-tag",
            "rust",
            "--remove-blocked-by",
            "--remove-effort",
            "--priority",
            "medium",
        ])
        .assert()
        .success();
    let updated = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
    assert_eq!(updated["title"], "ship it");
    assert_eq!(updated["tags"], json!(["rust"]));
    assert_eq!(updated["effort"], Value::Null);
    assert_eq!(updated["priority"], "medium");
    assert_eq!(updated["blocked_by"], Value::Null);

    fixture
        .database
        .command()
        .args(["edit", "FOO-0002", "--remove-priority"])
        .assert()
        .success();
    let updated = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
    assert_eq!(updated["priority"], Value::Null);

    fixture
        .database
        .command()
        .args(["done", "FOO-0002", "--commits", "a..b"])
        .assert()
        .success();
    let done = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
    assert_eq!(done["status"], "done");
    assert!(done["completed"].as_str().is_some());
    assert_eq!(done["commits"], "a..b");

    fixture
        .database
        .command()
        .args(["reopen", "FOO-0002", "--yes"])
        .assert()
        .success();
    let reopened = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
    assert_eq!(reopened["status"], "active");
    assert_eq!(reopened["completed"], Value::Null);
    assert_eq!(reopened["commits"], Value::Null);
}
