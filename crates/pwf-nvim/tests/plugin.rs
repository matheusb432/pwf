#![cfg(test)]

//! Exercises the Lua plugin and the release `pwf-nvim` child in headless Neovim against a real
//! `pwf-server`. The fzf-lua adapter is verified manually; these tests drive the modules below it.

#[path = "plugin/support.rs"]
mod support;

use std::process::{Command, Stdio};

use anyhow::Context as _;
use serde_json::{Value, json};
use support::{Fixture, release_binary};

fn sorted_entries(listing: &Value) -> anyhow::Result<Vec<String>> {
    let mut entries = listing["value"]["entries"]
        .as_array()
        .with_context(|| format!("listing has no entries: {listing}"))?
        .iter()
        .map(|entry| entry.as_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()
        .with_context(|| format!("listing has a non-string entry: {listing}"))?;
    entries.sort();
    Ok(entries)
}

#[test]
fn lists_the_working_directory_project_with_the_selected_statuses() -> anyhow::Result<()> {
    let fixture = Fixture::with_server()?;
    let alpha = fixture.add_project("ALP", "alpha")?;
    let beta = fixture.add_project("BET", "beta")?;
    let open = fixture.add_task(&alpha, "open alpha task")?;
    let closed = fixture.add_task(&alpha, "closed alpha task")?;
    fixture.pwf(&["task", "done", &closed])?;
    fixture.add_task(&beta, "beta task")?;

    let result = fixture.run_lua(
        &alpha.source,
        r#"
        local tasks = require("pwf.tasks")
        local function list(status)
          return await(function(done)
            tasks.list({ context_paths = tasks.context_paths(), status = status, limit = 100 }, done)
          end)
        end
        return { active = list("active"), all = list("all") }
        "#,
    )?;

    assert_eq!(
        result["active"]["value"],
        json!({"project": "ALP", "entries": [format!("{open} open alpha task")], "hidden": 0})
    );
    assert_eq!(
        sorted_entries(&result["all"])?,
        [
            format!("{open} open alpha task"),
            format!("{closed} closed alpha task [done]"),
        ]
    );
    Ok(())
}

#[test]
fn opened_task_file_scopes_the_picker_to_its_project() -> anyhow::Result<()> {
    let fixture = Fixture::with_server()?;
    let alpha = fixture.add_project("ALP", "alpha")?;
    let beta = fixture.add_project("BET", "beta")?;
    let alpha_task = fixture.add_task(&alpha, "alpha task")?;
    let beta_task = fixture.add_task(&beta, "beta task")?;

    let result = fixture.run_lua(
        &alpha.source,
        &r#"
        local tasks = require("pwf.tasks")
        local opened = await(function(done) tasks.open("TASK_ID", "edit", done) end)
        local scoped = await(function(done)
          tasks.list({ context_paths = tasks.context_paths(), status = "active", limit = 100 }, done)
        end)
        local global = await(function(done) tasks.list({ status = "active", limit = 100 }, done) end)
        return { opened = opened, buffer = vim.api.nvim_buf_get_name(0), scoped = scoped, global = global }
        "#
        .replace("TASK_ID", &beta_task),
    )?;

    let path = fixture.task_path(&beta_task)?;
    assert_eq!(result["opened"]["value"]["path"], json!(path));
    assert_eq!(result["buffer"], json!(path));
    assert_eq!(
        result["scoped"]["value"],
        json!({"project": "BET", "entries": [format!("{beta_task} beta task")], "hidden": 0})
    );
    assert_eq!(
        sorted_entries(&result["global"])?,
        [
            format!("{alpha_task} alpha task"),
            format!("{beta_task} beta task"),
        ]
    );
    assert_eq!(result["global"]["value"].get("project"), None);
    Ok(())
}

#[test]
fn raising_the_limit_retrieves_hidden_tasks() -> anyhow::Result<()> {
    let fixture = Fixture::with_server()?;
    let alpha = fixture.add_project("ALP", "alpha")?;
    for title in ["first", "second", "third"] {
        fixture.add_task(&alpha, title)?;
    }

    let result = fixture.run_lua(
        &alpha.source,
        r#"
        local tasks = require("pwf.tasks")
        local function list(limit)
          return await(function(done) tasks.list({ status = "active", limit = limit }, done) end)
        end
        return { capped = list(2), raised = list(4) }
        "#,
    )?;

    assert_eq!(sorted_entries(&result["capped"])?.len(), 2);
    assert_eq!(result["capped"]["value"]["hidden"], json!(1));
    assert_eq!(sorted_entries(&result["raised"])?.len(), 3);
    assert_eq!(result["raised"]["value"]["hidden"], json!(0));
    Ok(())
}

#[test]
fn reference_is_inserted_at_the_captured_cursor() -> anyhow::Result<()> {
    let fixture = Fixture::without_server()?;

    let result = fixture.run_lua(
        fixture.directory(),
        r#"
        local reference = require("pwf.reference")
        vim.api.nvim_buf_set_lines(0, 0, -1, true, { "see  here", "end:" })
        vim.api.nvim_win_set_cursor(0, { 1, 4 })
        local normal_target = reference.target()
        assert(reference.insert("ALP-0001", normal_target))
        local normal_cursor = vim.api.nvim_win_get_cursor(0)
        local insert_target = vim.tbl_extend("force", normal_target, { col = 4, insert_mode = true })
        assert(reference.insert("ALP-0002", insert_target))
        local insert_cursor = vim.api.nvim_win_get_cursor(0)
        local end_target = vim.tbl_extend("force", normal_target, { row = 2, col = 4, insert_mode = true })
        assert(reference.insert("ALP-0003", end_target))
        return {
          lines = vim.api.nvim_buf_get_lines(0, 0, -1, true),
          normal_cursor = normal_cursor,
          insert_cursor = insert_cursor,
        }
        "#,
    )?;

    assert_eq!(
        result["lines"],
        json!(["see [[ALP-0002]][[ALP-0001]] here", "end:[[ALP-0003]]"])
    );
    assert_eq!(result["normal_cursor"], json!([1, 15]));
    assert_eq!(result["insert_cursor"], json!([1, 16]));
    Ok(())
}

#[test]
fn unavailable_server_fails_requests_without_blocking_the_caller() -> anyhow::Result<()> {
    let fixture = Fixture::without_server()?;

    let result = fixture.run_lua(
        fixture.directory(),
        r#"
        local order = {}
        local outcome = await(function(done)
          require("pwf.tasks").list({ status = "active", limit = 10 }, function(err, value)
            table.insert(order, "callback")
            done(err, value)
          end)
          table.insert(order, "returned")
        end)
        return { order = order, err = outcome.err }
        "#,
    )?;

    assert_eq!(result["order"], json!(["returned", "callback"]));
    let err = result["err"].as_str().unwrap_or_default();
    assert!(err.starts_with("Cannot connect to pwf-server."), "{err}");
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
        stderr.contains("serves plugin protocol 1, but the plugin requested 999"),
        "{stderr}"
    );
    Ok(())
}
