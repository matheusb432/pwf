use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result};
use cargo_metadata::Metadata;
use proc_macro2::{Delimiter, Group, Ident, TokenStream, TokenTree};
use syn::{
    Attribute, Block, Expr, ExprCall, ExprPath, ItemFn, ItemUse, Macro, Meta, Token, parse::Parser,
    punctuated::Punctuated, visit::Visit,
};
use walkdir::{DirEntry, WalkDir};

use super::Violation;

mod boundary;
mod macro_item;

const MESSAGE: &str = "Call `execute` through exactly one operation module.";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SourceFinding {
    line: usize,
    message: &'static str,
}

impl SourceFinding {
    const fn new(line: usize, message: &'static str) -> Self {
        Self { line, message }
    }
}

pub(super) fn violations(metadata: &Metadata) -> Result<Vec<Violation>> {
    let workspace_root = metadata.workspace_root.as_std_path();
    let target_directory = metadata.target_directory.as_std_path();
    let canonical_target_directory = target_directory.canonicalize().ok();
    let mut source_files = BTreeSet::new();
    for package in metadata.workspace_packages() {
        let package_root = package
            .manifest_path
            .parent()
            .with_context(|| format!("package {} manifest has no parent", package.name))?
            .as_std_path();
        for entry in WalkDir::new(package_root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|entry| {
                is_source_entry(
                    entry,
                    target_directory,
                    canonical_target_directory.as_deref(),
                )
            })
        {
            let entry = entry.with_context(|| {
                format!(
                    "walking Rust sources below {}",
                    package_root.to_string_lossy()
                )
            })?;
            let path = entry.path();
            if entry.file_type().is_file() && path.extension().is_some_and(|value| value == "rs") {
                source_files.insert(path.to_path_buf());
            }
        }
    }

    let mut violations = Vec::new();
    for source_file in source_files {
        inspect_source(workspace_root, &source_file, &mut violations)?;
    }
    Ok(violations)
}

fn is_source_entry(
    entry: &DirEntry,
    target_directory: &Path,
    canonical_target_directory: Option<&Path>,
) -> bool {
    !entry.file_type().is_dir()
        || (entry.path() != target_directory
            && !(entry.path_is_symlink()
                && canonical_target_directory.is_some_and(|target| {
                    entry.path().canonicalize().is_ok_and(|path| path == target)
                })))
}

fn inspect_source(
    workspace_root: &Path,
    source_file: &Path,
    violations: &mut Vec<Violation>,
) -> Result<()> {
    let source = fs::read_to_string(source_file)
        .with_context(|| format!("reading Rust source {}", source_file.to_string_lossy()))?;
    let syntax = syn::parse_file(&source)
        .with_context(|| format!("parsing Rust source {}", source_file.to_string_lossy()))?;
    let relative_path = source_file
        .strip_prefix(workspace_root)
        .unwrap_or(source_file);
    let relative_path_display = relative_path.to_string_lossy().into_owned();
    let mut findings = Vec::new();
    SyntaxPolicyVisitor {
        scope: boundary::Scope::for_source(relative_path, &syntax),
        context: FallbackContext::default(),
        findings: &mut findings,
    }
    .visit_file(&syntax);
    findings.sort_unstable();
    findings.dedup();
    violations.extend(findings.into_iter().map(|finding| Violation {
        relative_path: relative_path_display.clone(),
        line: finding.line,
        message: finding.message.to_string(),
    }));
    Ok(())
}

#[derive(Clone, Copy, Default)]
struct FallbackContext {
    inspect_metavariable_items: bool,
    metavariable_path: bool,
}

fn inspect_opaque_tokens(
    stream: TokenStream,
    scope: boundary::Scope,
    context: FallbackContext,
    findings: &mut Vec<SourceFinding>,
) {
    if let Ok(file) = syn::parse2::<syn::File>(stream.clone()) {
        SyntaxPolicyVisitor {
            scope,
            context,
            findings,
        }
        .visit_file(&file);
        return;
    }

    let expression_parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    if let Ok(expressions) = expression_parser.parse2(stream.clone()) {
        let mut visitor = SyntaxPolicyVisitor {
            scope,
            context,
            findings,
        };
        for expression in &expressions {
            visitor.visit_expr(expression);
        }
        return;
    }

    let wrapped = TokenStream::from(TokenTree::Group(Group::new(
        Delimiter::Brace,
        stream.clone(),
    )));
    if let Ok(block) = syn::parse2::<Block>(wrapped) {
        SyntaxPolicyVisitor {
            scope,
            context,
            findings,
        }
        .visit_block(&block);
        return;
    }

    inspect_unparsed_tokens(stream, scope, context, findings);
}

fn inspect_unparsed_tokens(
    stream: TokenStream,
    scope: boundary::Scope,
    context: FallbackContext,
    findings: &mut Vec<SourceFinding>,
) {
    let tokens = stream.into_iter().collect::<Vec<_>>();
    if context.inspect_metavariable_items {
        macro_item::inspect_candidates(&tokens, scope, findings);
    }
    for (end, token) in tokens.iter().enumerate() {
        if parenthesized_arguments(token) {
            for start in 0..=end {
                if call_candidate_is_suffix(&tokens, start) {
                    continue;
                }
                let candidate = tokens[start..=end].iter().cloned().collect();
                let Ok(Expr::Call(call)) = syn::parse2::<Expr>(candidate) else {
                    continue;
                };
                if !context.metavariable_path && !candidate_has_metavariable_root(&tokens, start) {
                    SyntaxPolicyVisitor {
                        scope,
                        context,
                        findings,
                    }
                    .visit_expr_call(&call);
                }
                break;
            }
        }
        if let TokenTree::Group(group) = token
            && let Some(nested_context) = nested_context(&tokens, end, context)
        {
            inspect_macro_owner_path(&tokens, end, scope, findings);
            inspect_opaque_tokens(group.stream(), scope, nested_context, findings);
        }
    }
}

fn inspect_macro_owner_path(
    tokens: &[TokenTree],
    group_index: usize,
    scope: boundary::Scope,
    findings: &mut Vec<SourceFinding>,
) {
    let Some(path_end) = group_index.checked_sub(1) else {
        return;
    };
    if !tokens
        .get(path_end)
        .is_some_and(|token| punctuation_is(token, '!'))
    {
        return;
    }
    for start in 0..path_end {
        let candidate = tokens[start..path_end].iter().cloned().collect();
        if let Ok(path) = syn::parse2(candidate) {
            scope.inspect_path(&path, findings);
            break;
        }
    }
}

fn call_candidate_is_suffix(tokens: &[TokenTree], start: usize) -> bool {
    let Some(previous) = start.checked_sub(1).and_then(|index| tokens.get(index)) else {
        return false;
    };
    punctuation_is(previous, '.')
        || matches!(previous, TokenTree::Ident(identifier) if identifier == "fn")
}

fn candidate_has_metavariable_root(tokens: &[TokenTree], start: usize) -> bool {
    start
        .checked_sub(1)
        .and_then(|index| tokens.get(index))
        .is_some_and(|token| punctuation_is(token, '$'))
}

fn nested_context(
    tokens: &[TokenTree],
    group_index: usize,
    context: FallbackContext,
) -> Option<FallbackContext> {
    if group_is_quote_macro_arguments(tokens, group_index) {
        return None;
    }
    Some(FallbackContext {
        inspect_metavariable_items: context.inspect_metavariable_items,
        metavariable_path: context.metavariable_path
            || group_has_metavariable_path_prefix(tokens, group_index),
    })
}

fn group_is_quote_macro_arguments(tokens: &[TokenTree], group_index: usize) -> bool {
    group_index >= 2
        && punctuation_is(&tokens[group_index - 1], '!')
        && matches!(
            &tokens[group_index - 2],
            TokenTree::Ident(identifier)
                if identifier_is_quote(identifier)
        )
}

fn group_has_metavariable_path_prefix(tokens: &[TokenTree], group_index: usize) -> bool {
    (0..group_index)
        .rev()
        .filter(|index| punctuation_is(&tokens[*index], '$'))
        .any(|start| metavariable_path_reaches_group(&tokens[start..group_index]))
}

fn metavariable_path_reaches_group(tokens: &[TokenTree]) -> bool {
    if tokens.len() < 4
        || !punctuation_is(&tokens[0], '$')
        || !matches!(tokens[1], TokenTree::Ident(_))
    {
        return false;
    }
    let mut index = 2;
    loop {
        if !tokens.get(index..index + 2).is_some_and(double_colon) {
            return false;
        }
        index += 2;
        if index == tokens.len() {
            return true;
        }
        if !matches!(tokens[index], TokenTree::Ident(_)) {
            return false;
        }
        index += 1;
    }
}

fn double_colon(tokens: &[TokenTree]) -> bool {
    matches!(
        tokens,
        [first, second] if punctuation_is(first, ':') && punctuation_is(second, ':')
    )
}

struct SyntaxPolicyVisitor<'a> {
    scope: boundary::Scope,
    context: FallbackContext,
    findings: &'a mut Vec<SourceFinding>,
}

impl SyntaxPolicyVisitor<'_> {
    fn inspect_token_stream(&mut self, stream: TokenStream) {
        inspect_opaque_tokens(stream, self.scope, self.context, self.findings);
    }

    fn inspect_macro_rules(&mut self, stream: TokenStream) {
        let tokens = stream.into_iter().collect::<Vec<_>>();
        for (index, token) in tokens.iter().enumerate() {
            let TokenTree::Group(transcriber) = token else {
                continue;
            };
            let Some(arrow) = index
                .checked_sub(2)
                .and_then(|start| tokens.get(start..index))
            else {
                continue;
            };
            if matches!(
                arrow,
                [equals, greater]
                    if punctuation_is(equals, '=') && punctuation_is(greater, '>')
            ) {
                inspect_opaque_tokens(
                    transcriber.stream(),
                    self.scope,
                    FallbackContext {
                        inspect_metavariable_items: true,
                        ..self.context
                    },
                    self.findings,
                );
            }
        }
    }
}

impl<'ast> Visit<'ast> for SyntaxPolicyVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        self.scope.inspect_item_fn(node, self.findings);
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        self.scope.inspect_item_use(node, self.findings);
        syn::visit::visit_item_use(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if !self.context.metavariable_path {
            inspect_call(node, self.findings);
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast ExprPath) {
        self.scope.inspect_expr_path(node, self.findings);
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_macro(&mut self, node: &'ast Macro) {
        self.scope.inspect_path(&node.path, self.findings);
        if macro_is_quote(node) {
            return;
        }
        if macro_is_macro_rules(node) {
            self.inspect_macro_rules(node.tokens.clone());
        } else {
            self.inspect_token_stream(node.tokens.clone());
        }
    }

    fn visit_attribute(&mut self, node: &'ast Attribute) {
        if let Meta::List(meta) = &node.meta {
            self.inspect_token_stream(meta.tokens.clone());
        }
        syn::visit::visit_attribute(self, node);
    }
}

fn inspect_call(call: &ExprCall, findings: &mut Vec<SourceFinding>) {
    let Expr::Path(function) = call.func.as_ref() else {
        return;
    };
    let Some(operation) = function.path.segments.last() else {
        return;
    };
    if normalized_identifier(&operation.ident) == "execute"
        && !call_qualification_is_valid(function)
    {
        findings.push(SourceFinding::new(
            operation.ident.span().start().line,
            MESSAGE,
        ));
    }
}

fn call_qualification_is_valid(function: &ExprPath) -> bool {
    if function.qself.is_some() {
        return false;
    }
    let path = &function.path;
    if path.leading_colon.is_some() || path.segments.len() != 2 {
        return false;
    }
    let qualifier = normalized_identifier(&path.segments[0].ident);
    !matches!(qualifier.as_str(), "crate" | "self" | "Self")
}

fn normalized_identifier(identifier: &Ident) -> String {
    let identifier = identifier.to_string();
    identifier
        .strip_prefix("r#")
        .unwrap_or(&identifier)
        .to_string()
}

fn macro_is_quote(node: &Macro) -> bool {
    node.path
        .segments
        .last()
        .is_some_and(|segment| identifier_is_quote(&segment.ident))
}

fn macro_is_macro_rules(node: &Macro) -> bool {
    node.path.leading_colon.is_none()
        && node.path.segments.len() == 1
        && node
            .path
            .segments
            .first()
            .is_some_and(|segment| normalized_identifier(&segment.ident) == "macro_rules")
}

fn identifier_is_quote(identifier: &Ident) -> bool {
    matches!(
        normalized_identifier(identifier).as_str(),
        "quote" | "quote_spanned"
    )
}

fn parenthesized_arguments(token: &TokenTree) -> bool {
    matches!(token, TokenTree::Group(group) if group.delimiter() == Delimiter::Parenthesis)
}

fn punctuation_is(token: &TokenTree, expected: char) -> bool {
    matches!(token, TokenTree::Punct(punctuation) if punctuation.as_char() == expected)
}
