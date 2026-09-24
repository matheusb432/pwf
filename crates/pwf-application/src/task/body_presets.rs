//! Named task-body presets that map shorthand markers to rendered Markdown sections.

use std::collections::BTreeMap;

use pwf_marker_sections::{
    Adapter as _, MarkdownAdapter, MarkerSectionConfiguration, MarkerSectionConfigurationError,
    ParsedMarkerSections,
};
pub use pwf_marker_sections::{
    MarkerSectionDefinition, MarkerSectionDefinitionError, MarkerSectionHeadingLevel,
    MarkerSectionHeadingLevelError, MarkerSectionItemStyle,
};
use pwf_models::project::ProjectId;

/// Names the built-in preset selected when settings choose none.
pub const DEFAULT_TASK_BODY_PRESET_NAME: &str = "default";
const PRESET_NAME_CHARACTER_LIMIT: usize = 64;
/// `pwf task done` appends this level-three heading, so sections cannot claim it.
const REPORT_SECTION_HEADER: &str = "Report";

struct BuiltInSection {
    marker: &'static str,
    header: &'static str,
    heading_level: u8,
    item_style: MarkerSectionItemStyle,
}

const BUILT_IN_PRESETS: [(&str, &[BuiltInSection]); 2] = [
    (
        DEFAULT_TASK_BODY_PRESET_NAME,
        &[
            BuiltInSection {
                marker: "/g",
                header: "Goals",
                heading_level: 2,
                item_style: MarkerSectionItemStyle::Bullet,
            },
            BuiltInSection {
                marker: "/c",
                header: "Context",
                heading_level: 2,
                item_style: MarkerSectionItemStyle::Bullet,
            },
            BuiltInSection {
                marker: "/n",
                header: "Constraints",
                heading_level: 2,
                item_style: MarkerSectionItemStyle::Bullet,
            },
            BuiltInSection {
                marker: "/d",
                header: "Done When",
                heading_level: 2,
                item_style: MarkerSectionItemStyle::Bullet,
            },
        ],
    ),
    (
        "alt",
        &[
            BuiltInSection {
                marker: "/g",
                header: "Goals",
                heading_level: 1,
                item_style: MarkerSectionItemStyle::Bullet,
            },
            BuiltInSection {
                marker: "/c",
                header: "Context",
                heading_level: 2,
                item_style: MarkerSectionItemStyle::Paragraph,
            },
        ],
    ),
];

/// Holds one named, validated section layout for task bodies.
#[derive(Debug, Clone)]
pub struct TaskBodyPreset {
    name: String,
    configuration: MarkerSectionConfiguration,
}

impl TaskBodyPreset {
    /// Constructs a preset whose sections have unique markers and headers.
    ///
    /// # Errors
    ///
    /// Returns [`TaskBodyPresetError`] for an invalid name, an empty or ambiguous section set, or
    /// a section that claims the completion-report heading.
    pub fn try_new(
        name: impl Into<String>,
        sections: Vec<MarkerSectionDefinition>,
    ) -> Result<Self, TaskBodyPresetError> {
        let name = name.into();
        if !is_preset_name(&name) {
            return Err(TaskBodyPresetError::InvalidName { name });
        }
        if sections
            .iter()
            .any(|section| section.header() == REPORT_SECTION_HEADER)
        {
            return Err(TaskBodyPresetError::ReservedHeader { preset: name });
        }
        match MarkerSectionConfiguration::try_new(sections) {
            Ok(configuration) => Ok(Self {
                name,
                configuration,
            }),
            Err(source) => Err(TaskBodyPresetError::InvalidSections {
                preset: name,
                source,
            }),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns sections in rendering order; the first one receives text after bare `/`.
    #[must_use]
    pub fn sections(&self) -> &[MarkerSectionDefinition] {
        self.configuration.sections()
    }

    pub(super) fn parse(&self, body: &str) -> ParsedMarkerSections {
        pwf_marker_sections::parse(body, &self.configuration)
    }

    pub(super) fn render(&self, parsed: &ParsedMarkerSections) -> String {
        MarkdownAdapter::new(&self.configuration).render(parsed)
    }
}

fn is_preset_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= PRESET_NAME_CHARACTER_LIMIT
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// Reports one invalid task-body preset.
#[derive(Debug, thiserror::Error)]
pub enum TaskBodyPresetError {
    #[error(
        "task body preset name must contain 1 to {PRESET_NAME_CHARACTER_LIMIT} ASCII letters, digits, `-`, or `_`: {name:?}"
    )]
    InvalidName { name: String },
    #[error("task body preset {preset:?} cannot use the reserved `{REPORT_SECTION_HEADER}` header")]
    ReservedHeader { preset: String },
    #[error("task body preset {preset:?} is invalid: {source}")]
    InvalidSections {
        preset: String,
        #[source]
        source: MarkerSectionConfigurationError,
    },
}

/// Holds built-in and user presets with the global and per-project selections.
#[derive(Debug, Clone)]
pub struct TaskBodyPresets {
    presets: Vec<TaskBodyPreset>,
    selected_preset_index: usize,
    project_preset_indexes: BTreeMap<ProjectId, usize>,
}

impl TaskBodyPresets {
    /// Resolves preset selections against the built-in and user presets.
    ///
    /// An omitted global selection chooses [`DEFAULT_TASK_BODY_PRESET_NAME`].
    ///
    /// # Errors
    ///
    /// Returns [`TaskBodyPresetsError`] when a user preset reuses a preset name or a selection
    /// names an unknown preset or repeats a project.
    pub fn try_new(
        user_presets: Vec<TaskBodyPreset>,
        selected_preset_name: Option<&str>,
        project_preset_names: Vec<(ProjectId, String)>,
    ) -> Result<Self, TaskBodyPresetsError> {
        let mut presets = built_in_presets()?;
        let built_in_count = presets.len();
        for preset in user_presets {
            match preset_index(&presets, preset.name()) {
                Some(index) if index < built_in_count => {
                    return Err(TaskBodyPresetsError::BuiltInName { name: preset.name });
                }
                Some(_) => return Err(TaskBodyPresetsError::DuplicateName { name: preset.name }),
                None => presets.push(preset),
            }
        }
        let selected_preset_index = resolve_preset_index(
            &presets,
            selected_preset_name.unwrap_or(DEFAULT_TASK_BODY_PRESET_NAME),
        )?;
        let mut project_preset_indexes = BTreeMap::new();
        for (project_id, preset_name) in project_preset_names {
            let index = resolve_preset_index(&presets, &preset_name)?;
            if project_preset_indexes.contains_key(&project_id) {
                return Err(TaskBodyPresetsError::DuplicateProject { project_id });
            }
            project_preset_indexes.insert(project_id, index);
        }
        Ok(Self {
            presets,
            selected_preset_index,
            project_preset_indexes,
        })
    }

    /// Returns the project's override, or the global selection when it has none.
    #[must_use]
    pub fn for_project(&self, project_id: &ProjectId) -> &TaskBodyPreset {
        let index = self
            .project_preset_indexes
            .get(project_id)
            .copied()
            .unwrap_or(self.selected_preset_index);
        &self.presets[index]
    }

    /// Returns the global selection.
    #[must_use]
    pub fn selected(&self) -> &TaskBodyPreset {
        &self.presets[self.selected_preset_index]
    }
}

fn built_in_presets() -> Result<Vec<TaskBodyPreset>, TaskBodyPresetsError> {
    BUILT_IN_PRESETS
        .iter()
        .map(|(name, sections)| {
            let sections = sections
                .iter()
                .map(|section| {
                    let heading_level =
                        MarkerSectionHeadingLevel::try_new(section.heading_level)
                            .map_err(TaskBodyPresetsError::InvalidBuiltInHeadingLevel)?;
                    MarkerSectionDefinition::try_new(
                        section.marker,
                        section.header,
                        heading_level,
                        section.item_style,
                    )
                    .map_err(TaskBodyPresetsError::InvalidBuiltInSection)
                })
                .collect::<Result<Vec<_>, _>>()?;
            TaskBodyPreset::try_new(*name, sections).map_err(TaskBodyPresetsError::InvalidBuiltIn)
        })
        .collect()
}

fn preset_index(presets: &[TaskBodyPreset], name: &str) -> Option<usize> {
    presets.iter().position(|preset| preset.name() == name)
}

fn resolve_preset_index(
    presets: &[TaskBodyPreset],
    name: &str,
) -> Result<usize, TaskBodyPresetsError> {
    preset_index(presets, name).ok_or_else(|| TaskBodyPresetsError::UnknownPreset {
        name: name.to_string(),
    })
}

/// Reports an inconsistent set of task-body presets or selections.
#[derive(Debug, thiserror::Error)]
pub enum TaskBodyPresetsError {
    #[error("task body preset {name:?} is built in and cannot be redefined")]
    BuiltInName { name: String },
    #[error("task body preset {name:?} is defined more than once")]
    DuplicateName { name: String },
    #[error("unknown task body preset {name:?}")]
    UnknownPreset { name: String },
    #[error("project {project_id} selects more than one task body preset")]
    DuplicateProject { project_id: ProjectId },
    #[error("built-in task body preset has an invalid heading level: {0}")]
    InvalidBuiltInHeadingLevel(#[source] MarkerSectionHeadingLevelError),
    #[error("built-in task body preset has an invalid section: {0}")]
    InvalidBuiltInSection(#[source] MarkerSectionDefinitionError),
    #[error(transparent)]
    InvalidBuiltIn(TaskBodyPresetError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(marker: &str, header: &str) -> MarkerSectionDefinition {
        MarkerSectionDefinition::try_new(
            marker,
            header,
            MarkerSectionHeadingLevel::try_new(3).unwrap(),
            MarkerSectionItemStyle::Numbered,
        )
        .unwrap()
    }

    fn project_id(value: &str) -> ProjectId {
        ProjectId::try_new(value).unwrap()
    }

    fn layout(preset: &TaskBodyPreset) -> Vec<(&str, &str, u8, MarkerSectionItemStyle)> {
        preset
            .sections()
            .iter()
            .map(|section| {
                (
                    section.marker(),
                    section.header(),
                    section.heading_level().get(),
                    section.item_style(),
                )
            })
            .collect()
    }

    #[test]
    fn built_in_presets_define_default_and_alt_layouts() {
        let presets = TaskBodyPresets::try_new(Vec::new(), None, Vec::new()).unwrap();
        assert_eq!(presets.selected().name(), "default");
        assert_eq!(
            layout(presets.selected()),
            [
                ("/g", "Goals", 2, MarkerSectionItemStyle::Bullet),
                ("/c", "Context", 2, MarkerSectionItemStyle::Bullet),
                ("/n", "Constraints", 2, MarkerSectionItemStyle::Bullet),
                ("/d", "Done When", 2, MarkerSectionItemStyle::Bullet),
            ]
        );
        let alt = TaskBodyPresets::try_new(Vec::new(), Some("alt"), Vec::new()).unwrap();
        assert_eq!(
            layout(alt.selected()),
            [
                ("/g", "Goals", 1, MarkerSectionItemStyle::Bullet),
                ("/c", "Context", 2, MarkerSectionItemStyle::Paragraph),
            ]
        );
    }

    #[test]
    fn project_selection_overrides_the_global_preset() {
        let user = TaskBodyPreset::try_new("prompt_v2", vec![section("/o", "Objectives")]).unwrap();
        let presets = TaskBodyPresets::try_new(
            vec![user],
            Some("alt"),
            vec![(project_id("pwf"), "prompt_v2".to_string())],
        )
        .unwrap();
        assert_eq!(presets.for_project(&project_id("PWF")).name(), "prompt_v2");
        assert_eq!(presets.for_project(&project_id("aux")).name(), "alt");
    }

    #[test]
    fn invalid_presets_and_selections_are_rejected() {
        assert!(matches!(
            TaskBodyPreset::try_new("has space", vec![section("/g", "Goals")]),
            Err(TaskBodyPresetError::InvalidName { .. })
        ));
        assert!(matches!(
            TaskBodyPreset::try_new("mine", vec![section("/r", "Report")]),
            Err(TaskBodyPresetError::ReservedHeader { .. })
        ));
        assert!(matches!(
            TaskBodyPreset::try_new("mine", Vec::new()),
            Err(TaskBodyPresetError::InvalidSections { .. })
        ));
        let shadow = TaskBodyPreset::try_new("alt", vec![section("/g", "Goals")]).unwrap();
        assert!(matches!(
            TaskBodyPresets::try_new(vec![shadow], None, Vec::new()),
            Err(TaskBodyPresetsError::BuiltInName { .. })
        ));
        assert!(matches!(
            TaskBodyPresets::try_new(Vec::new(), Some("missing"), Vec::new()),
            Err(TaskBodyPresetsError::UnknownPreset { .. })
        ));
        assert!(matches!(
            TaskBodyPresets::try_new(
                Vec::new(),
                None,
                vec![(project_id("pwf"), "missing".to_string())]
            ),
            Err(TaskBodyPresetsError::UnknownPreset { .. })
        ));
        assert!(matches!(
            TaskBodyPresets::try_new(
                Vec::new(),
                None,
                vec![
                    (project_id("pwf"), "alt".to_string()),
                    (project_id("PWF"), "default".to_string()),
                ]
            ),
            Err(TaskBodyPresetsError::DuplicateProject { .. })
        ));
    }
}
