use std::path::Path;

use syn::{ExprPath, File, Item, ItemFn, ItemUse, UseTree, Visibility};

use super::{SourceFinding, normalized_identifier};

const OPERATION_EXPORT_MESSAGE: &str = "Application operation files may expose `execute` and declared contract items, not other functions or re-exports.";
const DOMAIN_EFFECT_MESSAGE: &str =
    "Domain models must not import or invoke effectful capabilities.";
const LOGIC_EFFECT_MESSAGE: &str = "Application `logic.rs` modules must remain effect-free.";

#[derive(Clone, Copy)]
pub(super) struct Scope {
    operation_exports: bool,
    effect_message: Option<&'static str>,
}

impl Scope {
    pub(super) fn for_source(relative_path: &Path, source: &File) -> Self {
        let components = relative_path
            .components()
            .map(std::path::Component::as_os_str)
            .collect::<Vec<_>>();
        let domain_source = components
            .get(..3)
            .is_some_and(|prefix| prefix == ["crates", "domain", "src"]);
        let application_source = components
            .get(..3)
            .is_some_and(|prefix| prefix == ["crates", "application", "src"]);
        let application_path = application_source.then(|| &components[3..]);

        Self {
            operation_exports: application_path.is_some_and(is_operation_path)
                && file_declares_execute(source),
            effect_message: if domain_source {
                Some(DOMAIN_EFFECT_MESSAGE)
            } else if application_path.is_some_and(is_logic_path) {
                Some(LOGIC_EFFECT_MESSAGE)
            } else {
                None
            },
        }
    }

    pub(super) fn inspect_item_fn(self, item: &ItemFn, findings: &mut Vec<SourceFinding>) {
        if self.operation_exports
            && matches!(item.vis, Visibility::Public(_))
            && normalized_identifier(&item.sig.ident) != "execute"
        {
            findings.push(SourceFinding::new(
                item.sig.ident.span().start().line,
                OPERATION_EXPORT_MESSAGE,
            ));
        }
    }

    pub(super) fn inspect_item_use(self, item: &ItemUse, findings: &mut Vec<SourceFinding>) {
        let line = item.use_token.span.start().line;
        if self.operation_exports && matches!(item.vis, Visibility::Public(_)) {
            findings.push(SourceFinding::new(line, OPERATION_EXPORT_MESSAGE));
        }
        if let Some(message) = self.effect_message
            && use_tree_has_effect(&item.tree)
        {
            findings.push(SourceFinding::new(line, message));
        }
    }

    pub(super) fn inspect_expr_path(
        self,
        expression: &ExprPath,
        findings: &mut Vec<SourceFinding>,
    ) {
        self.inspect_path(&expression.path, findings);
    }

    pub(super) fn inspect_path(self, path: &syn::Path, findings: &mut Vec<SourceFinding>) {
        let Some(message) = self.effect_message else {
            return;
        };
        if path_has_effect(path)
            && let Some(root) = path.segments.first()
        {
            findings.push(SourceFinding::new(root.ident.span().start().line, message));
        }
    }
}

fn file_declares_execute(source: &File) -> bool {
    source.items.iter().any(|item| {
        let Item::Fn(function) = item else {
            return false;
        };
        matches!(function.vis, Visibility::Public(_))
            && normalized_identifier(&function.sig.ident) == "execute"
    })
}

fn is_operation_path(path: &[&std::ffi::OsStr]) -> bool {
    if path.len() < 2 {
        return false;
    }
    let Some(file_name) = path.last().and_then(|name| name.to_str()) else {
        return false;
    };
    let excluded_file = matches!(file_name, "dto.rs" | "model.rs" | "ports.rs" | "tests.rs")
        || file_name.ends_with("_tests.rs");
    let excluded_directory = path[..path.len() - 1]
        .iter()
        .any(|component| matches!(component.to_str(), Some("ports" | "tests")));
    !excluded_file && !excluded_directory
}

fn is_logic_path(path: &[&std::ffi::OsStr]) -> bool {
    path.len() >= 2 && path.last().is_some_and(|name| *name == "logic.rs")
}

fn use_tree_has_effect(tree: &UseTree) -> bool {
    match tree {
        UseTree::Path(path) => match normalized_identifier(&path.ident).as_str() {
            "std" => std_use_tree_has_effect(&path.tree),
            "chrono" => chrono_use_tree_has_effect(&path.tree),
            root if external_root(root) => true,
            _ => false,
        },
        UseTree::Name(name) => external_root(&normalized_identifier(&name.ident)),
        UseTree::Rename(rename) => capability_root(&normalized_identifier(&rename.ident)),
        UseTree::Group(group) => group.items.iter().any(use_tree_has_effect),
        UseTree::Glob(_) => false,
    }
}

fn std_use_tree_has_effect(tree: &UseTree) -> bool {
    match tree {
        UseTree::Path(path) => match normalized_identifier(&path.ident).as_str() {
            "fs" | "process" | "net" => true,
            "time" => time_use_tree_has_effect(&path.tree),
            _ => false,
        },
        UseTree::Name(name) => matches!(
            normalized_identifier(&name.ident).as_str(),
            "fs" | "process" | "net"
        ),
        UseTree::Rename(rename) => matches!(
            normalized_identifier(&rename.ident).as_str(),
            "self" | "fs" | "process" | "net" | "time"
        ),
        UseTree::Group(group) => group.items.iter().any(std_use_tree_has_effect),
        UseTree::Glob(_) => true,
    }
}

fn time_use_tree_has_effect(tree: &UseTree) -> bool {
    match tree {
        UseTree::Path(path) => matches!(
            normalized_identifier(&path.ident).as_str(),
            "SystemTime" | "Instant"
        ),
        UseTree::Name(name) => matches!(
            normalized_identifier(&name.ident).as_str(),
            "SystemTime" | "Instant"
        ),
        UseTree::Rename(rename) => matches!(
            normalized_identifier(&rename.ident).as_str(),
            "self" | "SystemTime" | "Instant"
        ),
        UseTree::Group(group) => group.items.iter().any(time_use_tree_has_effect),
        UseTree::Glob(_) => true,
    }
}

fn chrono_use_tree_has_effect(tree: &UseTree) -> bool {
    match tree {
        UseTree::Path(path) => {
            matches!(normalized_identifier(&path.ident).as_str(), "Local" | "Utc")
        }
        UseTree::Name(name) => {
            matches!(normalized_identifier(&name.ident).as_str(), "Local" | "Utc")
        }
        UseTree::Rename(rename) => matches!(
            normalized_identifier(&rename.ident).as_str(),
            "self" | "Local" | "Utc"
        ),
        UseTree::Group(group) => group.items.iter().any(chrono_use_tree_has_effect),
        UseTree::Glob(_) => true,
    }
}

fn path_has_effect(path: &syn::Path) -> bool {
    let segments = path
        .segments
        .iter()
        .map(|segment| normalized_identifier(&segment.ident))
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [root, ..] if external_root(root) => true,
        [root, capability, ..]
            if root == "std" && matches!(capability.as_str(), "fs" | "process" | "net") =>
        {
            true
        }
        [root, time, clock, ..]
            if root == "std"
                && time == "time"
                && matches!(clock.as_str(), "SystemTime" | "Instant") =>
        {
            true
        }
        [root, clock, ..] if root == "chrono" && matches!(clock.as_str(), "Local" | "Utc") => true,
        _ => false,
    }
}

fn capability_root(root: &str) -> bool {
    matches!(root, "std" | "chrono") || external_root(root)
}

fn external_root(root: &str) -> bool {
    matches!(root, "sqlx" | "tokio" | "pwf_application" | "pwf_infra")
}
