use pwf_application::handoff::HandoffMutationOutcome;

pub(in crate::engines::pending_work) fn append_handoff_outcome(
    text: String,
    handoff: &HandoffMutationOutcome,
    label: &str,
) -> String {
    match handoff {
        HandoffMutationOutcome::Archived { path } | HandoffMutationOutcome::Reopened { path } => {
            format!("{text}\n  handoff: {label} {}", path.display())
        }
        _ => text,
    }
}
