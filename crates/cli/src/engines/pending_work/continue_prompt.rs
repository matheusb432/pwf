// Builders for the "continue" task prompts. Relocated from the route word-grammar
// (PWF-0034) so they survive only as explicit `add` flags. The output strings are
// asserted byte-for-byte by Rust tests — do not reword them casually.

use super::{
    naming::pathdiff_forward,
    parse::newest_handoff_typed,
    text::{get_title_from_continue_path, handoff_title_from_path},
};

/// `(title, prompt)` for `add <project> --continue-handoff`: continue the repo's
/// newest handoff. `repo` is the project's mapped repo root.
pub(super) fn continue_handoff_prompt(
    repo: &str,
) -> Result<(String, String), super::errors::PendingWorkError> {
    let handoff = newest_handoff_typed(repo)?;
    let rel = pathdiff_forward(repo, &handoff); // forward-slash relative path
    let title = handoff_title_from_path(&handoff.to_string_lossy());
    let prompt = format!("Continue the handoff at @{rel}.");
    Ok((title, prompt))
}

/// `(title, prompt)` for `add <project> --continue <path>`: continue the plan at
/// `path`.
pub(super) fn continue_plan_prompt(project_name: &str, path: &str) -> (String, String) {
    let title = get_title_from_continue_path(project_name, path);
    let prompt = format!("continue the plan at {path}");
    (title, prompt)
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, fs};

    use super::*;
    use crate::engines::pending_work::errors::PendingWorkError;

    #[test]
    fn continue_plan_prompt_reproduces_legacy_strings() {
        let (title, prompt) =
            continue_plan_prompt("glep-shimeji", "docs/plans/2026-01-01-make-it-sleep.md");
        assert_eq!(title, "glep shimeji make it sleep");
        assert_eq!(
            prompt,
            "continue the plan at docs/plans/2026-01-01-make-it-sleep.md"
        );
    }

    #[test]
    fn continue_handoff_prompt_reproduces_legacy_strings() {
        let repo = tempfile::tempdir().unwrap();
        let handoff_dir = repo.path().join("docs").join("handoffs");
        fs::create_dir_all(&handoff_dir).unwrap();
        fs::write(handoff_dir.join("2026-01-01-api-cleanup.md"), "body\n").unwrap();

        let (title, prompt) = continue_handoff_prompt(&repo.path().to_string_lossy()).unwrap();
        assert_eq!(title, "continue api cleanup");
        assert_eq!(
            prompt,
            "Continue the handoff at @docs/handoffs/2026-01-01-api-cleanup.md."
        );
    }

    #[test]
    fn missing_handoff_directory_returns_typed_error_with_legacy_display() {
        let guard = tempfile::tempdir().unwrap();
        let repo = guard.path().join("missing_repo");
        let expected = repo.join("docs").join("handoffs");

        let err = continue_handoff_prompt(&repo.to_string_lossy()).unwrap_err();

        assert_matches!(
            err,
            PendingWorkError::NoHandoffDirectory { ref path } if path == &expected
        );
        assert_eq!(
            err.to_string(),
            format!(
                "No handoff directory found for project at {}.",
                expected.display()
            )
        );
    }
}
