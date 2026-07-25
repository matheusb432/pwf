use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use pwf_application::{
    AppRecordStore, HandoffDocument, HandoffDocumentIdentifier, HandoffDocumentScopePresence,
    HandoffDocumentStore, HandoffLocation, HandoffPatch, HandoffScope, NewHandoffDocument,
};
use pwf_domain::{
    handoff::HandoffStatus,
    pending_work::{ProjectName, Timestamp},
};
use regex::Regex;

use super::{ObsidianStore, ObsidianStoreError};

static CHECKBOX_ANY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*-\s+\[[ xX]\]").expect("valid checkbox regex"));
static CHECKBOX_DONE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*-\s+\[[xX]\]").expect("valid checkbox regex"));

fn handoff_document(
    scope: &HandoffScope,
    identifier: &HandoffDocumentIdentifier,
) -> Result<Option<HandoffDocument>, ObsidianStoreError> {
    let path = handoff_document_path(scope, identifier);
    match read_handoff_document(&path, identifier.location) {
        Ok(document) => Ok(Some(document)),
        Err(ObsidianStoreError::ReadHandoffDocument { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn handoff_documents(scope: &HandoffScope) -> Result<Vec<HandoffDocument>, ObsidianStoreError> {
    let mut documents = Vec::new();
    for location in [HandoffLocation::Active, HandoffLocation::Archived] {
        documents.extend(handoff_documents_location(scope, location)?);
    }
    Ok(documents)
}

fn handoff_documents_location(
    scope: &HandoffScope,
    location: HandoffLocation,
) -> Result<Vec<HandoffDocument>, ObsidianStoreError> {
    let directory = handoff_directory(scope, location);
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(ObsidianStoreError::ReadHandoffDirectory {
                path: directory,
                source,
            });
        }
    };
    Ok(entries
        .filter_map(|entry| read_handoff_entry(entry, location))
        .collect())
}

#[rustfmt::skip]
fn read_handoff_entry(
    entry: Result<std::fs::DirEntry, std::io::Error>,
    location: HandoffLocation,
) -> Option<HandoffDocument> {
    // FIXME: Return per-entry read failures from the storage port; treating an unreadable handoff as absent can remove it from a rebuilt ledger.
    let entry = entry.ok()?;
    let path = entry.path();
    if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
        return None;
    }
    let file_name = path.file_name()?.to_str()?;
    if matches!(file_name.to_ascii_lowercase().as_str(), "ledger.md" | "readme.md") {
        return None;
    }
    read_handoff_document(&path, location).ok()
}

fn insert_handoff_document(
    scope: &HandoffScope,
    new: &NewHandoffDocument,
) -> Result<HandoffDocument, ObsidianStoreError> {
    let identifier = HandoffDocumentIdentifier {
        file_name: new.file_name.clone(),
        location: HandoffLocation::Active,
    };
    let path = handoff_document_path(scope, &identifier);
    if path.exists() {
        return Err(ObsidianStoreError::HandoffDocumentExists { path });
    }
    let source = render_new_handoff_document(new);
    pwf_core::fs_atomic::write_text_atomic(&path, &source).map_err(|source| {
        ObsidianStoreError::WriteHandoffDocument {
            path: path.clone(),
            source,
        }
    })?;
    read_handoff_document(&path, HandoffLocation::Active)
}

fn update_handoff_document(
    scope: &HandoffScope,
    identifier: &HandoffDocumentIdentifier,
    patch: HandoffPatch,
) -> Result<(), ObsidianStoreError> {
    let source_path = handoff_document_path(scope, identifier);
    let target_location = patch.location.unwrap_or(identifier.location);
    let target_path = if target_location == identifier.location {
        None
    } else {
        let target_identifier = HandoffDocumentIdentifier {
            file_name: identifier.file_name.clone(),
            location: target_location,
        };
        let target_path = handoff_document_path(scope, &target_identifier);
        if target_path.exists() {
            return Err(ObsidianStoreError::HandoffDocumentExists { path: target_path });
        }
        let target_directory = target_path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(target_directory).map_err(|source| {
            ObsidianStoreError::CreateHandoffArchiveDirectory {
                path: target_directory.to_path_buf(),
                source,
            }
        })?;
        Some(target_path)
    };
    let source_original = std::fs::read_to_string(&source_path).map_err(|source| {
        ObsidianStoreError::ReadHandoffDocument {
            path: source_path.clone(),
            source,
        }
    })?;
    let mut source = source_original.clone();
    if let Some(status) = patch.status {
        source = set_frontmatter_property(&source, "status", Some(status.as_frontmatter_str()));
    }
    if let Some(completed) = patch.completed {
        source = set_frontmatter_property(
            &source,
            "completed",
            completed.as_ref().map(Timestamp::as_str),
        );
    }
    if let Some(pending_work_identifier) = patch.pending_work_identifier {
        source = set_frontmatter_property(&source, "pw", Some(pending_work_identifier.as_ref()));
    }
    if let Some(body) = patch.body {
        source = replace_body(&source, &body);
    }
    pwf_core::fs_atomic::write_text_atomic(&source_path, &source).map_err(|source| {
        ObsidianStoreError::WriteHandoffDocument {
            path: source_path.clone(),
            source,
        }
    })?;

    if let Some(target_path) = target_path
        && let Err(move_source) = std::fs::rename(&source_path, &target_path)
    {
        if let Err(restore_source) =
            pwf_core::fs_atomic::write_text_atomic(&source_path, &source_original)
        {
            return Err(ObsidianStoreError::RestoreHandoffDocument {
                path: source_path.clone(),
                from: source_path,
                to: target_path,
                move_source,
                restore_source,
            });
        }
        return Err(ObsidianStoreError::MoveHandoffDocument {
            from: source_path,
            to: target_path,
            source: move_source,
        });
    }
    Ok(())
}

impl AppRecordStore<HandoffDocument> for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
    ) -> Result<Option<HandoffDocument>, Self::Error> {
        handoff_document(scope, identifier)
    }

    fn list(&self, scope: &HandoffScope) -> Result<Vec<HandoffDocument>, Self::Error> {
        handoff_documents(scope)
    }

    fn insert(
        &self,
        scope: &HandoffScope,
        new: NewHandoffDocument,
    ) -> Result<HandoffDocument, Self::Error> {
        insert_handoff_document(scope, &new)
    }

    fn update(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
        patch: HandoffPatch,
    ) -> Result<(), Self::Error> {
        update_handoff_document(scope, identifier, patch)
    }

    fn delete(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
    ) -> Result<(), Self::Error> {
        let path = handoff_document_path(scope, identifier);
        std::fs::remove_file(&path)
            .map_err(|source| ObsidianStoreError::RemoveHandoffDocument { path, source })
    }
}

impl HandoffDocumentStore for ObsidianStore {
    fn scope_presence(
        &self,
        scope: &HandoffScope,
    ) -> Result<HandoffDocumentScopePresence, <Self as AppRecordStore<HandoffDocument>>::Error>
    {
        let repository_exists = scope.repository_root.try_exists().map_err(|source| {
            ObsidianStoreError::ReadHandoffDirectory {
                path: scope.repository_root.clone(),
                source,
            }
        })?;
        if !repository_exists {
            return Ok(HandoffDocumentScopePresence::RepositoryMissing);
        }
        let directory = handoff_directory(scope, HandoffLocation::Active);
        let directory_exists =
            directory
                .try_exists()
                .map_err(|source| ObsidianStoreError::ReadHandoffDirectory {
                    path: directory.clone(),
                    source,
                })?;
        if !directory_exists {
            return Ok(HandoffDocumentScopePresence::HandoffDirectoryMissing);
        }
        let metadata = std::fs::metadata(&directory).map_err(|source| {
            ObsidianStoreError::ReadHandoffDirectory {
                path: directory.clone(),
                source,
            }
        })?;
        if !metadata.is_dir() {
            return Err(ObsidianStoreError::ReadHandoffDirectory {
                path: directory,
                source: std::io::Error::from(std::io::ErrorKind::NotADirectory),
            });
        }
        Ok(HandoffDocumentScopePresence::Present)
    }

    fn document_exists(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
    ) -> Result<bool, <Self as AppRecordStore<HandoffDocument>>::Error> {
        let path = handoff_document_path(scope, identifier);
        path.try_exists()
            .map_err(|source| ObsidianStoreError::InspectHandoffDocument { path, source })
    }

    fn list_location(
        &self,
        scope: &HandoffScope,
        location: HandoffLocation,
    ) -> Result<Vec<HandoffDocument>, <Self as AppRecordStore<HandoffDocument>>::Error> {
        handoff_documents_location(scope, location)
    }

    fn restore_document_after_move(
        &self,
        scope: &HandoffScope,
        snapshot: &HandoffDocument,
    ) -> Result<(), <Self as AppRecordStore<HandoffDocument>>::Error> {
        restore_document_source(scope, snapshot)?;
        let moved_identifier = HandoffDocumentIdentifier {
            file_name: snapshot.identifier.file_name.clone(),
            location: match snapshot.identifier.location {
                HandoffLocation::Active => HandoffLocation::Archived,
                HandoffLocation::Archived => HandoffLocation::Active,
            },
        };
        let moved_path = handoff_document_path(scope, &moved_identifier);
        match std::fs::remove_file(&moved_path) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ObsidianStoreError::RemoveHandoffDocument {
                    path: moved_path,
                    source,
                });
            }
        }
        Ok(())
    }

    fn restore_document_after_delete(
        &self,
        scope: &HandoffScope,
        snapshot: &HandoffDocument,
    ) -> Result<(), <Self as AppRecordStore<HandoffDocument>>::Error> {
        restore_document_source(scope, snapshot)
    }
}

fn restore_document_source(
    scope: &HandoffScope,
    snapshot: &HandoffDocument,
) -> Result<(), ObsidianStoreError> {
    let path = handoff_document_path(scope, &snapshot.identifier);
    pwf_core::fs_atomic::write_text_atomic(&path, &snapshot.source)
        .map_err(|source| ObsidianStoreError::WriteHandoffDocument { path, source })
}

fn handoff_directory(scope: &HandoffScope, location: HandoffLocation) -> PathBuf {
    let directory = scope.repository_root.join("docs").join("handoffs");
    match location {
        HandoffLocation::Active => directory,
        HandoffLocation::Archived => directory.join("archived"),
    }
}

fn handoff_document_path(scope: &HandoffScope, identifier: &HandoffDocumentIdentifier) -> PathBuf {
    handoff_directory(scope, identifier.location).join(&identifier.file_name)
}

fn read_handoff_document(
    path: &Path,
    location: HandoffLocation,
) -> Result<HandoffDocument, ObsidianStoreError> {
    let source = std::fs::read_to_string(path).map_err(|source| {
        ObsidianStoreError::ReadHandoffDocument {
            path: path.to_path_buf(),
            source,
        }
    })?;
    let modified_timestamp = std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|source| ObsidianStoreError::ReadHandoffMetadata {
            path: path.to_path_buf(),
            source,
        })?;
    let parsed = pwf_core::frontmatter::parse(&source);
    let file_name = path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .unwrap_or_default()
        .to_string();
    let field = |name: &str| {
        parsed
            .frontmatter
            .get(name)
            .filter(|value| !value.trim().is_empty())
    };
    let (goals_completed, goals_total) = goal_counts(&parsed.body);
    Ok(HandoffDocument {
        identifier: HandoffDocumentIdentifier {
            file_name: file_name.clone(),
            location,
        },
        location,
        project: field("project").and_then(|value| ProjectName::try_new(value).ok()),
        title: handoff_title(&parsed.body, file_name.trim_end_matches(".md")),
        status: field("status").and_then(|value| value.parse::<HandoffStatus>().ok()),
        created: field("created").map(|value| Timestamp::new(value.clone())),
        completed: field("completed").map(|value| Timestamp::new(value.clone())),
        pending_work_identifier_raw: parsed.frontmatter.get("pw").cloned(),
        goals_completed,
        goals_total,
        body: parsed.body,
        source,
        locator: path.to_path_buf(),
        modified_timestamp,
    })
}

fn render_new_handoff_document(new: &NewHandoffDocument) -> String {
    use std::fmt::Write as _;

    let mut source = String::from("---\nstatus: active\n");
    let _ = writeln!(source, "project: {}", new.project);
    let _ = writeln!(source, "created: {}", new.created.as_str());
    if let Some(pending_work_identifier) = &new.pending_work_identifier {
        let _ = writeln!(source, "pw: {pending_work_identifier}");
    }
    source.push_str("---\n");
    source.push_str(&new.body);
    source
}

fn handoff_title(body: &str, fallback: &str) -> String {
    body.lines()
        .filter_map(|line| line.strip_prefix('#'))
        .find_map(|heading| {
            heading
                .starts_with(char::is_whitespace)
                .then(|| heading.trim())
                .filter(|heading| !heading.is_empty())
        })
        .map_or_else(|| fallback.to_string(), str::to_string)
}

fn goal_counts(body: &str) -> (usize, usize) {
    body.lines().fold((0, 0), |(completed, total), line| {
        (
            completed + usize::from(CHECKBOX_DONE_RE.is_match(line)),
            total + usize::from(CHECKBOX_ANY_RE.is_match(line)),
        )
    })
}

fn set_frontmatter_property(source: &str, property: &str, value: Option<&str>) -> String {
    let Some((frontmatter_start, frontmatter_end, newline)) = frontmatter_bounds(source) else {
        return source.to_string();
    };
    let frontmatter = &source[frontmatter_start..frontmatter_end];
    let mut offset = 0;
    for line in frontmatter.split_inclusive('\n') {
        let line_without_ending = line.trim_end_matches(['\r', '\n']);
        if line_without_ending
            .split_once(':')
            .is_some_and(|(key, _)| key == property)
        {
            let start = frontmatter_start + offset;
            let end = start + line.len();
            let replacement =
                value.map_or_else(String::new, |value| format!("{property}: {value}{newline}"));
            return format!("{}{}{}", &source[..start], replacement, &source[end..]);
        }
        offset += line.len();
    }
    let Some(value) = value else {
        return source.to_string();
    };
    let insertion = if property == "completed" {
        frontmatter
            .split_inclusive('\n')
            .scan(frontmatter_start, |offset, line| {
                let start = *offset;
                *offset += line.len();
                Some((start, line))
            })
            .find_map(|(start, line)| {
                line.trim_end_matches(['\r', '\n'])
                    .split_once(':')
                    .is_some_and(|(key, _)| key == "status")
                    .then_some(start + line.len())
            })
            .unwrap_or(frontmatter_end)
    } else {
        frontmatter_end
    };
    format!(
        "{}{property}: {value}{newline}{}",
        &source[..insertion],
        &source[insertion..]
    )
}

fn replace_body(source: &str, body: &str) -> String {
    let Some((_, frontmatter_end, _)) = frontmatter_bounds(source) else {
        return body.to_string();
    };
    let close_line_end = source[frontmatter_end..]
        .find('\n')
        .map_or(source.len(), |offset| frontmatter_end + offset + 1);
    format!("{}{}", &source[..close_line_end], body)
}

fn frontmatter_bounds(source: &str) -> Option<(usize, usize, &'static str)> {
    let without_bom = source.strip_prefix('\u{feff}').unwrap_or(source);
    let byte_offset = source.len() - without_bom.len();
    let opening_end = without_bom.find('\n')? + 1;
    let opening = &without_bom[..opening_end];
    if opening.trim_end_matches(['\r', '\n']) != "---" {
        return None;
    }
    let newline = if opening.ends_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut offset = opening_end;
    for line in without_bom[opening_end..].split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Some((byte_offset + opening_end, byte_offset + offset, newline));
        }
        offset += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, path::PathBuf};

    use pwf_application::{
        AppRecordStore, HandoffDocument, HandoffDocumentIdentifier, HandoffDocumentStore,
        HandoffLocation, HandoffPatch, HandoffScope, NewHandoffDocument,
    };
    use pwf_domain::{
        handoff::HandoffStatus,
        pending_work::{ProjectName, Timestamp, WorkItemId},
    };

    use super::super::{ObsidianStore, ObsidianStoreError};

    fn store() -> ObsidianStore {
        ObsidianStore::new([])
    }

    fn scope(repository_root: PathBuf) -> HandoffScope {
        HandoffScope { repository_root }
    }

    #[test]
    fn list_reads_active_and_archived_documents_without_discarding_malformed_metadata() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let handoff_directory = scope.repository_root.join("docs/handoffs");
        let archive_directory = handoff_directory.join("archived");
        std::fs::create_dir_all(&archive_directory).unwrap();
        let malformed = "---\nstatus: paused\nproject: \ncreated: \ncompleted: \npw: not-an-id\n---\n\n# Malformed metadata\n\n- [x] one\n- [ ] two\n";
        let malformed_path = handoff_directory.join("2026-01-01-malformed.md");
        std::fs::write(&malformed_path, malformed).unwrap();
        let archived_path = archive_directory.join("2026-01-02-done.md");
        std::fs::write(
            &archived_path,
            "---\nstatus: done\nproject: test-project\ncreated: 2026-01-02\ncompleted: 2026-01-03\npw: TST-0002\n---\n\n# Done\n",
        )
        .unwrap();
        std::fs::write(handoff_directory.join("LEDGER.md"), "ignored").unwrap();
        std::fs::write(handoff_directory.join("README.md"), "ignored").unwrap();

        let documents =
            <ObsidianStore as AppRecordStore<HandoffDocument>>::list(&store(), &scope).unwrap();

        assert_eq!(documents.len(), 2);
        let malformed_document = documents
            .iter()
            .find(|document| document.identifier.file_name == "2026-01-01-malformed.md")
            .unwrap();
        assert_eq!(malformed_document.location, HandoffLocation::Active);
        assert_eq!(malformed_document.project, None);
        assert_eq!(malformed_document.status, None);
        assert_eq!(malformed_document.created, None);
        assert_eq!(malformed_document.completed, None);
        assert_eq!(
            malformed_document.pending_work_identifier_raw.as_deref(),
            Some("not-an-id")
        );
        assert_eq!(malformed_document.title, "Malformed metadata");
        assert_eq!(malformed_document.goals_completed, 1);
        assert_eq!(malformed_document.goals_total, 2);
        assert_eq!(malformed_document.source, malformed);
        assert_eq!(malformed_document.locator, malformed_path);
        assert_eq!(
            malformed_document.modified_timestamp,
            std::fs::metadata(&malformed_document.locator)
                .unwrap()
                .modified()
                .unwrap()
        );

        let archived_document = documents
            .iter()
            .find(|document| document.location == HandoffLocation::Archived)
            .unwrap();
        assert_eq!(archived_document.status, Some(HandoffStatus::Done));
        assert_eq!(archived_document.locator, archived_path);
    }

    #[test]
    fn scope_presence_reports_non_directory_errors() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        std::fs::create_dir_all(&scope.repository_root).unwrap();
        std::fs::write(scope.repository_root.join("docs"), "not a directory").unwrap();
        let store = store();

        let error = store.scope_presence(&scope).unwrap_err();
        assert!(matches!(
            error,
            ObsidianStoreError::ReadHandoffDirectory { .. }
        ));
    }

    #[test]
    fn exact_path_inspection_reports_non_directory_errors() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        std::fs::create_dir_all(&scope.repository_root).unwrap();
        std::fs::write(scope.repository_root.join("docs"), "not a directory").unwrap();
        let identifier = HandoffDocumentIdentifier {
            file_name: "2026-01-01-managed-flow.md".to_string(),
            location: HandoffLocation::Active,
        };

        let error = store().document_exists(&scope, &identifier).unwrap_err();

        assert!(matches!(
            error,
            ObsidianStoreError::InspectHandoffDocument { .. }
        ));
    }

    #[test]
    fn active_location_read_ignores_an_unreadable_archive_path() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let handoff_directory = scope.repository_root.join("docs/handoffs");
        std::fs::create_dir_all(&handoff_directory).unwrap();
        std::fs::write(
            handoff_directory.join("2026-01-01-active.md"),
            "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Active\n",
        )
        .unwrap();
        std::fs::write(handoff_directory.join("archived"), "not a directory").unwrap();

        let documents = store()
            .list_location(&scope, HandoffLocation::Active)
            .unwrap();

        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].title, "Active");
    }

    #[test]
    fn insert_update_move_and_delete_round_trip_atomically() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let store = store();
        let identifier = HandoffDocumentIdentifier {
            file_name: "2026-01-01-managed-flow.md".to_string(),
            location: HandoffLocation::Active,
        };

        let inserted = <ObsidianStore as AppRecordStore<HandoffDocument>>::insert(
            &store,
            &scope,
            NewHandoffDocument {
                file_name: identifier.file_name.clone(),
                project: ProjectName::try_new("test-project").unwrap(),
                title: "Managed Flow".to_string(),
                created: Timestamp::new("2026-01-01"),
                body: "\n# Managed Flow\n\n## Goals\n- [ ] first\n".to_string(),
                pending_work_identifier: None,
            },
        )
        .unwrap();
        assert_eq!(inserted.identifier, identifier);
        assert!(inserted.source.contains("status: active\n"));
        assert!(!inserted.source.contains("pw:"));

        <ObsidianStore as AppRecordStore<HandoffDocument>>::update(
            &store,
            &scope,
            &identifier,
            HandoffPatch {
                location: Some(HandoffLocation::Archived),
                status: Some(HandoffStatus::Done),
                completed: Some(Some(Timestamp::new("2026-01-02"))),
                pending_work_identifier: Some(WorkItemId::try_new("TST-0001").unwrap()),
                body: Some("\n# Managed Flow\n\n## Goals\n- [x] first\n".to_string()),
            },
        )
        .unwrap();

        assert!(
            <ObsidianStore as AppRecordStore<HandoffDocument>>::get(&store, &scope, &identifier)
                .unwrap()
                .is_none()
        );
        let archived_identifier = HandoffDocumentIdentifier {
            location: HandoffLocation::Archived,
            ..identifier
        };
        let archived = <ObsidianStore as AppRecordStore<HandoffDocument>>::get(
            &store,
            &scope,
            &archived_identifier,
        )
        .unwrap()
        .unwrap();
        assert_eq!(archived.status, Some(HandoffStatus::Done));
        assert_eq!(archived.completed, Some(Timestamp::new("2026-01-02")));
        assert_eq!(
            archived.pending_work_identifier_raw.as_deref(),
            Some("TST-0001")
        );
        assert_eq!(archived.goals_completed, 1);
        assert!(archived.locator.exists());
        assert!(!inserted.locator.exists());
        assert!(
            std::fs::read_dir(archived.locator.parent().unwrap())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("tmp-"))
        );

        <ObsidianStore as AppRecordStore<HandoffDocument>>::delete(
            &store,
            &scope,
            &archived_identifier,
        )
        .unwrap();
        assert!(!archived.locator.exists());
    }

    #[test]
    fn close_inserts_missing_completed_immediately_after_status() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let handoff_directory = scope.repository_root.join("docs/handoffs");
        std::fs::create_dir_all(&handoff_directory).unwrap();
        let identifier = HandoffDocumentIdentifier {
            file_name: "2026-01-01-legacy.md".to_string(),
            location: HandoffLocation::Active,
        };
        let source = "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Legacy\n";
        std::fs::write(handoff_directory.join(&identifier.file_name), source).unwrap();

        <ObsidianStore as AppRecordStore<HandoffDocument>>::update(
            &store(),
            &scope,
            &identifier,
            HandoffPatch {
                location: Some(HandoffLocation::Archived),
                status: Some(HandoffStatus::Done),
                completed: Some(Some(Timestamp::new("2026-01-02"))),
                ..HandoffPatch::default()
            },
        )
        .unwrap();

        let archived = std::fs::read_to_string(
            handoff_directory
                .join("archived")
                .join(&identifier.file_name),
        )
        .unwrap();
        assert_eq!(
            archived,
            "---\nstatus: done\ncompleted: 2026-01-02\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Legacy\n"
        );
    }

    #[test]
    fn restore_after_move_preserves_exact_source_without_leaving_destination() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let handoff_directory = scope.repository_root.join("docs/handoffs");
        std::fs::create_dir_all(&handoff_directory).unwrap();
        let identifier = HandoffDocumentIdentifier {
            file_name: "2026-01-01-managed-flow.md".to_string(),
            location: HandoffLocation::Active,
        };
        let source = "---\nstatus: active\ncustom: keep exactly\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n\nlegacy spacing  \n";
        std::fs::write(handoff_directory.join(&identifier.file_name), source).unwrap();
        let store = store();
        let snapshot =
            <ObsidianStore as AppRecordStore<HandoffDocument>>::get(&store, &scope, &identifier)
                .unwrap()
                .unwrap();
        <ObsidianStore as AppRecordStore<HandoffDocument>>::update(
            &store,
            &scope,
            &identifier,
            HandoffPatch {
                location: Some(HandoffLocation::Archived),
                status: Some(HandoffStatus::Done),
                completed: Some(Some(Timestamp::new("2026-01-02"))),
                ..HandoffPatch::default()
            },
        )
        .unwrap();

        store
            .restore_document_after_move(&scope, &snapshot)
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(handoff_directory.join(&identifier.file_name)).unwrap(),
            source
        );
        assert!(
            !handoff_directory
                .join("archived")
                .join(&identifier.file_name)
                .exists(),
            "moved destination must be removed after the original is restored"
        );
    }

    #[test]
    fn restore_after_delete_preserves_same_name_archived_document() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let handoff_directory = scope.repository_root.join("docs/handoffs");
        let archive_directory = handoff_directory.join("archived");
        std::fs::create_dir_all(&archive_directory).unwrap();
        let identifier = HandoffDocumentIdentifier {
            file_name: "2026-01-01-managed-flow.md".to_string(),
            location: HandoffLocation::Active,
        };
        let active_source = "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Active\n";
        let archived_source = "---\nstatus: done\nproject: test-project\ncreated: 2025-01-01\npw: TST-9999\n---\n\n# Existing archive\n";
        std::fs::write(handoff_directory.join(&identifier.file_name), active_source).unwrap();
        std::fs::write(
            archive_directory.join(&identifier.file_name),
            archived_source,
        )
        .unwrap();
        let store = store();
        let snapshot =
            <ObsidianStore as AppRecordStore<HandoffDocument>>::get(&store, &scope, &identifier)
                .unwrap()
                .unwrap();
        <ObsidianStore as AppRecordStore<HandoffDocument>>::delete(&store, &scope, &identifier)
            .unwrap();

        store
            .restore_document_after_delete(&scope, &snapshot)
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(handoff_directory.join(&identifier.file_name)).unwrap(),
            active_source
        );
        assert_eq!(
            std::fs::read_to_string(archive_directory.join(&identifier.file_name)).unwrap(),
            archived_source
        );
    }

    #[test]
    fn insert_rejects_an_existing_destination() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let store = store();
        let new = NewHandoffDocument {
            file_name: "2026-01-01-existing.md".to_string(),
            project: ProjectName::try_new("test-project").unwrap(),
            title: "Existing".to_string(),
            created: Timestamp::new("2026-01-01"),
            body: "\n# Existing\n".to_string(),
            pending_work_identifier: None,
        };
        <ObsidianStore as AppRecordStore<HandoffDocument>>::insert(&store, &scope, new.clone())
            .unwrap();

        let error = <ObsidianStore as AppRecordStore<HandoffDocument>>::insert(&store, &scope, new)
            .unwrap_err();

        assert_matches!(error, ObsidianStoreError::HandoffDocumentExists { .. });
    }

    #[test]
    fn move_collision_fails_before_rewriting_either_document() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let handoff_directory = scope.repository_root.join("docs/handoffs");
        let archive_directory = handoff_directory.join("archived");
        std::fs::create_dir_all(&archive_directory).unwrap();
        let file_name = "2026-01-01-managed-flow.md";
        let active_path = handoff_directory.join(file_name);
        let archived_path = archive_directory.join(file_name);
        let active_source = "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n\n- [ ] first\n";
        let archived_source = "existing archive destination\n";
        std::fs::write(&active_path, active_source).unwrap();
        std::fs::write(&archived_path, archived_source).unwrap();
        let identifier = HandoffDocumentIdentifier {
            file_name: file_name.to_string(),
            location: HandoffLocation::Active,
        };

        let error = <ObsidianStore as AppRecordStore<HandoffDocument>>::update(
            &store(),
            &scope,
            &identifier,
            HandoffPatch {
                location: Some(HandoffLocation::Archived),
                status: Some(HandoffStatus::Done),
                completed: Some(Some(Timestamp::new("2026-01-02"))),
                pending_work_identifier: None,
                body: Some("\n# Mutated\n".to_string()),
            },
        )
        .unwrap_err();

        assert_matches!(error, ObsidianStoreError::HandoffDocumentExists { .. });
        assert_eq!(std::fs::read_to_string(active_path).unwrap(), active_source);
        assert_eq!(
            std::fs::read_to_string(archived_path).unwrap(),
            archived_source
        );
    }

    #[test]
    fn archive_directory_creation_failure_precedes_source_rewrite() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let handoff_directory = scope.repository_root.join("docs/handoffs");
        std::fs::create_dir_all(&handoff_directory).unwrap();
        let file_name = "2026-01-01-managed-flow.md";
        let active_path = handoff_directory.join(file_name);
        let archive_path = handoff_directory.join("archived");
        let active_source = "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n";
        std::fs::write(&active_path, active_source).unwrap();
        std::fs::write(&archive_path, "blocks archive directory creation\n").unwrap();
        let identifier = HandoffDocumentIdentifier {
            file_name: file_name.to_string(),
            location: HandoffLocation::Active,
        };

        let error = <ObsidianStore as AppRecordStore<HandoffDocument>>::update(
            &store(),
            &scope,
            &identifier,
            HandoffPatch {
                location: Some(HandoffLocation::Archived),
                status: Some(HandoffStatus::Done),
                completed: Some(Some(Timestamp::new("2026-01-02"))),
                pending_work_identifier: None,
                body: Some("\n# Mutated\n".to_string()),
            },
        )
        .unwrap_err();

        assert_matches!(
            error,
            ObsidianStoreError::CreateHandoffArchiveDirectory { .. }
        );
        assert_eq!(std::fs::read_to_string(active_path).unwrap(), active_source);
        assert_eq!(
            std::fs::read_to_string(archive_path).unwrap(),
            "blocks archive directory creation\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rename_failure_restores_the_original_source_atomically() {
        use std::os::unix::fs::PermissionsExt as _;

        let temporary_directory = tempfile::tempdir().unwrap();
        let scope = scope(temporary_directory.path().join("repo"));
        let handoff_directory = scope.repository_root.join("docs/handoffs");
        let archive_directory = handoff_directory.join("archived");
        std::fs::create_dir_all(&archive_directory).unwrap();
        let file_name = "2026-01-01-managed-flow.md";
        let active_path = handoff_directory.join(file_name);
        let active_source = "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n";
        std::fs::write(&active_path, active_source).unwrap();
        let mut permissions = std::fs::metadata(&archive_directory).unwrap().permissions();
        permissions.set_mode(0o555);
        std::fs::set_permissions(&archive_directory, permissions).unwrap();
        let identifier = HandoffDocumentIdentifier {
            file_name: file_name.to_string(),
            location: HandoffLocation::Active,
        };

        let error = <ObsidianStore as AppRecordStore<HandoffDocument>>::update(
            &store(),
            &scope,
            &identifier,
            HandoffPatch {
                location: Some(HandoffLocation::Archived),
                status: Some(HandoffStatus::Done),
                completed: Some(Some(Timestamp::new("2026-01-02"))),
                pending_work_identifier: None,
                body: Some("\n# Mutated\n".to_string()),
            },
        )
        .unwrap_err();
        let mut permissions = std::fs::metadata(&archive_directory).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&archive_directory, permissions).unwrap();

        assert_matches!(error, ObsidianStoreError::MoveHandoffDocument { .. });
        assert_eq!(std::fs::read_to_string(active_path).unwrap(), active_source);
    }
}
