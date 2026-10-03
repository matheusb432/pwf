use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_textarea::TextArea;

pub(super) fn input_key(
    input: &mut TextArea<'_>,
    key: KeyEvent,
    multiline: bool,
    bytes_max: usize,
) -> bool {
    let newline = key.code == KeyCode::Enter
        || (key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('j' | 'm')));
    if newline && !multiline {
        return false;
    }
    let inserted_bytes = match key.code {
        _ if newline => 1,
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            character.len_utf8()
        }
        _ => 0,
    };
    if inserted_bytes > 0 && bytes(input) + inserted_bytes > bytes_max {
        return false;
    }
    if !input.input(key) {
        return false;
    }
    if bytes(input) > bytes_max {
        input.undo();
        return false;
    }
    true
}

pub(super) fn paste(input: &mut TextArea<'_>, text: &str, multiline: bool, bytes_max: usize) {
    if bytes(input) + text.len() <= bytes_max {
        if multiline {
            input.insert_str(text);
        } else {
            input.insert_str(text.replace(['\r', '\n'], " "));
        }
    }
}

pub(super) fn bytes(input: &TextArea<'_>) -> usize {
    input.lines().iter().map(String::len).sum::<usize>() + input.lines().len().saturating_sub(1)
}
