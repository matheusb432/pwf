//! TOML-backed user settings used by the server process root.

use std::{collections::BTreeMap, path::PathBuf, str::FromStr as _};

use pwf_application::{
    ports::user_settings::{
        TaskBodyPresetReader, UserSettingsConfigurationError, UserSettingsLoadError,
        UserSettingsReader,
    },
    task::body_presets::{
        MarkerSectionDefinition, MarkerSectionDefinitionError, MarkerSectionHeadingLevel,
        MarkerSectionHeadingLevelError, MarkerSectionItemStyle, TaskBodyPreset,
        TaskBodyPresetError, TaskBodyPresets, TaskBodyPresetsError,
    },
};
use pwf_models::{
    project::ProjectId,
    settings::{
        NoteStatusColors, ProjectStatusColors, RgbColor, RgbColorError, TaskStatusColors,
        UserSettings,
    },
    task::{
        PriorityTier, PriorityTierError, TaskListLimit, TaskListLimitError,
        order::{OrderSpec, OrderSpecError},
    },
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UserSettingsDocument {
    colors: ColorsDocument,
    default_list_page_size: Option<usize>,
    default_priority: Option<String>,
    default_sort_order: Option<String>,
    datetime_format: Option<String>,
    task_body: TaskBodyDocument,
}

impl UserSettingsDocument {
    fn parse(bytes: Vec<u8>) -> Result<Self, UserSettingsDocumentError> {
        let source = String::from_utf8(bytes)?;
        toml::from_str(&source).map_err(UserSettingsDocumentError::TomlSchema)
    }

    fn into_validated(
        mut self,
    ) -> Result<(UserSettings, TaskBodyPresets), UserSettingsDocumentError> {
        let task_body = std::mem::take(&mut self.task_body).into_presets()?;
        Ok((self.into_settings()?, task_body))
    }

    fn into_settings(self) -> Result<UserSettings, UserSettingsDocumentError> {
        let default_list_page_size = self
            .default_list_page_size
            .map(TaskListLimit::try_new)
            .transpose()
            .map_err(UserSettingsDocumentError::ListPageSize)?
            .unwrap_or_default();
        let default_priority = self
            .default_priority
            .map(|value| value.parse::<PriorityTier>())
            .transpose()
            .map_err(UserSettingsDocumentError::Priority)?
            .unwrap_or(PriorityTier::Medium);
        let default_sort_order = self
            .default_sort_order
            .map(|value| value.parse::<OrderSpec>())
            .transpose()
            .map_err(UserSettingsDocumentError::Order)?
            .unwrap_or_default();
        let datetime_format = self
            .datetime_format
            .map(|value| value.parse::<pwf_models::settings::DateTimeFormat>())
            .transpose()
            .map_err(UserSettingsDocumentError::DateTimeFormat)?
            .unwrap_or_default();
        Ok(UserSettings::new(
            TaskStatusColors::new(
                configured_color("colors.task.active", self.colors.task.active)?,
                configured_color("colors.task.done", self.colors.task.done)?,
                configured_color("colors.task.cancelled", self.colors.task.cancelled)?,
                configured_color("colors.task.backlog", self.colors.task.backlog)?,
            ),
            ProjectStatusColors::new(
                configured_color("colors.project.active", self.colors.project.active)?,
                configured_color("colors.project.paused", self.colors.project.paused)?,
            ),
            NoteStatusColors::new(
                configured_color("colors.note.active", self.colors.note.active)?,
                configured_color("colors.note.verified", self.colors.note.verified)?,
            ),
            default_priority,
            default_sort_order,
        )
        .with_datetime_format(datetime_format)
        .with_default_list_page_size(default_list_page_size))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ColorsDocument {
    task: TaskStatusColorsDocument,
    project: ProjectStatusColorsDocument,
    note: NoteStatusColorsDocument,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ProjectStatusColorsDocument {
    active: Option<String>,
    paused: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NoteStatusColorsDocument {
    active: Option<String>,
    verified: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TaskStatusColorsDocument {
    active: Option<String>,
    done: Option<String>,
    cancelled: Option<String>,
    backlog: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TaskBodyDocument {
    preset: Option<String>,
    projects: BTreeMap<String, String>,
    presets: BTreeMap<String, TaskBodyPresetDocument>,
}

impl TaskBodyDocument {
    fn into_presets(self) -> Result<TaskBodyPresets, UserSettingsDocumentError> {
        let user_presets = self
            .presets
            .into_iter()
            .map(|(name, preset)| preset.into_preset(name))
            .collect::<Result<Vec<_>, _>>()?;
        let project_presets = self
            .projects
            .into_iter()
            .map(|(project_id, preset)| {
                ProjectId::try_new(&project_id)
                    .map(|id| (id, preset))
                    .map_err(|_| UserSettingsDocumentError::TaskBodyProject { project_id })
            })
            .collect::<Result<Vec<_>, _>>()?;
        TaskBodyPresets::try_new(user_presets, self.preset.as_deref(), project_presets)
            .map_err(UserSettingsDocumentError::TaskBodyPresets)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskBodyPresetDocument {
    sections: Vec<TaskBodySectionDocument>,
}

impl TaskBodyPresetDocument {
    fn into_preset(self, name: String) -> Result<TaskBodyPreset, UserSettingsDocumentError> {
        let sections = self
            .sections
            .into_iter()
            .enumerate()
            .map(|(index, section)| {
                section.into_definition().map_err(|source| {
                    UserSettingsDocumentError::TaskBodySection {
                        preset: name.clone(),
                        index,
                        source,
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        TaskBodyPreset::try_new(name, sections).map_err(UserSettingsDocumentError::TaskBodyPreset)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskBodySectionDocument {
    marker: String,
    title: String,
    #[serde(default = "default_task_body_heading_level")]
    level: u8,
    #[serde(default)]
    items: TaskBodyItemStyleDocument,
}

const fn default_task_body_heading_level() -> u8 {
    2
}

impl TaskBodySectionDocument {
    fn into_definition(self) -> Result<MarkerSectionDefinition, TaskBodySectionError> {
        let heading_level = MarkerSectionHeadingLevel::try_new(self.level)?;
        let item_style = match self.items {
            TaskBodyItemStyleDocument::Bullet => MarkerSectionItemStyle::Bullet,
            TaskBodyItemStyleDocument::Numbered => MarkerSectionItemStyle::Numbered,
            TaskBodyItemStyleDocument::Paragraph => MarkerSectionItemStyle::Paragraph,
        };
        Ok(MarkerSectionDefinition::try_new(
            self.marker,
            self.title,
            heading_level,
            item_style,
        )?)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TaskBodyItemStyleDocument {
    #[default]
    Bullet,
    Numbered,
    Paragraph,
}

#[derive(Debug, thiserror::Error)]
enum TaskBodySectionError {
    #[error(transparent)]
    HeadingLevel(#[from] MarkerSectionHeadingLevelError),
    #[error(transparent)]
    Definition(#[from] MarkerSectionDefinitionError),
}

fn configured_color(
    key: &'static str,
    value: Option<String>,
) -> Result<Option<RgbColor>, UserSettingsDocumentError> {
    value
        .map(|value| {
            RgbColor::from_str(&value)
                .map_err(|source| UserSettingsDocumentError::Color { key, source })
        })
        .transpose()
}

#[derive(Debug, thiserror::Error)]
enum UserSettingsDocumentError {
    #[error("`default_list_page_size` is invalid: {0}")]
    ListPageSize(#[source] TaskListLimitError),
    #[error(transparent)]
    DateTimeFormat(pwf_models::settings::DateTimeFormatError),
    #[error("`default_priority` is invalid: {0}")]
    Priority(#[source] PriorityTierError),
    #[error("`default_sort_order` is invalid: {0}")]
    Order(#[source] OrderSpecError),
    #[error("user settings are not UTF-8: {0}")]
    Encoding(#[from] std::string::FromUtf8Error),
    #[error("user settings TOML schema is invalid: {0}")]
    TomlSchema(#[source] toml::de::Error),
    #[error("`{key}` is invalid: {source}")]
    Color {
        key: &'static str,
        #[source]
        source: RgbColorError,
    },
    #[error("`task_body.presets.{preset}.sections[{index}]` is invalid: {source}")]
    TaskBodySection {
        preset: String,
        index: usize,
        #[source]
        source: TaskBodySectionError,
    },
    #[error(transparent)]
    TaskBodyPreset(TaskBodyPresetError),
    #[error(
        "`task_body.projects` key must be a project ID with two to four ASCII letters: {project_id:?}"
    )]
    TaskBodyProject { project_id: String },
    #[error("`task_body` is invalid: {0}")]
    TaskBodyPresets(#[source] TaskBodyPresetsError),
}

/// TOML-backed user settings for one resolved configuration path.
#[derive(Clone, Debug)]
pub struct TomlSettingsStore {
    path: Option<PathBuf>,
}

impl TomlSettingsStore {
    #[must_use]
    pub const fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    #[must_use]
    pub fn from_environment() -> Self {
        let path = directories::BaseDirs::new()
            .map(|directories| directories.config_dir().join("pwf").join("config.toml"));
        Self::new(path)
    }
}

impl TomlSettingsStore {
    /// Validates the whole document so every settings consumer sees the same accepted file.
    fn load_validated(&self) -> Result<(UserSettings, TaskBodyPresets), UserSettingsLoadError> {
        let Some(path) = self.path.as_deref() else {
            return default_settings();
        };
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return default_settings();
            }
            Err(error) => {
                return Err(anyhow::Error::new(error)
                    .context(format!("reading user settings {}", path.display()))
                    .into());
            }
        };
        let settings = UserSettingsDocument::parse(bytes)
            .and_then(UserSettingsDocument::into_validated)
            .map_err(|source| {
                UserSettingsConfigurationError::new(path.to_path_buf(), anyhow::Error::new(source))
            })?;
        Ok(settings)
    }
}

fn default_settings() -> Result<(UserSettings, TaskBodyPresets), UserSettingsLoadError> {
    let task_body = TaskBodyPresets::try_new(Vec::new(), None, Vec::new()).map_err(|error| {
        anyhow::Error::new(error).context("building built-in task body presets")
    })?;
    Ok((UserSettings::default(), task_body))
}

impl UserSettingsReader for TomlSettingsStore {
    fn load(&self) -> Result<UserSettings, UserSettingsLoadError> {
        self.load_validated().map(|(settings, _)| settings)
    }
}

impl TaskBodyPresetReader for TomlSettingsStore {
    fn load_task_body_presets(&self) -> Result<TaskBodyPresets, UserSettingsLoadError> {
        self.load_validated().map(|(_, task_body)| task_body)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pwf_application::{
        ports::user_settings::{TaskBodyPresetReader, UserSettingsLoadError, UserSettingsReader},
        task::body_presets::MarkerSectionItemStyle,
    };
    use pwf_models::{
        project::ProjectId,
        settings::{RgbColor, UserSettings},
    };

    use super::TomlSettingsStore;

    #[test]
    fn task_body_presets_resolve_user_presets_and_project_overrides() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = TomlSettingsStore::new(Some(path.clone()));
        assert_eq!(
            store.load_task_body_presets().unwrap().selected().name(),
            "default"
        );
        fs::write(
            &path,
            concat!(
                "[task_body]\npreset = \"alt\"\n",
                "[task_body.projects]\npwf = \"prompt\"\n",
                "[task_body.presets.prompt]\nsections = [\n",
                "  { marker = \"/g\", title = \"Goals\", level = 3, items = \"numbered\" },\n",
                "  { marker = \"/c\", title = \"Context\", items = \"paragraph\" },\n",
                "  { marker = \"/n\", title = \"Constraints\" },\n",
                "]\n",
            ),
        )
        .unwrap();
        let presets = store.load_task_body_presets().unwrap();
        assert_eq!(presets.selected().name(), "alt");
        let prompt = presets.for_project(&ProjectId::try_new("PWF").unwrap());
        assert_eq!(prompt.name(), "prompt");
        assert_eq!(
            prompt
                .sections()
                .iter()
                .map(|section| (
                    section.marker(),
                    section.header(),
                    section.heading_level().get(),
                    section.item_style()
                ))
                .collect::<Vec<_>>(),
            [
                ("/g", "Goals", 3, MarkerSectionItemStyle::Numbered),
                ("/c", "Context", 2, MarkerSectionItemStyle::Paragraph),
                ("/n", "Constraints", 2, MarkerSectionItemStyle::Bullet),
            ]
        );
        assert_eq!(store.load().unwrap(), UserSettings::default());
    }

    #[test]
    fn invalid_task_body_documents_reject_every_settings_read() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = TomlSettingsStore::new(Some(path.clone()));
        let section =
            "[task_body.presets.mine]\nsections = [{ marker = \"/g\", title = \"Goals\" }]\n";
        for source in [
            "[task_body]\npreset = \"missing\"\n".to_string(),
            "[task_body]\nunknown = true\n".to_string(),
            "[task_body.projects]\npwf = \"missing\"\n".to_string(),
            "[task_body.projects]\npwfxx = \"alt\"\n".to_string(),
            "[task_body.presets.alt]\nsections = [{ marker = \"/g\", title = \"Goals\" }]\n"
                .to_string(),
            "[task_body.presets.mine]\nsections = []\n".to_string(),
            "[task_body.presets.mine]\nsections = [{ marker = \"g\", title = \"Goals\" }]\n"
                .to_string(),
            "[task_body.presets.mine]\nsections = [{ marker = \"/g\", title = \"Report\" }]\n"
                .to_string(),
            section.replace("\" }", "\", level = 7 }"),
            section.replace("\" }", "\", items = \"checkbox\" }"),
            section.replace("\" }", "\", unknown = 1 }"),
            format!(
                "{}\n",
                section.replace("}]", "}, { marker = \"/g\", title = \"Context\" }]")
            ),
        ] {
            fs::write(&path, &source).unwrap();
            for error in [
                store.load().unwrap_err(),
                store.load_task_body_presets().unwrap_err(),
            ] {
                assert!(
                    matches!(error, UserSettingsLoadError::InvalidConfiguration(_)),
                    "{source}: {error:?}"
                );
            }
        }
    }

    #[test]
    fn list_page_size_accepts_supported_limits_and_reloads() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = TomlSettingsStore::new(Some(path.clone()));
        assert_eq!(store.load().unwrap().default_list_page_size().get(), 10);
        for size in [1, 20, 300, 100_000] {
            fs::write(&path, format!("default_list_page_size = {size}\n")).unwrap();
            assert_eq!(store.load().unwrap().default_list_page_size().get(), size);
        }
    }

    #[test]
    fn list_settings_accept_every_cli_sort_field_and_direction() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = TomlSettingsStore::new(Some(path.clone()));
        for field in ["id", "created", "project-id", "priority", "effort", "title"] {
            for suffix in ["", ":asc", ":desc"] {
                fs::write(
                    &path,
                    format!(
                        "default_priority = \"high\"\ndefault_sort_order = \"{field}{suffix}\"\n"
                    ),
                )
                .unwrap();
                let settings = store.load().unwrap();
                assert_eq!(
                    settings.default_priority(),
                    pwf_models::task::PriorityTier::High
                );
                assert_eq!(
                    settings.default_sort_order(),
                    format!("{field}{suffix}").parse().unwrap()
                );
            }
        }
    }

    #[test]
    fn partial_document_overrides_only_the_configured_status_color() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "[colors.task]\nactive = \"#ff8700\"\n").unwrap();
        let store = TomlSettingsStore::new(Some(path));

        let colors = store.load().unwrap().task_status_colors();

        assert_eq!(colors.active(), RgbColor::new(255, 135, 0));
        assert_eq!(colors.done(), RgbColor::new(163, 230, 53));
        assert_eq!(colors.cancelled(), RgbColor::new(255, 107, 138));
        assert_eq!(colors.backlog(), RgbColor::new(234, 179, 8));
    }

    #[test]
    fn backlog_color_accepts_an_override_and_rejects_invalid_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = TomlSettingsStore::new(Some(path.clone()));
        fs::write(&path, "[colors.task]\nbacklog = \"#102030\"\n").unwrap();
        assert_eq!(
            store.load().unwrap().task_status_colors().backlog(),
            RgbColor::new(16, 32, 48)
        );
        fs::write(&path, "[colors.task]\nbacklog = \"gold\"\n").unwrap();
        let error = store.load().unwrap_err();
        assert!(error.to_string().contains("colors.task.backlog"));
    }

    #[test]
    fn scoped_colors_override_each_object_independently() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(
            &path,
            concat!(
                "[colors.task]\nactive = \"#010203\"\nbacklog = \"#101112\"\n",
                "[colors.project]\nactive = \"#040506\"\npaused = \"#070809\"\n",
                "[colors.note]\nactive = \"#0a0b0c\"\nverified = \"#0d0e0f\"\n",
            ),
        )
        .unwrap();
        let settings = TomlSettingsStore::new(Some(path)).load().unwrap();
        assert_eq!(
            settings.task_status_colors().active(),
            RgbColor::new(1, 2, 3)
        );
        assert_eq!(
            settings.project_status_colors().active(),
            RgbColor::new(4, 5, 6)
        );
        assert_eq!(
            settings.project_status_colors().paused(),
            RgbColor::new(7, 8, 9)
        );
        assert_eq!(
            settings.note_status_colors().active(),
            RgbColor::new(10, 11, 12)
        );
        assert_eq!(
            settings.note_status_colors().verified(),
            RgbColor::new(13, 14, 15)
        );
    }

    #[test]
    fn missing_document_returns_defaults_without_creating_a_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing").join("config.toml");
        let store = TomlSettingsStore::new(Some(path.clone()));

        assert_eq!(store.load().unwrap(), UserSettings::default());
        assert!(!path.exists());

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for document in [
            "",
            "[colors]\n",
            "[colors.task]\n[colors.project]\n[colors.note]\n",
        ] {
            fs::write(&path, document).unwrap();
            assert_eq!(store.load().unwrap(), UserSettings::default());
        }
    }

    #[test]
    fn invalid_documents_report_their_path_and_cause() -> anyhow::Result<()> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = TomlSettingsStore::new(Some(path.clone()));

        for source in [
            "unknown = true\n",
            "default_list_page_size = 0\n",
            "default_list_page_size = -1\n",
            "default_list_page_size = 100001\n",
            "default_list_page_size = 1.5\n",
            "default_list_page_size = \"20\"\n",
            "default_list_page_size = false\n",
            "default_priority = \"urgent\"\n",
            "default_priority = 3\n",
            "datetime_format = \"%J\"\n",
            "datetime_format = \"\"\n",
            "datetime_format = 3\n",
            "default_sort_order = \"title:sideways\"\n",
            "default_sort_order = \"title:asc:desc\"\n",
            "default_sort_order = false\n",
            "[colors.task]\nunknown = \"#ffffff\"\n",
            "[colors.task]\nactive = \"#fff\"\n",
            "[colors.task]\nactive = 42\n",
            "[colors]\nactive = \"#ffffff\"\n",
            "[colors.project]\npaused = \"#gg0000\"\n",
            "[colors.project]\ndone = \"#ffffff\"\n",
            "[colors.note]\nverified = 42\n",
            "[colors.note]\narchived = \"#ffffff\"\n",
        ] {
            fs::write(&path, source).unwrap();
            let error = store.load().unwrap_err();
            let UserSettingsLoadError::InvalidConfiguration(error) = error else {
                anyhow::bail!("expected invalid configuration, got {error:?}");
            };

            assert_eq!(error.path(), path);
            assert!(error.to_string().contains("config.toml"));
        }

        Ok(())
    }

    #[test]
    fn each_load_reads_the_latest_document() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = TomlSettingsStore::new(Some(path.clone()));
        fs::write(&path, "[colors.task]\nactive = \"#010203\"\n").unwrap();
        assert_eq!(
            store.load().unwrap().task_status_colors().active(),
            RgbColor::new(1, 2, 3)
        );

        fs::write(&path, "[colors.task]\nactive = \"#040506\"\n").unwrap();
        assert_eq!(
            store.load().unwrap().task_status_colors().active(),
            RgbColor::new(4, 5, 6)
        );
    }
}
