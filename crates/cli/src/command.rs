//! Defines root parsing and top-level engine selection.

use clap::{Parser, Subcommand};

use crate::engines::{handoff, note, pending_work, project};

/// Manages pending work, handoffs, and project notes across configured repositories.
#[derive(Parser, Debug)]
#[command(
    name = "pwf",
    version,
    about,
    long_about = None,
    styles = clap_cargo::style::CLAP_STYLING
)]
pub struct Cli {
    #[command(subcommand)]
    pub engine: Engine,
}

#[derive(Subcommand, Debug)]
pub enum Engine {
    /// Manages registered projects.
    Project(project::Arguments),
    #[command(flatten)]
    PendingWork(pending_work::Command),
    /// Per-repo handoff ledgers (resume notes between sessions).
    Handoff {
        #[command(subcommand)]
        command: handoff::Command,
    },
    /// One-liner project notes: `pwf note [ls|add <msg>|remove <id>] <proj>`.
    Note(note::Arguments),
}

/// Parses post-binary arguments after preserving the accepted normalization pass.
pub fn parse_argv(argv: Vec<String>) -> Result<Cli, clap::Error> {
    let normalized = if argv
        .first()
        .is_some_and(|token| token.eq_ignore_ascii_case("project"))
    {
        argv
    } else {
        crate::preprocess::normalize(argv)
    };
    Cli::try_parse_from(std::iter::once("pwf".to_string()).chain(normalized))
}

/// Renders help scoped to the project command.
pub fn project_help() -> String {
    let mut help = Cli::try_parse_from(["pwf", "project", "--help"])
        .expect_err("project help exits clap parsing")
        .render()
        .to_string();
    if help.ends_with('\n') {
        help.pop();
    }
    help
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;
    use crate::engines::project;

    fn parse(tokens: &[&str]) -> Cli {
        parse_argv(tokens.iter().map(|token| (*token).to_string()).collect()).expect("parse")
    }

    fn pending_work(tokens: &[&str]) -> pending_work::Command {
        let Engine::PendingWork(command) = parse(tokens).engine else {
            panic!("expected pending-work command");
        };
        command
    }

    fn project(tokens: &[&str]) -> project::Arguments {
        let Engine::Project(arguments) = parse(tokens).engine else {
            panic!("expected project command");
        };
        arguments
    }

    #[test]
    fn cli_tree_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn project_list_parses_into_the_typed_leaf() {
        assert!(matches!(
            project(&["project", "ls"]).command,
            Some(project::Command::List(_))
        ));
    }

    #[test]
    fn project_get_parses_into_the_typed_leaf() {
        let Some(project::Command::Get(arguments)) = project(&["project", "get", "pwf"]).command
        else {
            panic!("expected project get");
        };

        assert_eq!(arguments.id.as_ref(), "PWF");
    }

    #[test]
    fn project_add_parses_into_the_typed_leaf() {
        let payload = r#"{
            "id": "pwf",
            "title": "pwf",
            "source": {"value": "/work/pwf"},
            "tasks": {"kind": "directory", "path": "/notes/pwf"}
        }"#;
        let Some(project::Command::Add(arguments)) =
            project(&["project", "add", "--kind", "directory", payload]).command
        else {
            panic!("expected project add");
        };

        assert_eq!(arguments.kind, project::add::SourceKind::Directory);
        assert_eq!(arguments.payload.0.id.as_ref(), "PWF");
        assert_eq!(arguments.payload.0.title.as_ref(), "pwf");
        assert_eq!(arguments.payload.0.source.value().as_ref(), "/work/pwf");
        assert_eq!(arguments.payload.0.tasks.path().as_ref(), "/notes/pwf");
    }

    #[test]
    fn project_pause_and_resume_parse_into_typed_leaves() {
        let Some(project::Command::Pause(arguments)) =
            project(&["project", "pause", "arc"]).command
        else {
            panic!("expected project pause");
        };
        assert_eq!(arguments.id.as_ref(), "ARC");

        let Some(project::Command::Resume(arguments)) =
            project(&["project", "resume", "arc"]).command
        else {
            panic!("expected project resume");
        };
        assert_eq!(arguments.id.as_ref(), "ARC");
    }

    #[test]
    fn add_parses_into_the_typed_pending_work_leaf() {
        let pending_work::Command::Add(arguments) = pending_work(&[
            "add",
            "pwf",
            "keep",
            "typed",
            "--title",
            "Typed CLI",
            "--tag",
            "architecture",
            "--effort",
            "3",
            "--date",
            "2026-07-20",
        ]) else {
            panic!("expected add");
        };
        assert_eq!(arguments.project.as_deref(), Some("pwf"));
        assert_eq!(arguments.prompt, ["keep", "typed"]);
        assert_eq!(arguments.title.as_deref(), Some("Typed CLI"));
        assert_eq!(arguments.tag, ["architecture"]);
        assert_eq!(arguments.effort, Some(3));
        assert_eq!(arguments.common.date.as_deref(), Some("2026-07-20"));
    }

    #[test]
    fn list_parses_into_the_typed_pending_work_leaf() {
        let pending_work::Command::List(arguments) = pending_work(&[
            "list",
            "--project",
            "pwf",
            "--long",
            "--all",
            "-n",
            "2",
            "--effort",
            "3",
            "--tag",
            "rust",
            "-o",
            "project-id:desc",
            "--status",
            "done",
        ]) else {
            panic!("expected list");
        };
        assert_eq!(arguments.project.as_deref(), Some("pwf"));
        assert!(arguments.long && arguments.all);
        assert_eq!(arguments.number, Some(2));
        assert_eq!(arguments.effort, Some(3));
        assert_eq!(arguments.tag, ["rust"]);
        assert_eq!(
            arguments.order,
            Some(pwf_application::pending_work::OrderSpec {
                field: pwf_application::pending_work::OrderField::ProjectId,
                direction: pwf_application::pending_work::OrderDirection::Desc,
            })
        );
        assert_eq!(
            arguments.status.expect("explicit --status").filter(),
            pwf_domain::pending_work::WorkItemStatusFilter::Exact(
                pwf_domain::pending_work::WorkItemStatus::Done
            )
        );
    }

    #[test]
    fn lifecycle_verbs_parse_into_typed_pending_work_leaves() {
        let pending_work::Command::Done(done) = pending_work(&[
            "done",
            "cfg",
            "57",
            "--report",
            "finished",
            "--commits",
            "a..b",
            "--review",
        ]) else {
            panic!("expected done");
        };
        assert_eq!(done.identifier.canonical().as_deref(), Some("CFG-0057"));
        assert_eq!(done.report.as_deref(), Some("finished"));
        assert_eq!(done.commits, ["a..b"]);
        assert!(done.review);

        let pending_work::Command::Cancel(cancel) = pending_work(&[
            "cancel",
            "PWF-0002",
            "--report",
            "blocked",
            "--commits",
            "b..c",
        ]) else {
            panic!("expected cancel");
        };
        assert_eq!(cancel.identifier.canonical().as_deref(), Some("PWF-0002"));
        assert_eq!(cancel.report.as_deref(), Some("blocked"));

        let pending_work::Command::Reopen(reopen) = pending_work(&["reopen", "pwf3"]) else {
            panic!("expected reopen");
        };
        assert_eq!(reopen.identifier.canonical().as_deref(), Some("PWF-0003"));
    }

    #[test]
    fn update_parses_into_the_typed_pending_work_leaf() {
        let pending_work::Command::Update(arguments) = pending_work(&[
            "update",
            "PWF-0001",
            "--title",
            "new",
            "--prereq",
            "PWF-0002",
            "--tag",
            "rust",
            "--commits",
            "a..b",
            "--append-report",
            "done",
            "--effort",
            "4",
        ]) else {
            panic!("expected update");
        };
        assert_eq!(
            arguments.identifier.canonical().as_deref(),
            Some("PWF-0001")
        );
        assert_eq!(arguments.title.as_deref(), Some("new"));
        assert_eq!(arguments.prereq, ["PWF-0002"]);
        assert_eq!(arguments.tag, ["rust"]);
        assert_eq!(arguments.commits, ["a..b"]);
        assert_eq!(arguments.append_report.as_deref(), Some("done"));
        assert_eq!(arguments.effort, Some(4));
    }

    #[test]
    fn show_remove_and_verify_parse_into_typed_pending_work_leaves() {
        let pending_work::Command::Show(show) = pending_work(&["s", "pwf1", "--path"]) else {
            panic!("expected show");
        };
        assert_eq!(show.identifier.raw(), Some("pwf1"));
        assert!(show.path);

        let pending_work::Command::Remove(remove) = pending_work(&["remove", "PWF-0002", "-y"])
        else {
            panic!("expected remove");
        };
        assert_eq!(remove.identifier.canonical().as_deref(), Some("PWF-0002"));
        assert!(remove.assume_yes);

        let pending_work::Command::Verify(verify) =
            pending_work(&["verify", "PWF-0003", "-a", "codex", "-m", "gpt-5"])
        else {
            panic!("expected verify");
        };
        assert_eq!(verify.identifier.canonical().as_deref(), Some("PWF-0003"));
        assert_eq!(verify.agent, pending_work::common::AgentChoice::Codex);
        assert_eq!(verify.model.as_deref(), Some("gpt-5"));
    }

    #[test]
    fn session_and_route_parse_into_typed_pending_work_leaves() {
        let pending_work::Command::Session(session) = pending_work(&[
            "session",
            "PWF-0001",
            "--color",
            "always",
            "--yes",
            "--inline",
            "--worktree",
            "--auto",
            "--agent",
            "codex",
            "-a",
            "context",
            "-m",
            "gpt-5",
        ]) else {
            panic!("expected session");
        };
        assert_eq!(session.identifier.canonical().as_deref(), Some("PWF-0001"));
        assert_eq!(session.color, pending_work::session::ColorChoice::Always);
        assert!(session.assume_yes && session.inline && session.worktree && session.autonomous);
        assert_eq!(session.agent, pending_work::common::AgentChoice::Codex);
        assert_eq!(session.append.as_deref(), Some("context"));

        let pending_work::Command::Route(route) = pending_work(&[
            "pwf",
            "--long",
            "--section",
            "future",
            "-n",
            "3",
            "--status",
            "all",
        ]) else {
            panic!("expected route");
        };
        assert_eq!(route.words, ["pwf"]);
        assert!(route.long);
        assert!(matches!(
            route.section,
            Some(pending_work::common::SectionChoice::Future)
        ));
        assert_eq!(route.number, Some(3));
        assert_eq!(
            route.status.expect("explicit --status").filter(),
            pwf_domain::pending_work::WorkItemStatusFilter::All
        );
    }

    #[test]
    fn compatibility_routes_resolve_to_typed_pending_work_leaves() {
        let pending_work::Command::Route(project_route) = pending_work(&[
            "pwf",
            "--long",
            "--section",
            "future",
            "-n",
            "3",
            "--status",
            "all",
        ]) else {
            panic!("expected project route");
        };
        let pending_work::route::ResolvedCommand::List(list) =
            pending_work::route::resolve(&project_route)
        else {
            panic!("expected routed list");
        };
        assert_eq!(list.project.as_deref(), Some("pwf"));
        assert!(list.long);
        assert!(matches!(
            list.section,
            Some(pending_work::common::SectionChoice::Future)
        ));
        assert_eq!(list.number, Some(3));
        assert_eq!(list.order, None);
        assert_eq!(
            list.mode,
            pwf_application::pending_work::ListMode::ProjectRoute
        );
        assert_eq!(
            list.status.expect("explicit --status").filter(),
            pwf_domain::pending_work::WorkItemStatusFilter::All
        );

        let pending_work::Command::Route(verify_route) =
            pending_work(&["route", "verify", "cfg57"])
        else {
            panic!("expected verify route");
        };
        let pending_work::route::ResolvedCommand::Verify(verify) =
            pending_work::route::resolve(&verify_route)
        else {
            panic!("expected routed verify");
        };
        assert_eq!(verify.identifier.canonical().as_deref(), Some("CFG-0057"));
        assert_eq!(verify.agent, pending_work::common::AgentChoice::Claude);
        assert_eq!(verify.model, None);
    }

    #[test]
    fn positional_and_id_flag_conflict_is_rejected() {
        let error = parse_argv(
            ["done", "GLP-0001", "--id", "GLP-0002"]
                .map(str::to_string)
                .to_vec(),
        )
        .expect_err("positional and --id must conflict");

        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
        assert!(error.to_string().contains("cannot be used with"));
    }

    #[test]
    fn project_first_note_verb_is_rejected() {
        let error = parse_argv(["note", "pwf", "add", "x"].map(str::to_string).to_vec())
            .expect_err("note verbs must be verb-first");

        assert!(error.to_string().contains("unexpected argument 'add'"));
    }

    #[test]
    fn handoff_verbs_parse_into_typed_engine_leaves() {
        let Engine::Handoff {
            command: handoff::Command::Add(add),
        } = parse(&[
            "handoff",
            "add",
            "--title",
            "checkpoint",
            "--slug",
            "checkpoint",
            "--repo-root",
            "/tmp/repo",
            "--date",
            "2026-07-20",
        ])
        .engine
        else {
            panic!("expected handoff add");
        };
        assert_eq!(add.title.as_deref(), Some("checkpoint"));
        assert_eq!(add.slug.as_deref(), Some("checkpoint"));
        assert_eq!(add.common.repo_root.as_deref(), Some("/tmp/repo"));

        let Engine::Handoff {
            command: handoff::Command::List(list),
        } = parse(&["handoff", "list", "--repo-root", "/tmp/repo"]).engine
        else {
            panic!("expected handoff list");
        };
        assert_eq!(list.common.repo_root.as_deref(), Some("/tmp/repo"));
    }
}
