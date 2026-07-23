use pwf_application::handoff::HandoffMutationOk;

pub(in crate::engines::pending_work) fn append_handoff_outcome(
    text: String,
    handoff: &HandoffMutationOk,
    label: &str,
) -> String {
    match handoff {
        HandoffMutationOk::Archived { path } | HandoffMutationOk::Reopened { path } => {
            format!("{text}\n  handoff: {label} {}", path.display())
        }
        _ => text,
    }
}
