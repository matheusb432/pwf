#![cfg(test)]

//! Exercises the Lua file getter and its release child against an isolated pwf-server.

#[path = "plugin/support.rs"]
mod support;

use std::{
    fs,
    process::{Command, Stdio},
};

use anyhow::Context as _;
use serde_json::{Value, json};
use support::{Fixture, release_binary};

fn summaries(listing: &Value) -> anyhow::Result<Vec<String>> {
    let mut entries = listing["value"]["entries"]
        .as_array()
        .with_context(|| format!("listing has no entries: {listing}"))?
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|entry| entry.splitn(3, '\t').nth(2))
        .filter(|entry| !entry.starts_with("[content]"))
        .map(str::to_string)
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries)
}

#[test]
fn combines_tasks_and_notes_with_project_and_status_filters() -> anyhow::Result<()> {
    let fixture = Fixture::with_server()?;
    let alpha = fixture.add_project("ALP", "alpha")?;
    let beta = fixture.add_project("BET", "beta")?;
    let paused = fixture.add_project("PAU", "paused")?;
    let active = fixture.add_task(&alpha, "active alpha")?;
    let done = fixture.add_task(&alpha, "done alpha")?;
    let backlog = fixture.add_task(&alpha, "backlog alpha")?;
    let cancelled = fixture.add_task(&alpha, "cancelled alpha")?;
    fixture.pwf(&["task", "done", &done])?;
    fixture.pwf(&["task", "backlog", &backlog])?;
    fixture.pwf(&["task", "cancel", &cancelled, "--report", "no longer needed"])?;
    fixture.add_task(&beta, "beta task")?;
    fixture.add_task(&paused, "paused task")?;
    for id in ["ALP", "BET", "PAU"] {
        fixture.pwf(&["note", "add", id, "project note"])?;
    }
    fixture.pwf(&["project", "pause", "PAU"])?;
    let result = fixture.run_lua(&alpha.source, r#"
        local records = require("pwf.records")
        local result = {}
        for _, status in ipairs({ "active", "done", "backlog", "cancelled", "all" }) do
          result[status] = await(function(done)
            records.list({context_paths = records.context_paths(), status = status, limit = 100}, done)
          end)
        end
        result.global = await(function(done) records.list({status = "all", limit = 100}, done) end)
        result.fallback = await(function(done)
          records.list({context_paths = { "/not-a-project" }, status = "all", limit = 100}, done)
        end)
        return result
    "#)?;
    for (status, id, title, suffix) in [
        ("active", &active, "active", ""),
        ("done", &done, "done", " [done]"),
        ("backlog", &backlog, "backlog", " [backlog]"),
        ("cancelled", &cancelled, "cancelled", " [cancelled]"),
    ] {
        assert_eq!(
            summaries(&result[status])?,
            [
                "[note] ALP-NOTE-0001 project note".to_string(),
                format!("[task] {id} {title} alpha{suffix}")
            ]
        );
        assert_eq!(result[status]["value"]["project"], "ALP");
    }
    assert_eq!(summaries(&result["all"])?.len(), 5);
    assert_eq!(summaries(&result["global"])?.len(), 7);
    assert_eq!(result["global"]["value"], result["fallback"]["value"]);
    assert_eq!(result["global"]["value"].get("project"), None);
    Ok(())
}

#[test]
fn record_cap_counts_hidden_tasks_and_notes() -> anyhow::Result<()> {
    let fixture = Fixture::with_server()?;
    let alpha = fixture.add_project("ALP", "alpha")?;
    for title in ["first", "second", "third"] {
        fixture.add_task(&alpha, title)?;
        fixture.pwf(&["note", "add", "ALP", title])?;
    }
    let result = fixture.run_lua(&alpha.source, r#"
        local records = require("pwf.records")
        local function list(limit) return await(function(done) records.list({status = "active", limit = limit}, done) end) end
        return { capped = list(2), raised = list(4) }
    "#)?;
    assert_eq!(summaries(&result["capped"])?.len(), 4);
    assert_eq!(result["capped"]["value"]["hidden"], 2);
    assert_eq!(summaries(&result["raised"])?.len(), 6);
    assert_eq!(result["raised"]["value"]["hidden"], 0);
    Ok(())
}

#[test]
fn content_rows_share_the_listing_and_open_the_real_file_at_the_matching_line() -> anyhow::Result<()>
{
    let fixture = Fixture::with_server()?;
    let alpha = fixture.add_project("ALP", "alpha space")?;
    fixture.add_project("BET", "beta")?;
    let active = fixture.add_task(&alpha, "frontmatter-only")?;
    fixture.pwf(&["note", "add", "ALP", "note / note-body-needle"])?;
    let original = fixture.task_path(&active)?;
    let directory = original.parent().context("task has no parent")?;
    let renamed = directory.join("renamed 'task.md");
    let source = format!(
        "{}\npré body-only-needle\nshell $(pwd) ' -- literal\n",
        fs::read_to_string(&original)?
    )
    .replace('\n', "\r\n");
    fs::write(&renamed, &source)?;
    fs::remove_file(&original)?;
    for name in [
        "ALP-9999.plan.md",
        "alpha space.md",
        "alpha space.backup.md",
    ] {
        fs::write(directory.join(name), "excluded-body-needle\n")?;
    }
    fs::create_dir(directory.join("nested"))?;
    fs::write(
        directory.join("nested/ALP-NOTE-9999.md"),
        "excluded-body-needle\n",
    )?;
    fs::write(directory.join(".gitignore"), "*.md\n")?;
    let result = fixture.run_lua(&alpha.source, include_str!("plugin/records.lua"))?;
    let entries = result["listing"]["value"]["entries"]
        .as_array()
        .context("missing entries")?;
    let line = source
        .lines()
        .position(|line| line == "pré body-only-needle")
        .unwrap()
        + 1;
    assert!(entries.contains(&json!(format!(
        "{active}\t{line}\t[content] {active}:{line} pré body-only-needle"
    ))));
    assert!(entries.iter().any(|entry| {
        entry
            .as_str()
            .unwrap()
            .contains("[note] ALP-NOTE-0001 note")
    }));
    assert!(entries.iter().any(
        |entry| entry.as_str().unwrap().contains("[content] ALP-NOTE-0001:")
            && entry.as_str().unwrap().contains("note-body-needle")
    ));
    assert!(entries.iter().any(|entry| {
        entry
            .as_str()
            .unwrap()
            .contains("shell $(pwd) ' -- literal")
    }));
    assert!(
        !entries
            .iter()
            .any(|entry| entry.as_str().unwrap().contains("excluded-body-needle"))
    );
    assert!(
        !entries
            .iter()
            .any(|entry| entry.as_str().unwrap().contains("[content]")
                && entry.as_str().unwrap().contains("frontmatter-only"))
    );
    assert_eq!(result["opened"], json!(renamed));
    assert_eq!(result["cursor"], json!([line, 0]));
    assert_eq!(result["scoped"]["value"]["project"], "ALP");
    assert!(result["protected"].as_str().unwrap().contains("E37"));
    assert_eq!(result["unsaved"], "unsaved text");
    assert!(
        result["missing"]
            .as_str()
            .unwrap()
            .contains("no longer available")
    );
    assert_eq!(result["cwd_unchanged"], true);
    assert_eq!(fs::read_to_string(renamed)?, source);
    Ok(())
}

#[test]
fn removed_mutations_and_reference_operations_are_not_exposed() -> anyhow::Result<()> {
    let fixture = Fixture::without_server()?;
    let result = fixture.run_lua(fixture.directory(), r#"
        vim.cmd("runtime plugin/pwf.lua")
        local client = require("pwf.client")
        local result = {}
        for _, operation in ipairs({ "create_task", "task_template", "update_task", "task_metadata", "list_tags", "task_action", "add_note", "list_projects" }) do
          result[operation] = await(function(done) client.request(operation, {}, done) end).err
        end
        local pwf = require("pwf")
        for _, method in ipairs({"new_task", "new_note", "actions", "complete", "insert_reference"}) do assert(pwf[method] == nil) end
        for _, mapping in ipairs({"new", "new-note", "actions", "done", "insert-reference"}) do
          assert(vim.fn.maparg("<Plug>(pwf-" .. mapping .. ")", "n") == "")
        end
        assert(pwf.notes == pwf.tasks)
        return result
    "#)?;
    for error in result.as_object().unwrap().values() {
        assert!(
            error.as_str().unwrap().contains("unknown operation"),
            "{error}"
        );
    }
    Ok(())
}

#[test]
fn unavailable_server_fails_without_blocking_the_caller() -> anyhow::Result<()> {
    let fixture = Fixture::without_server()?;
    let result = fixture.run_lua(
        fixture.directory(),
        r#"
        local order = {}
        local outcome = await(function(done)
          require("pwf.records").list({ status = "active", limit = 10 }, function(err, value)
            table.insert(order, "callback"); done(err, value)
          end)
          table.insert(order, "returned")
        end)
        return { order = order, err = outcome.err }
    "#,
    )?;
    assert_eq!(result["order"], json!(["returned", "callback"]));
    let error = result["err"].as_str().unwrap_or_default();
    assert!(
        error.starts_with("Cannot connect to pwf-server."),
        "{error}"
    );
    Ok(())
}

#[test]
fn child_rejects_a_different_plugin_protocol() -> anyhow::Result<()> {
    let output = Command::new(release_binary("pwf-nvim"))
        .args(["--protocol", "999"])
        .stdin(Stdio::null())
        .output()?;
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("serves plugin protocol 5, but the plugin requested 999"),
        "{stderr}"
    );
    Ok(())
}
