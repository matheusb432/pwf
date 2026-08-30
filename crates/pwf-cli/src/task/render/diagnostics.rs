use prost::Message as _;
use pwf_client::{
    ClientError,
    v1::{CreateTaskFailureDetails, CreatedTask},
};

pub(in crate::task) const TITLE_NORMALIZED_NOTICE: &str =
    "info: title normalized to keep metadata valid";

pub(in crate::task) fn emit_created_section(task: &CreatedTask) {
    if let Some(section) = task.created_section.as_ref() {
        eprintln!("info: created `## {section}` section in {}", task.project);
    }
}

pub(in crate::task) fn emit_created_section_for_error(error: &ClientError) {
    if let Some((project, section)) = created_section_for_error(error) {
        eprintln!("info: created `## {section}` section in {project}");
    }
}

fn created_section_for_error(error: &ClientError) -> Option<(String, String)> {
    let ClientError::Rpc(status) = error;
    created_section_from_details(status.details())
}

fn created_section_from_details(details: &[u8]) -> Option<(String, String)> {
    let details = CreateTaskFailureDetails::decode(details).ok()?;
    details
        .created_section
        .map(|section| (details.project, section))
}

#[cfg(test)]
mod tests {
    use pwf_client::v1::CreateTaskFailureDetails;

    use super::*;

    #[test]
    fn add_write_index_error_exposes_created_section_diagnostic_data() {
        let details = CreateTaskFailureDetails {
            project: "foo-bar".to_string(),
            created_section: Some("Human".to_string()),
        };
        assert_eq!(
            created_section_from_details(&details.encode_to_vec()),
            Some(("foo-bar".to_string(), "Human".to_string()))
        );
    }
}
