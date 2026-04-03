// Builders for the "continue" task prompts. Relocated from the route word-grammar
// (PWF-0034) so they survive only as explicit `add` flags. The output strings are
// asserted byte-for-byte by the conformance handoff fixture — do not reword them.

use super::naming::pathdiff_forward;
use super::parse::newest_handoff;
use super::text::{get_title_from_continue_path, handoff_title_from_path};

/// `(title, prompt)` for `add <project> --continue-handoff`: continue the repo's
/// newest handoff. `repo` is the project's mapped repo root.
pub(super) fn continue_handoff_prompt(repo: &str) -> Result<(String, String), String> {
    let handoff = newest_handoff(repo)?;
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
    use super::*;
    use std::fs;

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

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
        let repo = std::env::temp_dir().join(format!("pwcontinue_{}", nanos()));
        let handoff_dir = repo.join("docs").join("handoffs");
        fs::create_dir_all(&handoff_dir).unwrap();
        fs::write(handoff_dir.join("2026-01-01-api-cleanup.md"), "body\n").unwrap();

        let (title, prompt) = continue_handoff_prompt(&repo.to_string_lossy()).unwrap();
        assert_eq!(title, "continue api cleanup");
        assert_eq!(
            prompt,
            "Continue the handoff at @docs/handoffs/2026-01-01-api-cleanup.md."
        );

        fs::remove_dir_all(&repo).ok();
    }
}
