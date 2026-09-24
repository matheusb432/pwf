use std::fmt::Write as _;

use anyhow::Context as _;
use clap::Args;
use pwf_client::{
    pb::{GetTaskBodySectionsRequest, GetTaskBodySectionsResponse, TaskBodyItemStyle},
    task::TaskClient,
};
use pwf_models::project::ProjectId;
use serde::Serialize;

const SHORTHAND_RULES: &str = "Text before the first marker is the title. `/` starts another item in the current section; text after it goes to the first section until a marker selects another.";

#[derive(Args, Debug)]
pub struct Arguments {
    /// Project whose preset applies; omit it to show the global preset.
    #[arg(value_name = "PROJECT", value_parser = crate::project::parse_project_id)]
    project: Option<ProjectId>,
    /// Write the preset as one JSON object.
    #[arg(long)]
    json: bool,
}

pub(super) async fn run(arguments: &Arguments, client: &TaskClient) -> anyhow::Result<String> {
    let response = client
        .get_task_body_sections(GetTaskBodySectionsRequest {
            project_id: arguments.project.as_ref().map(ToString::to_string),
        })
        .await
        .map_err(crate::rpc_error)?;
    let sections = TaskBodySectionsOutput::try_from(response)?;
    if arguments.json {
        return serde_json::to_string(&sections).context("serializing task body sections");
    }
    Ok(render(&sections))
}

#[derive(Serialize)]
struct TaskBodySectionsOutput {
    preset: String,
    sections: Vec<TaskBodySectionOutput>,
}

#[derive(Serialize)]
struct TaskBodySectionOutput {
    marker: String,
    header: String,
    heading_level: u32,
    item_style: &'static str,
}

impl TryFrom<GetTaskBodySectionsResponse> for TaskBodySectionsOutput {
    type Error = anyhow::Error;

    fn try_from(response: GetTaskBodySectionsResponse) -> anyhow::Result<Self> {
        let sections = response
            .sections
            .into_iter()
            .map(|section| {
                let item_style = match TaskBodyItemStyle::try_from(section.item_style) {
                    Ok(TaskBodyItemStyle::Bullet) => "bullet",
                    Ok(TaskBodyItemStyle::Numbered) => "numbered",
                    Ok(TaskBodyItemStyle::Paragraph) => "paragraph",
                    Ok(TaskBodyItemStyle::Unspecified) | Err(_) => {
                        anyhow::bail!(
                            "pwf-server returned an unknown item style for section {:?}",
                            section.header
                        )
                    }
                };
                Ok(TaskBodySectionOutput {
                    marker: section.marker,
                    header: section.header,
                    heading_level: section.heading_level,
                    item_style,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            preset: response.preset,
            sections,
        })
    }
}

fn render(sections: &TaskBodySectionsOutput) -> String {
    let heading_width = sections
        .sections
        .iter()
        .map(|section| heading(section).chars().count())
        .max()
        .unwrap_or(0);
    let mut output = format!("Preset: {}\n\n", sections.preset);
    for section in &sections.sections {
        let _ = writeln!(
            output,
            "{}  {:<heading_width$}  {}",
            section.marker,
            heading(section),
            section.item_style
        );
    }
    output.push('\n');
    output.push_str(SHORTHAND_RULES);
    output
}

fn heading(section: &TaskBodySectionOutput) -> String {
    let level = usize::try_from(section.heading_level)
        .unwrap_or(usize::MAX)
        .min(6);
    format!("{} {}", "#".repeat(level), section.header)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_output_aligns_markers_headings_and_styles() {
        let sections = TaskBodySectionsOutput {
            preset: "alt".to_string(),
            sections: vec![
                TaskBodySectionOutput {
                    marker: "/g".to_string(),
                    header: "Goals".to_string(),
                    heading_level: 1,
                    item_style: "bullet",
                },
                TaskBodySectionOutput {
                    marker: "/c".to_string(),
                    header: "Context".to_string(),
                    heading_level: 2,
                    item_style: "paragraph",
                },
            ],
        };
        assert_eq!(
            render(&sections),
            format!(
                "Preset: alt\n\n/g  # Goals     bullet\n/c  ## Context  paragraph\n\n{SHORTHAND_RULES}"
            )
        );
    }
}
