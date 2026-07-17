//! Newest-first `<ID> :: <message>` list rendering.

use std::fmt::Write;

use crate::store::Note;

/// Renders up to `cap` notes with a `-n 0` hint when entries are hidden.
///
/// A zero cap renders every note.
pub fn render_list(notes: &[Note], cap: usize) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let shown = if cap == 0 {
        notes.len()
    } else {
        cap.min(notes.len())
    };
    let mut out = String::new();
    for note in &notes[..shown] {
        let _ = writeln!(out, "{} :: {}", note.id, note.message);
    }
    let hidden = notes.len() - shown;
    if hidden > 0 {
        let _ = writeln!(
            out,
            "... and {hidden} more; run 'pwf note <proj> ls -n 0' to show all"
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(n: u32, msg: &str) -> Note {
        Note {
            id: format!("PWF-NOTE-{n:04}"),
            number: n,
            message: msg.to_string(),
        }
    }

    #[test]
    fn renders_id_and_message_lines() {
        let notes = vec![note(2, "second"), note(1, "first")];
        assert_eq!(
            render_list(&notes, 10),
            "PWF-NOTE-0002 :: second\nPWF-NOTE-0001 :: first\n"
        );
    }

    #[test]
    fn caps_and_footers() {
        let notes = vec![note(3, "c"), note(2, "b"), note(1, "a")];
        let out = render_list(&notes, 2);
        assert!(out.contains("PWF-NOTE-0003 :: c"));
        assert!(out.contains("1 more"), "got: {out}");
        assert!(!out.contains("PWF-NOTE-0001"), "got: {out}");
    }

    #[test]
    fn zero_cap_shows_all() {
        let notes = vec![note(2, "b"), note(1, "a")];
        let out = render_list(&notes, 0);
        assert!(out.contains("PWF-NOTE-0001"));
        assert!(!out.contains("more"));
    }
}
