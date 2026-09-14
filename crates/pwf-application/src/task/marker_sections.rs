use pwf_marker_sections::{
    Adapter as _, MarkdownAdapter, MarkerSectionConfiguration, MarkerSectionConfigurationError,
    MarkerSectionDefinition, MarkerSectionDefinitionError, ParsedMarkerSections,
};
use pwf_wire::task::TaskMarkerSection;

const TASK_MARKER_SECTION_COUNT: usize = 4;
const TASK_MARKER_SECTION_NAMES: [&str; TASK_MARKER_SECTION_COUNT] =
    ["goals", "context", "constraints", "done_when"];

/// Reports an unreadable or invalid persisted task-body section configuration.
#[derive(Debug, thiserror::Error)]
pub enum TaskMarkerSectionsError {
    #[error("cannot read task body section configuration: {0}")]
    Database(#[source] anyhow::Error),
    #[error(
        "task body section configuration must contain {expected:?} in that order; found {actual:?}"
    )]
    InvalidSectionSet {
        expected: [&'static str; TASK_MARKER_SECTION_COUNT],
        actual: Vec<String>,
    },
    #[error("invalid task body section {section:?}: {source}")]
    InvalidSection {
        section: String,
        #[source]
        source: MarkerSectionDefinitionError,
    },
    #[error("invalid task body section configuration: {0}")]
    InvalidConfiguration(#[from] MarkerSectionConfigurationError),
    #[error("task body section configuration produced {actual} definitions; expected {expected}")]
    InvalidDefinitionCount { expected: usize, actual: usize },
}

#[derive(Debug)]
pub struct TaskMarkerSections {
    configuration: MarkerSectionConfiguration<TASK_MARKER_SECTION_COUNT>,
}

pub struct TaskMarkerSectionRow {
    pub section: String,
    pub marker: String,
    pub header: String,
}

impl TaskMarkerSections {
    pub fn try_from_rows(rows: Vec<TaskMarkerSectionRow>) -> Result<Self, TaskMarkerSectionsError> {
        let actual = rows
            .iter()
            .map(|row| row.section.clone())
            .collect::<Vec<_>>();
        if actual.as_slice() != TASK_MARKER_SECTION_NAMES {
            return Err(TaskMarkerSectionsError::InvalidSectionSet {
                expected: TASK_MARKER_SECTION_NAMES,
                actual,
            });
        }
        let mut definitions = Vec::with_capacity(TASK_MARKER_SECTION_COUNT);
        for row in rows {
            definitions.push(marker_section_definition(
                row.section,
                row.marker,
                row.header,
            )?);
        }
        let definition_count = definitions.len();
        let sections = definitions.try_into().map_err(|_| {
            TaskMarkerSectionsError::InvalidDefinitionCount {
                expected: TASK_MARKER_SECTION_COUNT,
                actual: definition_count,
            }
        })?;
        Ok(Self {
            configuration: MarkerSectionConfiguration::try_new(sections)?,
        })
    }

    pub(super) fn parse(&self, body: &str) -> ParsedMarkerSections<TASK_MARKER_SECTION_COUNT> {
        pwf_marker_sections::parse(body, &self.configuration)
    }

    pub(super) fn render(
        &self,
        parsed: &ParsedMarkerSections<TASK_MARKER_SECTION_COUNT>,
    ) -> String {
        MarkdownAdapter::new(&self.configuration).render(parsed)
    }

    pub(super) fn header(&self, section: TaskMarkerSection) -> &str {
        self.configuration.sections()[marker_section_index(section)].header()
    }

    #[cfg(test)]
    pub(super) fn default_fixture() -> Self {
        Self {
            configuration: MarkerSectionConfiguration::try_new([
                MarkerSectionDefinition::try_new("/g", "Goals").unwrap(),
                MarkerSectionDefinition::try_new("/c", "Context").unwrap(),
                MarkerSectionDefinition::try_new("/n", "Constraints").unwrap(),
                MarkerSectionDefinition::try_new("/d", "Done When").unwrap(),
            ])
            .unwrap(),
        }
    }
}

fn marker_section_definition(
    section: String,
    marker: String,
    header: String,
) -> Result<MarkerSectionDefinition, TaskMarkerSectionsError> {
    MarkerSectionDefinition::try_new(marker, header)
        .map_err(|source| TaskMarkerSectionsError::InvalidSection { section, source })
}

const fn marker_section_index(section: TaskMarkerSection) -> usize {
    match section {
        TaskMarkerSection::Goal => 0,
        TaskMarkerSection::Context => 1,
        TaskMarkerSection::Constraint => 2,
        TaskMarkerSection::DoneWhen => 3,
    }
}
