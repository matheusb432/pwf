use proc_macro2::{Delimiter, Group, Ident, Spacing, Span, TokenStream, TokenTree};

use super::{SourceFinding, boundary::Scope, punctuation_is};

const METAVARIABLE_PLACEHOLDER: &str = "__pwf_macro_metavariable";

pub(super) fn inspect_candidates(
    tokens: &[TokenTree],
    scope: Scope,
    findings: &mut Vec<SourceFinding>,
) {
    inspect_use_candidates(tokens, scope, findings);
    inspect_function_candidates(tokens, scope, findings);
}

fn inspect_use_candidates(tokens: &[TokenTree], scope: Scope, findings: &mut Vec<SourceFinding>) {
    for (use_index, token) in tokens.iter().enumerate() {
        if !matches!(token, TokenTree::Ident(identifier) if identifier == "use") {
            continue;
        }
        let Some(semicolon_offset) = tokens[use_index + 1..]
            .iter()
            .position(|token| punctuation_is(token, ';'))
        else {
            continue;
        };
        let end = use_index + 1 + semicolon_offset;
        let start = visibility_start(tokens, use_index).unwrap_or(use_index);
        let candidate = normalize(tokens[start..=end].iter().cloned().collect(), false);
        if let Ok(item) = syn::parse2(candidate) {
            scope.inspect_item_use(&item, findings);
        }
    }
}

fn inspect_function_candidates(
    tokens: &[TokenTree],
    scope: Scope,
    findings: &mut Vec<SourceFinding>,
) {
    for (function_index, token) in tokens.iter().enumerate() {
        if !matches!(token, TokenTree::Ident(identifier) if identifier == "fn") {
            continue;
        }
        let Some(public_span) = public_visibility_span(tokens, function_index) else {
            continue;
        };
        let Some(name) = function_name(tokens, function_index) else {
            continue;
        };
        let candidate = synthetic_function(public_span, token, name);
        if let Ok(item) = syn::parse2(candidate) {
            scope.inspect_item_fn(&item, findings);
        }
    }
}

fn public_visibility_span(tokens: &[TokenTree], function_index: usize) -> Option<Span> {
    let prefix_end = function_modifiers_start(tokens, function_index);
    let previous_index = prefix_end.checked_sub(1)?;
    if identifier_is(&tokens[previous_index], "pub") {
        return Some(tokens[previous_index].span());
    }
    if matches!(&tokens[previous_index], TokenTree::Group(group) if group.delimiter() == Delimiter::Parenthesis)
        && previous_index > 0
        && identifier_is(&tokens[previous_index - 1], "pub")
    {
        return None;
    }
    potential_visibility_span(tokens, prefix_end)
}

fn function_modifiers_start(tokens: &[TokenTree], function_index: usize) -> usize {
    let mut prefix_end = function_index;
    if prefix_end >= 2
        && matches!(tokens[prefix_end - 1], TokenTree::Literal(_))
        && identifier_is(&tokens[prefix_end - 2], "extern")
    {
        prefix_end -= 2;
    }
    while prefix_end > 0
        && matches!(
            &tokens[prefix_end - 1],
            TokenTree::Ident(identifier)
                if matches!(
                    identifier.to_string().as_str(),
                    "async" | "const" | "extern" | "safe" | "unsafe"
                )
        )
    {
        prefix_end -= 1;
    }
    prefix_end
}

fn potential_visibility_span(tokens: &[TokenTree], prefix_end: usize) -> Option<Span> {
    if let [.., dollar, TokenTree::Ident(identifier)] = &tokens[..prefix_end]
        && punctuation_is(dollar, '$')
    {
        // A visibility metavariable may expand to `pub`, so evaluate it as public.
        return Some(identifier.span());
    }
    if let [.., dollar, TokenTree::Group(group)] = &tokens[..prefix_end]
        && punctuation_is(dollar, '$')
        && group.delimiter() == Delimiter::Brace
    {
        return Some(group.span());
    }
    repeated_group_before(&tokens[..prefix_end]).and_then(repeated_visibility_span)
}

fn repeated_group_before(tokens: &[TokenTree]) -> Option<&Group> {
    (0..tokens.len().saturating_sub(1))
        .rev()
        .find_map(|dollar_index| {
            let [dollar, TokenTree::Group(group)] = tokens.get(dollar_index..dollar_index + 2)?
            else {
                return None;
            };
            if !punctuation_is(dollar, '$') || group.delimiter() != Delimiter::Parenthesis {
                return None;
            }
            let suffix = &tokens[dollar_index + 2..];
            repetition_suffix_len(suffix)
                .filter(|suffix_len| *suffix_len == suffix.len())
                .map(|_| group)
        })
}

fn repeated_visibility_span(group: &Group) -> Option<Span> {
    let tokens = group.stream().into_iter().collect::<Vec<_>>();
    let mut index = 0;
    while index < tokens.len() {
        if matches!(
            tokens.get(index..index + 2),
            Some([hash, TokenTree::Group(attribute)])
                if punctuation_is(hash, '#')
                    && attribute.delimiter() == Delimiter::Bracket
        ) {
            index += 2;
            continue;
        }
        if identifier_is(&tokens[index], "pub") {
            if matches!(
                tokens.get(index + 1),
                Some(TokenTree::Group(restriction))
                    if restriction.delimiter() == Delimiter::Parenthesis
            ) {
                index += 2;
                continue;
            }
            return Some(tokens[index].span());
        }
        if punctuation_is(&tokens[index], '$') {
            match tokens.get(index + 1) {
                Some(TokenTree::Ident(identifier)) => return Some(identifier.span()),
                Some(TokenTree::Group(nested)) => {
                    if nested.delimiter() == Delimiter::Parenthesis
                        && let Some(suffix_len) = repetition_suffix_len(&tokens[index + 2..])
                    {
                        if let Some(span) = repeated_visibility_span(nested) {
                            return Some(span);
                        }
                        index += 2 + suffix_len;
                        continue;
                    }
                    if nested.delimiter() == Delimiter::Brace {
                        return Some(nested.span());
                    }
                    index += 2;
                    continue;
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn repetition_suffix_len(tokens: &[TokenTree]) -> Option<usize> {
    if let Some(separator_len) = repetition_separator_len(tokens)
        && tokens
            .get(separator_len)
            .is_some_and(repetition_sequence_quantifier)
    {
        return Some(separator_len + 1);
    }
    tokens
        .first()
        .is_some_and(repetition_quantifier)
        .then_some(1)
}

fn repetition_separator_len(tokens: &[TokenTree]) -> Option<usize> {
    let first = tokens.first()?;
    match first {
        TokenTree::Ident(_) | TokenTree::Literal(_)
            if tokens.get(1).is_some_and(repetition_sequence_quantifier) =>
        {
            Some(1)
        }
        TokenTree::Punct(punctuation)
            if punctuation.as_char() == '\''
                && punctuation.spacing() == Spacing::Joint
                && matches!(tokens.get(1), Some(TokenTree::Ident(_)))
                && tokens.get(2).is_some_and(repetition_sequence_quantifier) =>
        {
            Some(2)
        }
        TokenTree::Punct(_) => (1..=3).rev().find(|separator_len| {
            tokens
                .get(*separator_len)
                .is_some_and(repetition_sequence_quantifier)
                && punctuation_separator_is_legal(&tokens[..*separator_len])
        }),
        _ => None,
    }
}

fn punctuation_separator_is_legal(tokens: &[TokenTree]) -> bool {
    let mut separator = String::with_capacity(tokens.len());
    for (index, token) in tokens.iter().enumerate() {
        let TokenTree::Punct(punctuation) = token else {
            return false;
        };
        if index + 1 < tokens.len() && punctuation.spacing() != Spacing::Joint {
            return false;
        }
        separator.push(punctuation.as_char());
    }
    match separator.as_str() {
        "*" | "+" | "?" => false,
        "==" | "!=" | "<=" | ">=" | "&&" | "||" | "<<" | ">>" | "+=" | "-=" | "*=" | "/="
        | "%=" | "^=" | "&=" | "|=" | "->" | "<-" | "=>" | ".." | "::" | "<<=" | ">>=" | "..="
        | "..." => true,
        _ => tokens.len() == 1,
    }
}

fn repetition_quantifier(token: &TokenTree) -> bool {
    matches!(
        token,
        TokenTree::Punct(punctuation)
            if matches!(punctuation.as_char(), '*' | '+' | '?')
    )
}

fn repetition_sequence_quantifier(token: &TokenTree) -> bool {
    matches!(
        token,
        TokenTree::Punct(punctuation)
            if matches!(punctuation.as_char(), '*' | '+')
    )
}

fn function_name(tokens: &[TokenTree], function_index: usize) -> Option<Ident> {
    match tokens.get(function_index + 1)? {
        TokenTree::Ident(identifier) => Some(identifier.clone()),
        dollar if punctuation_is(dollar, '$') => {
            let span = match tokens.get(function_index + 2)? {
                TokenTree::Ident(identifier) => identifier.span(),
                TokenTree::Group(group) => group.span(),
                _ => return None,
            };
            Some(Ident::new(METAVARIABLE_PLACEHOLDER, span))
        }
        _ => None,
    }
}

fn synthetic_function(public_span: Span, function: &TokenTree, name: Ident) -> TokenStream {
    let mut parameters = Group::new(Delimiter::Parenthesis, TokenStream::new());
    parameters.set_span(name.span());
    let mut body = Group::new(Delimiter::Brace, TokenStream::new());
    body.set_span(name.span());
    [
        TokenTree::Ident(Ident::new("pub", public_span)),
        function.clone(),
        TokenTree::Ident(name),
        TokenTree::Group(parameters),
        TokenTree::Group(body),
    ]
    .into_iter()
    .collect()
}

fn visibility_start(tokens: &[TokenTree], item_index: usize) -> Option<usize> {
    let previous_index = item_index.checked_sub(1)?;
    if identifier_is(&tokens[previous_index], "pub") {
        return Some(previous_index);
    }
    if matches!(&tokens[previous_index], TokenTree::Group(group) if group.delimiter() == proc_macro2::Delimiter::Parenthesis)
        && previous_index > 0
        && identifier_is(&tokens[previous_index - 1], "pub")
    {
        return Some(previous_index - 1);
    }
    None
}

fn identifier_is(token: &TokenTree, expected: &str) -> bool {
    matches!(token, TokenTree::Ident(identifier) if identifier == expected)
}

fn normalize(stream: TokenStream, strip_group_absolute_roots: bool) -> TokenStream {
    let tokens = stream.into_iter().collect::<Vec<_>>();
    let mut normalized = TokenStream::new();
    let mut index = 0;
    let mut group_entry_start = true;
    while index < tokens.len() {
        if strip_group_absolute_roots
            && group_entry_start
            && tokens
                .get(index..index + 2)
                .is_some_and(|tokens| tokens.iter().all(|token| punctuation_is(token, ':')))
        {
            index += 2;
            continue;
        }
        if punctuation_is(&tokens[index], '$')
            && let Some(TokenTree::Ident(identifier)) = tokens.get(index + 1)
        {
            normalized.extend([TokenTree::Ident(Ident::new(
                METAVARIABLE_PLACEHOLDER,
                identifier.span(),
            ))]);
            index += 2;
            group_entry_start = false;
            continue;
        }
        let token = match &tokens[index] {
            TokenTree::Group(group) => {
                let mut normalized_group = Group::new(
                    group.delimiter(),
                    normalize(group.stream(), group.delimiter() == Delimiter::Brace),
                );
                normalized_group.set_span(group.span());
                TokenTree::Group(normalized_group)
            }
            token => token.clone(),
        };
        group_entry_start = punctuation_is(&token, ',');
        normalized.extend([token]);
        index += 1;
    }
    normalized
}
