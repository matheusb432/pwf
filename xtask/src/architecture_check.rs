//! Conservative dependency and source checks for workspace architecture.

use std::{collections::HashSet, path::Path};

use anyhow::{Context, Result};
use cargo_metadata::{DependencyKind, MetadataCommand};

mod source_policy;

const DEPENDENCY_EDGES_FORBIDDEN: &[(&str, &str)] = &[
    ("pwf-domain", "pwf-application"),
    ("pwf-domain", "pwf-infra"),
    ("pwf-domain", "pwf"),
    ("pwf-application", "pwf-infra"),
    ("pwf-application", "pwf"),
    ("pwf-infra", "pwf"),
];

/// Locates one architecture violation and explains the rejected dependency.
pub(crate) struct Violation {
    pub(crate) relative_path: String,
    pub(crate) line: usize,
    pub(crate) message: String,
}

/// Checks normal workspace dependencies and syntax-backed source invariants.
pub(crate) fn run(repo_root: &Path) -> Result<Result<(), Vec<Violation>>> {
    let metadata = MetadataCommand::new()
        .current_dir(repo_root)
        .no_deps()
        .exec()
        .context("reading Cargo workspace metadata for architecture check")?;
    let package_names = metadata
        .workspace_packages()
        .into_iter()
        .map(|package| package.name.as_str())
        .collect::<HashSet<_>>();
    let mut dependency_edges_reported = HashSet::new();
    let mut violations = Vec::new();

    for package in metadata.workspace_packages() {
        for dependency in package.dependencies.iter().filter(|dependency| {
            dependency.kind == DependencyKind::Normal
                && package_names.contains(dependency.name.as_str())
                && DEPENDENCY_EDGES_FORBIDDEN
                    .contains(&(package.name.as_str(), dependency.name.as_str()))
        }) {
            if !dependency_edges_reported.insert((package.name.as_str(), dependency.name.as_str()))
            {
                continue;
            }
            let manifest_path = package
                .manifest_path
                .strip_prefix(&metadata.workspace_root)
                .map_or_else(|_| package.manifest_path.to_string(), ToString::to_string);
            violations.push(Violation {
                relative_path: manifest_path,
                line: 1,
                message: format!(
                    "{} must not depend on outward layer {}",
                    package.name, dependency.name
                ),
            });
        }
    }
    violations.extend(source_policy::violations(&metadata)?);

    Ok(if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn workspace_with_dependencies(
        dependency_domain: &str,
        dependency_application: &str,
    ) -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temporary workspace");
        fs::write(
            directory.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"3\"\n",
        )
        .expect("workspace manifest");

        for (directory_name, package_name, dependencies) in [
            ("domain", "pwf-domain", dependency_domain),
            ("application", "pwf-application", dependency_application),
            (
                "infra",
                "pwf-infra",
                "[dependencies]\npwf-application = { path = \"../application\" }\n",
            ),
            (
                "cli",
                "pwf",
                "[dependencies]\npwf-infra = { path = \"../infra\" }\n",
            ),
        ] {
            let package_directory = directory.path().join("crates").join(directory_name);
            fs::create_dir_all(package_directory.join("src")).expect("package source directory");
            fs::write(
                package_directory.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{dependencies}"
                ),
            )
            .expect("package manifest");
            fs::write(package_directory.join("src/lib.rs"), "").expect("package source");
        }

        directory
    }

    #[test]
    fn dependency_policy_accepts_inward_edges() {
        let workspace = workspace_with_dependencies(
            "",
            "[dependencies]\npwf-domain = { path = \"../domain\" }\n",
        );

        assert!(run(workspace.path()).unwrap().is_ok());
    }

    #[test]
    fn dependency_policy_rejects_domain_to_application() {
        let workspace = workspace_with_dependencies(
            "[dependencies]\npwf-application = { path = \"../application\" }\n",
            "",
        );

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].relative_path, "crates/domain/Cargo.toml");
        assert_eq!(violations[0].line, 1);
        assert_eq!(
            violations[0].message,
            "pwf-domain must not depend on outward layer pwf-application"
        );
    }

    #[test]
    fn dependency_policy_ignores_development_edges() {
        let workspace = workspace_with_dependencies(
            "[dev-dependencies]\npwf-application = { path = \"../application\" }\n",
            "",
        );

        assert!(run(workspace.path()).unwrap().is_ok());
    }

    #[test]
    fn source_policy_rejects_invalid_calls_in_macros_and_attributes() {
        let workspace = workspace_with_dependencies("", "");
        fs::write(
            workspace.path().join("crates/application/src/lib.rs"),
            r"fn inspect() {
    assert_eq!(execute(command), expected);
    assert_eq!(wrap(execute(command)), expected);
    assert_eq!(r#execute(command), expected);
    assert_eq!(execute::<Command>(command), expected);
    assert_eq!(crate::execute(command), expected);
    assert_eq!(self::execute(command), expected);
    assert_eq!(Self::execute(command), expected);
    assert_eq!(::add::execute(command), expected);
    let _ = vec![pwf_application::pending_work::add::execute(command)];
    assert_eq!(<T as Operation>::execute(command), expected);
    assert_eq!(<T as application::Operation>::execute(command), expected);
    dsl! { command => execute(command); }
}

#[route(default = execute(command))]
fn tagged() {}

#[route(default = wrap(execute(command)))]
fn nested_tagged() {}
",
        )
        .expect("application source");

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(
            violations
                .iter()
                .map(|violation| violation.line)
                .collect::<Vec<_>>(),
            [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, 19]
        );
        assert!(violations.iter().all(|violation| {
            violation.relative_path == "crates/application/src/lib.rs"
                && violation.message == "Call `execute` through exactly one operation module."
        }));
    }

    #[test]
    fn source_policy_accepts_exact_qualifiers_and_non_calls_in_opaque_syntax() {
        let workspace = workspace_with_dependencies("", "");
        fs::write(
            workspace.path().join("crates/application/src/lib.rs"),
            r#"macro_rules! compare {
    ($execute:ident) => {
        assert_eq!($execute(command), expected);
    };
}

fn inspect() {
    assert_eq!(add::execute(command), expected);
    assert_eq!(super::execute(command), expected);
    assert_eq!(add::execute::<Command>(command), expected);
    assert_eq!(add::r#execute(command), expected);
    assert_eq!(query.execute(pool), expected);
    assert_eq!(query.execute::<Command>(pool), expected);
    assert_eq!("execute(command)", "execute(command)");
    // execute(command)
    quote! { execute(command) };
    quote_spanned! { span => execute(command) };
    r#quote! { execute(command) };
    r#quote_spanned! { span => execute(command) };
    declare! { fn execute(command: Command) {} }
    dsl! { $operation::execute(command) }
    dsl! { outer => { $operation::execute(command) } }
    dsl! { $operation::{execute(command)} }
}
"#,
        )
        .expect("application source");

        assert!(run(workspace.path()).unwrap().is_ok());
    }

    #[test]
    fn source_policy_skips_nested_quote_dsl_and_rejects_adjacent_calls() {
        let workspace = workspace_with_dependencies("", "");
        fs::write(
            workspace.path().join("crates/application/src/lib.rs"),
            r"fn inspect() {
    outer_dsl! {
        generated => quote! { execute(generated) },
        generated_spanned => quote_spanned! { span => execute(generated) },
        raw_generated => r#quote! { execute(generated) },
        raw_generated_spanned => r#quote_spanned! { span => execute(generated) },
        real => execute(command),
    }
}
",
        )
        .expect("application source");

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].relative_path, "crates/application/src/lib.rs");
        assert_eq!(violations[0].line, 7);
    }

    #[test]
    fn source_policy_limits_metavariable_allowance_to_its_path() {
        let workspace = workspace_with_dependencies("", "");
        fs::write(
            workspace.path().join("crates/application/src/lib.rs"),
            r"fn inspect() {
    dsl! {
        templated => $operation::execute(command),
        nested => $operation::{execute(command)},
        real => execute(command),
        multi => application::add::execute(command),
    }
}
",
        )
        .expect("application source");

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(
            violations
                .iter()
                .map(|violation| violation.line)
                .collect::<Vec<_>>(),
            [5, 6]
        );
    }

    #[test]
    fn source_policy_rejects_operation_exports_in_source_and_macro_transcribers() {
        let workspace = workspace_with_dependencies("", "");
        let feature = workspace.path().join("crates/application/src/pending_work");
        fs::create_dir_all(&feature).expect("application feature directory");
        fs::write(
            feature.join("add.rs"),
            r"pub fn execute() {}
pub fn run() {}
pub use std::mem::drop as run;
macro_rules! generate {
    () => {
        pub fn generated() {}
        pub use std::mem::forget as generated_forget;
    };
    (pub fn matcher_only() {}) => {
        quote! {
            pub fn quoted() {}
            pub use std::mem::drop as quoted_drop;
        }
    };
}
",
        )
        .expect("application operation source");

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(
            violations
                .iter()
                .map(|violation| (violation.line, violation.message.as_str()))
                .collect::<Vec<_>>(),
            [
                (
                    2,
                    "Application operation files may expose `execute` and declared contract items, not other functions or re-exports."
                ),
                (
                    3,
                    "Application operation files may expose `execute` and declared contract items, not other functions or re-exports."
                ),
                (
                    6,
                    "Application operation files may expose `execute` and declared contract items, not other functions or re-exports."
                ),
                (
                    7,
                    "Application operation files may expose `execute` and declared contract items, not other functions or re-exports."
                ),
            ]
        );
        assert!(violations.iter().all(|violation| {
            violation.relative_path == "crates/application/src/pending_work/add.rs"
        }));
    }

    #[test]
    fn source_policy_rejects_effects_in_source_and_macro_transcribers() {
        let workspace = workspace_with_dependencies("", "");
        fs::write(
            workspace.path().join("crates/domain/src/lib.rs"),
            r#"use std as runtime;
fn aliased() { let _ = runtime::fs::read("item"); }
fn direct() { let _ = std::fs::read("item"); }
macro_rules! generate {
    () => {
        use chrono as clock;
        fn now() { let _ = chrono::Utc::now(); }
        fn query() { let _ = sqlx::query!("SELECT 1"); }
    };
}
"#,
        )
        .expect("domain source");
        let logic = workspace.path().join("crates/application/src/pending_work");
        fs::create_dir_all(&logic).expect("application feature directory");
        fs::write(
            logic.join("logic.rs"),
            r"macro_rules! generate {
    () => {
        use pwf_infra as infrastructure;
        fn load() { let _ = tokio::runtime::Runtime::new(); }
    };
}
",
        )
        .expect("application logic source");

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(
            violations
                .iter()
                .map(|violation| {
                    (
                        violation.relative_path.as_str(),
                        violation.line,
                        violation.message.as_str(),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (
                    "crates/application/src/pending_work/logic.rs",
                    3,
                    "Application `logic.rs` modules must remain effect-free.",
                ),
                (
                    "crates/application/src/pending_work/logic.rs",
                    4,
                    "Application `logic.rs` modules must remain effect-free.",
                ),
                (
                    "crates/domain/src/lib.rs",
                    1,
                    "Domain models must not import or invoke effectful capabilities.",
                ),
                (
                    "crates/domain/src/lib.rs",
                    3,
                    "Domain models must not import or invoke effectful capabilities.",
                ),
                (
                    "crates/domain/src/lib.rs",
                    6,
                    "Domain models must not import or invoke effectful capabilities.",
                ),
                (
                    "crates/domain/src/lib.rs",
                    7,
                    "Domain models must not import or invoke effectful capabilities.",
                ),
                (
                    "crates/domain/src/lib.rs",
                    8,
                    "Domain models must not import or invoke effectful capabilities.",
                ),
            ]
        );
    }

    #[test]
    fn source_policy_rejects_metavariable_uses_in_macro_transcribers() {
        let workspace = workspace_with_dependencies("", "");
        let feature = workspace.path().join("crates/application/src/pending_work");
        fs::create_dir_all(&feature).expect("application feature directory");
        fs::write(
            feature.join("add.rs"),
            r"pub fn execute() {}
macro_rules! expose {
    ($name:ident) => {
        pub use std::mem::drop as $name;
        pub use ::std::mem::{forget as $name};
        nested! {
            pub use {
                std::mem::drop as $name,
            };
        }
    };
}
",
        )
        .expect("application operation source");
        fs::write(
            workspace.path().join("crates/domain/src/lib.rs"),
            r"macro_rules! effects {
    ($runtime:ident, $clock:ident) => {
        use std as $runtime;
        use ::std::time as $clock;
        use {
            ::chrono as $clock,
            core::fmt,
        };
        use std::{self as $runtime, collections::BTreeMap};
        nested! {
            use ::sqlx as $runtime;
        }
    };
}
",
        )
        .expect("domain source");

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(
            violations
                .iter()
                .map(|violation| {
                    (
                        violation.relative_path.as_str(),
                        violation.line,
                        violation.message.as_str(),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (
                    "crates/application/src/pending_work/add.rs",
                    4,
                    "Application operation files may expose `execute` and declared contract items, not other functions or re-exports.",
                ),
                (
                    "crates/application/src/pending_work/add.rs",
                    5,
                    "Application operation files may expose `execute` and declared contract items, not other functions or re-exports.",
                ),
                (
                    "crates/application/src/pending_work/add.rs",
                    7,
                    "Application operation files may expose `execute` and declared contract items, not other functions or re-exports.",
                ),
                (
                    "crates/domain/src/lib.rs",
                    3,
                    "Domain models must not import or invoke effectful capabilities.",
                ),
                (
                    "crates/domain/src/lib.rs",
                    4,
                    "Domain models must not import or invoke effectful capabilities.",
                ),
                (
                    "crates/domain/src/lib.rs",
                    5,
                    "Domain models must not import or invoke effectful capabilities.",
                ),
                (
                    "crates/domain/src/lib.rs",
                    9,
                    "Domain models must not import or invoke effectful capabilities.",
                ),
                (
                    "crates/domain/src/lib.rs",
                    11,
                    "Domain models must not import or invoke effectful capabilities.",
                ),
            ]
        );
    }

    #[test]
    fn source_policy_rejects_generated_function_headers_with_unparseable_contracts() {
        let workspace = workspace_with_dependencies("", "");
        let feature = workspace.path().join("crates/application/src/pending_work");
        fs::create_dir_all(&feature).expect("application feature directory");
        fs::write(
            feature.join("add.rs"),
            r#"pub fn execute() {}
macro_rules! expose {
    ($name:ident, $($body:tt)*) => {
        pub fn $name() { $($body)* }
        nested! {
            pub fn $nested() { $($body)* }
            grouped! {{
                pub fn $grouped() { $($body)* }
            }}
        }
        pub fn $parameters($($argument: $kind),*) {}
        pub fn $generic<$($type: $bound),*>($value: $kind) {}
        $(#[$attribute])* pub fn $attributed() { $($body)* }
        pub fn $result<T: $bound>($value: $kind) -> $output
        where
            T: $bound,
        {
            body_dsl!($($body)*);
        }
        pub async fn $asynchronous($($argument: $kind),*) {}
        pub const fn $constant() -> $output { $($body)* }
        pub unsafe fn $unsafe_name() { $($body)* }
        pub extern "C" fn $external($($argument: $kind),*) {}
        $visibility fn maybe_public() { $($body)* }
        $visibility async fn $visible_name() { $($body)* }
        $($maybe_public)* fn $conditional_name() { $($body)* }
        pub fn adjacent() {}
        pub fn execute($($argument: $kind),*) -> $output { $($body)* }
    };
}
"#,
        )
        .expect("application operation source");

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(
            violations
                .iter()
                .map(|violation| violation.line)
                .collect::<Vec<_>>(),
            [4, 6, 8, 11, 12, 13, 14, 20, 21, 22, 23, 24, 25, 26, 27]
        );
        assert!(violations.iter().all(|violation| {
            violation.relative_path == "crates/application/src/pending_work/add.rs"
                && violation.message
                    == "Application operation files may expose `execute` and declared contract items, not other functions or re-exports."
        }));
    }

    #[test]
    fn source_policy_rejects_visibility_after_repeated_attributes() {
        let workspace = workspace_with_dependencies("", "");
        let feature = workspace.path().join("crates/application/src/pending_work");
        fs::create_dir_all(&feature).expect("application feature directory");
        fs::write(
            feature.join("add.rs"),
            r"pub fn execute() {}
macro_rules! expose {
    () => {
        $(#[$attribute] pub)? fn mixed() {}
        $(#[$first] #[$second] pub)* fn multiple_attributes() {}
        $(#[$attribute] $visibility)+ fn metavariable() {}
        $(#[$attribute] $(pub)?)* fn nested_public() {}
        $(#[$attribute] $($visibility)*)? fn nested_visibility() {}
        $(#[$attribute] $(#[$nested] pub)?)* fn nested_attributes_public() {}
        $(#[$attribute] pub)? fn execute() {}
    };
}
",
        )
        .expect("application operation source");

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(
            violations
                .iter()
                .map(|violation| violation.line)
                .collect::<Vec<_>>(),
            [4, 5, 6, 7, 8, 9]
        );
        assert!(violations.iter().all(|violation| {
            violation.relative_path == "crates/application/src/pending_work/add.rs"
                && violation.message
                    == "Application operation files may expose `execute` and declared contract items, not other functions or re-exports."
        }));
    }

    #[test]
    fn source_policy_rejects_separated_visibility_repetitions() {
        let workspace = workspace_with_dependencies("", "");
        let feature = workspace.path().join("crates/application/src/pending_work");
        fs::create_dir_all(&feature).expect("application feature directory");
        fs::write(
            feature.join("add.rs"),
            r"pub fn execute() {}
macro_rules! expose {
    () => {
        $(#[$attribute] pub),* fn exact_reproducer() {}
        $(#[$attribute] $visibility);+ fn semicolon_visibility() {}
        $(#[$attribute] pub)=>* fn fat_arrow_public() {}
        $(#[$attribute] $(pub),*)? fn nested_comma_public() {}
        $(#[$attribute] $(#[$nested] $visibility)=>+)* fn nested_fat_arrow_visibility() {}
        $(#[$attribute] pub),* fn execute() {}
    };
}
",
        )
        .expect("application operation source");

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(
            violations
                .iter()
                .map(|violation| violation.line)
                .collect::<Vec<_>>(),
            [4, 5, 6, 7, 8]
        );
        assert!(violations.iter().all(|violation| {
            violation.relative_path == "crates/application/src/pending_work/add.rs"
                && violation.message
                    == "Application operation files may expose `execute` and declared contract items, not other functions or re-exports."
        }));
    }

    #[test]
    fn source_policy_preserves_separated_visibility_repetition_controls() {
        let workspace = workspace_with_dependencies("", "");
        let feature = workspace.path().join("crates/application/src/pending_work");
        fs::create_dir_all(&feature).expect("application feature directory");
        fs::write(
            feature.join("add.rs"),
            r"pub fn execute() {}
macro_rules! preserve {
    () => {
        $(#[$attribute]),* fn $separated_attributes() {}
        $(#[$first] #[$second])=>+ fn $separated_multiple_attributes() {}
        $(#[$attribute] pub(crate));* fn $separated_crate() {}
        $(#[$attribute] pub(super)),+ fn $separated_super() {}
        $(#[$attribute] $(pub(self));*)? fn $nested_separated_restricted() {}
        $(#[$attribute]) pub * fn $keyword_separated_attributes() {}
        $(#[$attribute] $(#[$nested]) pub *)? fn $nested_keyword_separated_attributes() {}
        $(#[$attribute] pub),* fn execute() {}
        $(#[$attribute] $visibility)=>+ fn execute() {}
        $(#[$attribute] pub),? fn $separator_before_question() {}
        $(#[$attribute] pub), fn $missing_operator() {}
        $(#[$attribute] pub),** fn $extra_operator() {}
        $[pub],* fn $bracketed_non_repetition() {}
        $(#[$attribute] $(pub),?)* fn $nested_separator_before_question() {}
    };
}
",
        )
        .expect("application operation source");

        assert!(run(workspace.path()).unwrap().is_ok());
    }

    #[test]
    fn source_policy_preserves_restricted_exports_pure_effects_and_quote_dsl() {
        let workspace = workspace_with_dependencies("", "");
        fs::write(
            workspace.path().join("crates/domain/src/lib.rs"),
            r#"use std::{collections::BTreeMap, time::Duration};
use chrono::{Duration as ChronoDuration, NaiveDate};
macro_rules! preserve {
    (use std as matcher_runtime;) => {};
    (
        std::fs,
        use std as runtime,
        pub use std::mem::drop as $name:ident,
        use std as $runtime:ident,
    ) => {
        use std::collections::BTreeMap as $name;
        use ::std::time::Duration as $runtime;
        pub fn $domain_function($($argument: $kind),*) { $($body)* }
        quote! {
            use std as $runtime;
            fn generated() { let _ = std::fs::read("item"); }
        }
    };
}
ordinary_dsl! {
    use std as $runtime;
}
outer_dsl! {
    generated => quote_spanned! { span =>
        use chrono as clock;
        fn generated() { let _ = chrono::Utc::now(); }
    },
}
"#,
        )
        .expect("domain source");
        let application = workspace.path().join("crates/application/src");
        let feature = application.join("pending_work");
        fs::create_dir_all(&feature).expect("application feature directory");
        fs::write(
            application.join("pending_work.rs"),
            "macro_rules! facade { () => { pub use operation::Request; }; }\n",
        )
        .expect("application facade source");
        fs::write(
            feature.join("dto.rs"),
            "macro_rules! dto { () => { pub use super::Response; }; }\n",
        )
        .expect("application DTO source");
        fs::write(
            feature.join("add.rs"),
            r#"pub fn execute() {}
macro_rules! operation {
    (pub use std::mem::drop as matcher_only;) => {};
    (pub fn $matcher:ident() { $($body:tt)* }) => {};
    (
        pub fn matcher_only() {},
        pub use std::mem::drop as $name:ident,
    ) => {
        pub(crate) fn normalize() {}
        pub(crate) async fn $crate_name($($argument: $kind),*) { $($body)* }
        pub(super) extern "C" fn $super_name() -> $output { $($body)* }
        unsafe fn $private_name($($argument: $kind),*) { $($body)* }
        $(#[$attribute])* fn $attributed_private() { $($body)* }
        $(#[$first] #[$second])* fn $attributes_only() { $($body)* }
        $(#[$attribute] $(#[$nested])*)? fn $nested_attributes_only() { $($body)* }
        $(#[$attribute] pub(crate))? fn $repeated_crate() { $($body)* }
        $(#[$attribute] pub(super))* fn $repeated_super() { $($body)* }
        $(#[$attribute] pub(self))+ fn $repeated_self() { $($body)* }
        $(#[$attribute] $(pub(crate))?)* fn $nested_restricted() { $($body)* }
        pub async fn execute($($argument: $kind),*) -> $output { $($body)* }
        $visibility fn execute() { $($body)* }
        $(#[$attribute] pub)? fn execute() { $($body)* }
        $(#[$attribute] $visibility)? fn execute() { $($body)* }
        pub(crate) use std::mem::drop as $name;
        pub(super) use ::std::mem::{forget as $name};
        quote! {
            pub fn generated() { $($body)* }
            pub fn $quoted($($argument: $kind),*) { $($body)* }
            pub use std::mem::drop as $name;
        }
        quote_spanned! { span =>
            pub fn $quoted_spanned() { $($body)* }
        }
    };
}
operation_dsl! {
    pub fn $outside($($argument: $kind),*) { $($body)* }
    pub use std::mem::drop as $name;
}
"#,
        )
        .expect("application operation source");

        assert!(run(workspace.path()).unwrap().is_ok());
    }

    #[test]
    fn source_discovery_includes_directories_named_target() {
        let workspace = workspace_with_dependencies("", "");
        let source_directory = workspace.path().join("crates/application/src/target");
        fs::create_dir_all(&source_directory).expect("nested target source directory");
        fs::write(
            source_directory.join("generated.rs"),
            "fn inspect() { assert_eq!(execute(command), expected); }\n",
        )
        .expect("nested target source");

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].relative_path,
            "crates/application/src/target/generated.rs"
        );
    }

    #[test]
    fn source_policy_excludes_only_the_reusable_test_runner_package() {
        let workspace = workspace_with_dependencies("", "");
        fs::write(
            workspace.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\", \"xtask\", \"xtask/xtk_test\"]\nresolver = \"3\"\n",
        )
        .expect("workspace manifest");
        for (directory, package) in [("xtask", "xtask"), ("xtask/xtk_test", "xtk_test")] {
            let root = workspace.path().join(directory);
            fs::create_dir_all(root.join("src")).expect("package source directory");
            fs::write(
                root.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
                ),
            )
            .expect("package manifest");
            fs::write(
                root.join("src/lib.rs"),
                "fn inspect() { assert_eq!(execute(command), expected); }\n",
            )
            .expect("package source");
        }

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(
            violations.len(),
            1,
            "{:?}",
            violations
                .iter()
                .map(|violation| &violation.relative_path)
                .collect::<Vec<_>>()
        );
        assert_eq!(violations[0].relative_path, "xtask/src/lib.rs");
    }

    #[cfg(unix)]
    #[test]
    fn source_discovery_includes_symlinked_rust_files() {
        use std::os::unix::fs::symlink;

        let workspace = workspace_with_dependencies("", "");
        let generated_source = workspace.path().join("generated.rs");
        fs::write(
            &generated_source,
            "fn inspect() { assert_eq!(execute(command), expected); }\n",
        )
        .expect("generated source");
        symlink(
            &generated_source,
            workspace.path().join("crates/application/src/generated.rs"),
        )
        .expect("source symlink");

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].relative_path,
            "crates/application/src/generated.rs"
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_discovery_reports_symlink_cycles() {
        use std::os::unix::fs::symlink;

        let workspace = workspace_with_dependencies("", "");
        let source_directory = workspace.path().join("crates/application/src");
        symlink(&source_directory, source_directory.join("cycle")).expect("source cycle");

        let Err(error) = run(workspace.path()) else {
            panic!("source cycle should fail the architecture check");
        };

        assert!(
            format!("{error:#}").contains("File system loop found"),
            "unexpected error: {error:#}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_discovery_excludes_only_the_cargo_target_directory() {
        use std::os::unix::fs::symlink;

        let workspace = workspace_with_dependencies("", "");
        let target_directory = workspace.path().join("target");
        fs::create_dir_all(&target_directory).expect("Cargo target directory");
        fs::write(
            target_directory.join("generated.rs"),
            "fn inspect() { assert_eq!(execute(command), expected); }\n",
        )
        .expect("target source");
        symlink(
            &target_directory,
            workspace.path().join("crates/application/src/build-output"),
        )
        .expect("target directory symlink");

        assert!(run(workspace.path()).unwrap().is_ok());
    }
}
