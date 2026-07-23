use std::{fs, path::Path};

fn repo_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(path)
}

fn source(path: &str) -> String {
    fs::read_to_string(repo_path(path)).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

fn occurrence_count(content: &str, needle: &str) -> usize {
    content.match_indices(needle).count()
}

#[test]
fn root_parser_contains_no_leaf_note_model_or_execution() {
    let root = source("crates/cli/src/command.rs");

    for forbidden in [
        "struct NoteArguments",
        "struct NoteCommonArguments",
        "enum NoteAction",
        "NoteVerb",
        "into_command",
        "pwf_note::run",
    ] {
        assert!(
            !root.contains(forbidden),
            "root parser retains note leaf concern `{forbidden}`"
        );
    }
    assert!(root.contains("Note(note::Arguments)"));
}

#[test]
fn compatibility_router_only_converts_tokens_to_typed_leaves() {
    let router = source("crates/cli/src/engines/pending_work/route.rs");

    for forbidden in [
        "pwf_application",
        "pwf_infra",
        "load_configuration",
        "ObsidianStore",
        "ProjectRegistry",
        "::execute(",
        "render_",
    ] {
        assert!(
            !router.contains(forbidden),
            "compatibility router retains effect or presentation concern `{forbidden}`"
        );
    }
    assert!(router.contains("ResolvedCommand"));
    assert!(router.contains("fn resolve("));
}

#[test]
fn pending_work_dispatch_matches_command_once_without_outcome_bridge() {
    let dispatch = source("crates/cli/src/engines/pending_work.rs");

    assert_eq!(occurrence_count(&dispatch, "match command"), 1);
    assert!(dispatch.contains("route::resolve"));
    for forbidden in [
        "EngineOutcome",
        "MutationOutcome",
        "into_raw_text",
        "render_outcome_confirmation",
    ] {
        assert!(
            !dispatch.contains(forbidden),
            "pending-work dispatch retains compatibility bridge `{forbidden}`"
        );
    }
    assert!(
        !repo_path("crates/cli/src/engines/pending_work/render/outcome.rs").exists(),
        "generic mutation/raw outcome bridge must be deleted"
    );
}

#[test]
fn executable_pending_work_leaves_compose_and_execute_locally() {
    for leaf in [
        "add", "list", "done", "cancel", "reopen", "update", "remove", "show", "session", "verify",
    ] {
        let path = format!("crates/cli/src/engines/pending_work/{leaf}.rs");
        let content = source(&path);
        assert!(
            content.contains("ObsidianStore::new"),
            "{path} must compose its concrete store"
        );
        assert!(
            content.contains("ProjectRegistry::new"),
            "{path} must compose its project registry"
        );
        assert_eq!(
            occurrence_count(&content, "::execute("),
            1,
            "{path} must call one application operation"
        );
        assert!(content.contains("fn run("), "{path} must own execution");
    }

    let common = source("crates/cli/src/engines/pending_work/common.rs");
    for forbidden in [
        "pwf_infra",
        "ObsidianStore",
        "fn store(",
        "fn project_registry(",
    ] {
        assert!(
            !common.contains(forbidden),
            "pending-work common retains composition facade `{forbidden}`"
        );
    }
}

#[test]
fn handoff_composes_without_importing_pending_work_cli_ownership() {
    let add = source("crates/cli/src/engines/handoff/add.rs");
    let list = source("crates/cli/src/engines/handoff/list.rs");

    assert!(add.contains("ObsidianStore::new"));
    assert!(add.contains("ProjectRegistry::new"));
    assert!(!add.contains("engines::pending_work"));
    assert_eq!(occurrence_count(&add, "::execute("), 1);
    assert!(list.contains("ObsidianStore::new"));
    assert_eq!(occurrence_count(&list, "::execute("), 1);
}

#[test]
fn sibling_verbs_do_not_import_presentation_from_other_verbs() {
    for (leaf, forbidden) in [
        ("cancel", ["    add::", "    done::"]),
        ("done", ["    add::", "    done::"]),
        ("update", ["    add::", "    done::"]),
    ] {
        let path = format!("crates/cli/src/engines/pending_work/{leaf}.rs");
        let content = source(&path);
        let sibling_imports = content
            .split_once("use super::{")
            .and_then(|(_, rest)| rest.split_once("\n};"))
            .map_or("", |(imports, _)| imports);
        for import in forbidden {
            assert!(
                !sibling_imports.contains(import),
                "{path} imports sibling presentation through `{import}`"
            );
        }
    }
}
