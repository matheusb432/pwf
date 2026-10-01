//! Prepares record summaries and saved Markdown body lines for the Neovim picker.

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{self, BufRead as _, BufReader, Read as _},
    path::{Path, PathBuf},
    time::Instant,
};

use gray_matter::engine::{Engine as _, YAML};
use pwf_client::{PwfClient, pb};
use pwf_models::{
    note::NoteId,
    task::{TaskId, TaskListLimit},
};
use rmpv::Value;
use serde::{Deserialize, Deserializer};

use crate::{
    OperationError, REQUEST_TIMEOUT, note_list, project_scope,
    task_list::{self, StatusScope},
};

const FRONTMATTER_BYTES_MAX: usize = 1024 * 1024;
const BODY_BYTES_MAX: usize = 4 * 1024 * 1024;
const PICKER_BYTES_MAX: usize = 64 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListRecordsParams {
    #[serde(default)]
    context_paths: Option<Vec<PathBuf>>,
    status: StatusScope,
    #[serde(deserialize_with = "deserialize_limit")]
    limit: TaskListLimit,
}

fn deserialize_limit<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<TaskListLimit, D::Error> {
    TaskListLimit::try_new(usize::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RecordListError {
    #[error("cannot read picker record {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("cannot resolve record directory {0:?}")]
    Directory(String),
    #[error("preparing picker records exceeded {} seconds", REQUEST_TIMEOUT.as_secs())]
    TimedOut,
    #[error("picker data exceeds 64 MiB; narrow the project scope or record limit")]
    TooLarge,
}

pub(crate) struct RecordListing {
    project: Option<String>,
    names: Vec<String>,
    contents: Vec<String>,
    files: BTreeMap<String, String>,
    hidden: u64,
}

impl From<RecordListing> for Value {
    fn from(listing: RecordListing) -> Self {
        let mut fields = vec![
            (
                Value::from("names"),
                Value::Array(listing.names.into_iter().map(Value::from).collect()),
            ),
            (
                Value::from("contents"),
                Value::Array(listing.contents.into_iter().map(Value::from).collect()),
            ),
            (
                Value::from("files"),
                Value::Map(
                    listing
                        .files
                        .into_iter()
                        .map(|(id, path)| (Value::from(id), Value::from(path)))
                        .collect(),
                ),
            ),
            (Value::from("hidden"), Value::from(listing.hidden)),
        ];
        if let Some(project) = listing.project {
            fields.push((Value::from("project"), Value::from(project)));
        }
        Value::Map(fields)
    }
}

pub(crate) async fn execute(
    client: &PwfClient,
    params: ListRecordsParams,
) -> Result<RecordListing, OperationError> {
    let mut projects = project_scope::active_projects(&client.project()).await?;
    let project = params
        .context_paths
        .as_ref()
        .and_then(|paths| project_scope::infer_from_projects(&projects, paths));
    projects.retain(|candidate| project.as_ref().is_none_or(|id| *id == candidate.id));
    let (tasks, notes) = tokio::try_join!(
        task_list::execute(client, project.clone(), params.status, params.limit),
        note_list::execute(client, &projects, params.limit.get()),
    )?;
    let hidden = tasks.hidden + notes.hidden;
    let tasks = tasks
        .tasks
        .into_iter()
        .map(|task| {
            let entry = task_list::task_entry(&task)?;
            Ok(((task.project, task.id), entry))
        })
        .collect::<Result<HashMap<_, _>, OperationError>>()?;
    Ok(tokio::task::spawn_blocking(move || {
        collect(project, projects, &tasks, &notes.entries, hidden)
    })
    .await??)
}

struct Record {
    path: String,
    summary: String,
    body: String,
    body_line_start: usize,
}

fn collect(
    selected: Option<String>,
    projects: Vec<pb::Project>,
    tasks: &HashMap<(String, String), String>,
    notes: &HashMap<String, String>,
    hidden: u64,
) -> Result<RecordListing, RecordListError> {
    let started = Instant::now();
    let home = std::env::home_dir();
    let mut records = BTreeMap::new();
    let mut bytes = 0;
    for project in projects {
        let directory = project_scope::expand_home(&project.tasks_path, home.as_deref())
            .ok_or_else(|| RecordListError::Directory(project.tasks_path.clone()))?;
        let read_error = |source| RecordListError::Read {
            path: directory.clone(),
            source,
        };
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(read_error(source)),
        };
        for entry in entries {
            if started.elapsed() >= REQUEST_TIMEOUT {
                return Err(RecordListError::TimedOut);
            }
            let entry = entry.map_err(read_error)?;
            let path = entry.path();
            if !entry.file_type().map_err(read_error)?.is_file()
                || !record_candidate(&path, &directory)
            {
                continue;
            }
            let Some((id, record)) = read_record(&path, &project, tasks, notes)? else {
                continue;
            };
            bytes += record.path.len() + record.summary.len() + record.body.len();
            if bytes > PICKER_BYTES_MAX {
                return Err(RecordListError::TooLarge);
            }
            records.insert(id, record);
        }
    }
    let mut listing = RecordListing {
        project: selected,
        names: Vec::new(),
        contents: Vec::new(),
        files: BTreeMap::new(),
        hidden,
    };
    bytes = 0;
    for (id, record) in records {
        if started.elapsed() >= REQUEST_TIMEOUT {
            return Err(RecordListError::TimedOut);
        }
        listing.files.insert(id.clone(), record.path);
        push_entry(
            &mut listing.names,
            format!("{id}\t1\t{}", clean_text(&record.summary)),
            &mut bytes,
        )?;
        for (offset, text) in record.body.lines().enumerate() {
            if text.trim().is_empty() {
                continue;
            }
            let line = record.body_line_start + offset;
            push_entry(
                &mut listing.contents,
                format!("{id}\t{line}\t{id}:{line} {}", clean_text(text)),
                &mut bytes,
            )?;
        }
    }
    Ok(listing)
}

fn push_entry(
    entries: &mut Vec<String>,
    entry: String,
    bytes: &mut usize,
) -> Result<(), RecordListError> {
    *bytes += entry.len();
    if *bytes > PICKER_BYTES_MAX {
        return Err(RecordListError::TooLarge);
    }
    entries.push(entry);
    Ok(())
}

fn clean_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn read_record(
    path: &Path,
    project: &pb::Project,
    tasks: &HashMap<(String, String), String>,
    notes: &HashMap<String, String>,
) -> Result<Option<(String, Record)>, RecordListError> {
    let read_error = |source| RecordListError::Read {
        path: path.to_path_buf(),
        source,
    };
    let mut reader = BufReader::new(fs::File::open(path).map_err(read_error)?);
    let (metadata, body_line_start, mut body) =
        read_frontmatter(&mut reader).map_err(read_error)?;
    let note_id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| NoteId::try_new(stem).ok())
        .filter(|id| id.project_id().as_ref() == project.id);
    let matched = if let Some(id) = note_id {
        notes
            .get(id.as_ref())
            .map(|summary| (id.to_string(), summary.clone()))
    } else if metadata.kind.as_deref() != Some("note") {
        metadata
            .id
            .and_then(|raw| TaskId::try_new(raw).ok())
            .and_then(|id| {
                tasks
                    .get(&(project.title.clone(), id.to_string()))
                    .map(|summary| (id.to_string(), summary.clone()))
            })
    } else {
        None
    };
    let Some((id, summary)) = matched else {
        return Ok(None);
    };
    reader
        .take(BODY_BYTES_MAX as u64 + 1)
        .read_to_string(&mut body)
        .map_err(read_error)?;
    if body.len() > BODY_BYTES_MAX {
        return Err(read_error(io::Error::new(
            io::ErrorKind::InvalidData,
            "record body exceeds 4 MiB",
        )));
    }
    let path = path
        .to_str()
        .ok_or_else(|| RecordListError::Directory(path.display().to_string()))?
        .to_string();
    Ok(Some((
        id,
        Record {
            path,
            summary,
            body,
            body_line_start,
        },
    )))
}

fn record_candidate(path: &Path, directory: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if path.extension().and_then(|extension| extension.to_str()) != Some("md")
        || name.ends_with(".plan.md")
    {
        return false;
    }
    let Some(snapshot) = directory.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    name != format!("{snapshot}.md") && name != format!("{snapshot}.backup.md")
}

#[derive(Default, Deserialize)]
struct Metadata {
    id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

fn read_frontmatter(reader: &mut impl io::BufRead) -> io::Result<(Metadata, usize, String)> {
    let mut reader = reader.take(FRONTMATTER_BYTES_MAX as u64 + 1);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let fence = |line: &str| line.trim_end_matches(['\r', '\n', ' ', '\t']) == "---";
    if !fence(line.strip_prefix('\u{feff}').unwrap_or(&line)) {
        return Ok((Metadata::default(), 1, line));
    }
    let mut yaml = String::new();
    let mut line_number = 1;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unclosed YAML frontmatter",
            ));
        }
        line_number += 1;
        if yaml.len() + line.len() > FRONTMATTER_BYTES_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "YAML frontmatter exceeds 1 MiB",
            ));
        }
        if fence(&line) {
            let metadata = YAML::parse(&yaml)
                .and_then(|value| value.deserialize::<Option<Metadata>>())
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
                .unwrap_or_default();
            return Ok((metadata, line_number + 1, String::new()));
        }
        yaml.push_str(&line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_location_preserves_frontmatter_newlines_and_quoted_identity() {
        for newline in ["\n", "\r\n"] {
            let source = format!(
                "\u{feff}---{newline}'id': \"PWF-0007\"{newline}type: task{newline}--- \t{newline}body"
            );
            let mut reader = io::Cursor::new(source.as_bytes());
            let (metadata, line, prefix) = read_frontmatter(&mut reader).unwrap();
            assert_eq!(metadata.id.as_deref(), Some("PWF-0007"));
            assert_eq!(line, 5);
            assert_eq!(prefix, "");
            let mut body = String::new();
            reader.read_to_string(&mut body).unwrap();
            assert_eq!(body, "body");
        }
        let (metadata, line, prefix) =
            read_frontmatter(&mut io::Cursor::new(b"# Note\nbody\n")).unwrap();
        assert!(metadata.id.is_none());
        assert_eq!(line, 1);
        assert_eq!(prefix, "# Note\n");
        assert!(read_frontmatter(&mut io::Cursor::new(b"---\nid: PWF-0007\n")).is_err());
    }

    #[test]
    fn plans_and_snapshots_cannot_be_picker_records() {
        let directory = Path::new("/vault/pwf");
        for name in ["pwf.md", "pwf.backup.md", "PWF-0007.plan.md", "data.json"] {
            assert!(!record_candidate(&directory.join(name), directory));
        }
        for name in ["PWF-0007.md", "PWF-NOTE-0001.md", "renamed task.md"] {
            assert!(record_candidate(&directory.join(name), directory));
        }
    }

    #[test]
    fn record_text_cannot_inject_picker_rows_or_fields() {
        assert_eq!(
            clean_text("title\nbody\twith\rcontrols\u{1b}[31m"),
            "title body with controls [31m"
        );
    }
}
