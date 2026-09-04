use std::{
    fmt, fs,
    io::{self, Read as _, Write as _},
    ops::Range,
    path::{Path, PathBuf},
};

use gray_matter::engine::{Engine as _, YAML};
use serde::{Serialize, de::DeserializeOwned};

use super::markdown_line;
use crate::file_transaction::{FileSnapshot, FileTransaction, PresentFileSnapshot, snapshot};

const UTF8_BYTE_ORDER_MARK: char = '\u{feff}';
const FRONTMATTER_BYTE_COUNT_MAX: usize = 1024 * 1024;

/// Owns one UTF-8 Markdown document and its filesystem path.
#[derive(Debug)]
pub struct MarkdownFile {
    path: PathBuf,
    source: String,
    observed: Option<PresentFileSnapshot>,
}

impl MarkdownFile {
    pub(super) fn read_source(path: &Path) -> Result<Self, MarkdownFileError> {
        let source = fs::read_to_string(path).map_err(|source| MarkdownFileError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(Self::from_source(path, source))
    }

    pub(super) fn read_frontmatter_file(path: &Path) -> Result<Self, MarkdownFileError> {
        read_frontmatter_source(path)
            .map(|source| Self::from_source(path, source.unwrap_or_default()))
    }

    /// Reads a Markdown file without normalizing its contents.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, MarkdownFileError> {
        let path = path.into();
        let observed = snapshot(&path)
            .map_err(|source| MarkdownFileError::Read {
                path: path.clone(),
                source: io::Error::other(source),
            })?
            .into_present()
            .ok_or_else(|| MarkdownFileError::Read {
                path: path.clone(),
                source: io::Error::new(io::ErrorKind::NotFound, "file does not exist"),
            })?;
        Self::from_snapshot(observed)
    }

    /// Reads and deserializes YAML frontmatter without loading the Markdown body.
    ///
    /// Frontmatter is limited to 1 MiB. A file without an opening fence returns `None`.
    pub fn read_frontmatter<M>(path: impl Into<PathBuf>) -> Result<Option<M>, MarkdownFileError>
    where
        M: DeserializeOwned,
    {
        let path = path.into();
        let Some(source) = read_frontmatter_source(&path)? else {
            return Ok(None);
        };
        frontmatter_view(&path, &source)?
            .map(|frontmatter| frontmatter.deserialize())
            .transpose()
    }

    /// Creates a Markdown file from typed YAML frontmatter and a body.
    ///
    /// The metadata must serialize to a string-keyed mapping. This operation creates parent
    /// directories and fails when the destination already exists.
    pub fn create_new<M>(
        path: impl Into<PathBuf>,
        frontmatter: &M,
        body: &str,
    ) -> Result<Self, MarkdownFileError>
    where
        M: Serialize,
    {
        let path = path.into();
        let value = serde_json::to_value(frontmatter).map_err(|source| {
            MarkdownFileError::SerializeFrontmatter {
                path: path.clone(),
                source: FrontmatterSerializeError(source),
            }
        })?;
        let serde_json::Value::Object(properties) = value else {
            return Err(MarkdownFileError::FrontmatterMustBeMapping { path });
        };
        let source = render_new_source(&path, properties, body)?;
        Self::create_rendered_new(path, source)
    }

    /// Returns the file's current path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the complete in-memory Markdown source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the body after the closing frontmatter fence.
    ///
    /// A document without a complete opening frontmatter block is entirely body text.
    #[must_use]
    pub fn body(&self) -> &str {
        frontmatter_bounds(&self.path, &self.source)
            .ok()
            .flatten()
            .map_or(&self.source, |bounds| &self.source[bounds.body_start..])
    }

    /// Deserializes the complete frontmatter mapping without changing the document.
    pub fn frontmatter<M>(&self) -> Result<Option<M>, MarkdownFileError>
    where
        M: DeserializeOwned,
    {
        self.frontmatter_view()?
            .map(|frontmatter| frontmatter.deserialize())
            .transpose()
    }

    /// Validates the frontmatter once and exposes borrowed top-level property values.
    pub fn frontmatter_view(&self) -> Result<Option<FrontmatterView<'_>>, MarkdownFileError> {
        frontmatter_view(&self.path, &self.source)
    }

    /// Adds or replaces one top-level frontmatter property in memory.
    ///
    /// The edit retains all bytes outside the selected property. Call [`Self::save`] to persist it.
    pub fn set_property<V>(&mut self, name: &str, value: &V) -> Result<(), MarkdownFileError>
    where
        V: Serialize + ?Sized,
    {
        self.set_property_after(name, value, &[])
    }

    pub(super) fn set_property_after<V>(
        &mut self,
        name: &str,
        value: &V,
        insert_after: &[&str],
    ) -> Result<(), MarkdownFileError>
    where
        V: Serialize + ?Sized,
    {
        let rendered = serde_json::to_string(value).map_err(|source| {
            MarkdownFileError::SerializeFrontmatter {
                path: self.path.clone(),
                source: FrontmatterSerializeError(source),
            }
        })?;
        self.set_property_rendered(name, Some(&rendered), insert_after)?;
        Ok(())
    }

    /// Removes one top-level frontmatter property in memory.
    ///
    /// Returns whether the property existed. Call [`Self::save`] to persist the edit.
    pub fn remove_property(&mut self, name: &str) -> Result<bool, MarkdownFileError> {
        self.set_property_rendered(name, None, &[])
    }

    /// Atomically replaces the file through a temporary file in the destination directory.
    pub fn save(&self) -> Result<(), MarkdownFileError> {
        let observed = self
            .observed
            .clone()
            .ok_or_else(|| MarkdownFileError::Write {
                path: self.path.clone(),
                source: io::Error::other("Markdown source was not read from this file"),
            })?;
        let mut transaction = FileTransaction::new();
        transaction
            .replace(
                FileSnapshot::Present(observed),
                self.source.clone().into_bytes().into_boxed_slice(),
            )
            .and_then(|()| transaction.commit())
            .map_err(|source| MarkdownFileError::Write {
                path: self.path.clone(),
                source: io::Error::other(source),
            })
    }

    pub(super) fn from_source(path: impl Into<PathBuf>, source: String) -> Self {
        Self {
            path: path.into(),
            source,
            observed: None,
        }
    }

    pub(crate) fn from_snapshot(observed: PresentFileSnapshot) -> Result<Self, MarkdownFileError> {
        let path = observed.path().to_path_buf();
        let source = String::from_utf8(observed.bytes().to_vec()).map_err(|source| {
            MarkdownFileError::Read {
                path: path.clone(),
                source: io::Error::new(io::ErrorKind::InvalidData, source),
            }
        })?;
        Ok(Self {
            path,
            source,
            observed: Some(observed),
        })
    }

    pub(crate) fn into_replacement(
        self,
    ) -> Result<(PresentFileSnapshot, Box<[u8]>), MarkdownFileError> {
        let observed = self.observed.ok_or_else(|| MarkdownFileError::Write {
            path: self.path,
            source: io::Error::other("Markdown source was not read from this file"),
        })?;
        Ok((observed, self.source.into_bytes().into_boxed_slice()))
    }

    pub(super) fn into_source(self) -> String {
        self.source
    }

    pub(super) fn into_parts(self) -> (PathBuf, String) {
        (self.path, self.source)
    }

    pub(super) fn replace_source(&mut self, source: String) {
        self.source = source;
    }

    pub(super) fn create_rendered_new(
        path: impl Into<PathBuf>,
        source: String,
    ) -> Result<Self, MarkdownFileError> {
        let file = Self::from_source(path, source);
        write_text_atomic_new(&file.path, &file.source).map_err(|source| {
            MarkdownFileError::Create {
                path: file.path.clone(),
                source,
            }
        })?;
        Ok(file)
    }

    pub(super) fn write_rendered(
        path: impl Into<PathBuf>,
        source: String,
    ) -> Result<(), MarkdownFileError> {
        let path = path.into();
        let observed = snapshot(&path).map_err(|source| MarkdownFileError::Write {
            path: path.clone(),
            source: io::Error::other(source),
        })?;
        let mut transaction = FileTransaction::new();
        transaction
            .replace(observed, source.into_bytes().into_boxed_slice())
            .and_then(|()| transaction.commit())
            .map_err(|source| MarkdownFileError::Write {
                path,
                source: io::Error::other(source),
            })
    }

    pub(super) fn property_text(&self, name: &str) -> Result<Option<&str>, MarkdownFileError> {
        self.frontmatter_view()?
            .map_or(Ok(None), |frontmatter| frontmatter.get(name))
    }

    pub(super) fn set_property_rendered(
        &mut self,
        name: &str,
        value: Option<&str>,
        insert_after: &[&str],
    ) -> Result<bool, MarkdownFileError> {
        set_property_rendered_source(&mut self.source, &self.path, name, value, insert_after)
    }
}

/// Borrows one validated YAML frontmatter block for repeated property access.
#[derive(Debug)]
pub struct FrontmatterView<'document> {
    path: &'document Path,
    source: &'document str,
    properties: Vec<FrontmatterProperty>,
}

impl<'document> FrontmatterView<'document> {
    /// Returns one top-level property value without reparsing the frontmatter.
    pub fn get(&self, name: &str) -> Result<Option<&'document str>, MarkdownFileError> {
        validate_property_name(name)?;
        Ok(self
            .properties
            .iter()
            .find(|property| property.name(self.source) == name)
            .map(|property| property.value(self.source)))
    }

    /// Deserializes the complete YAML mapping.
    pub fn deserialize<M>(&self) -> Result<M, MarkdownFileError>
    where
        M: DeserializeOwned,
    {
        let source = self.source.trim();
        if source.is_empty() {
            return Err(MarkdownFileError::MalformedFrontmatter {
                path: self.path.to_path_buf(),
            });
        }
        YAML::parse(source)
            .and_then(|value| value.deserialize::<M>())
            .map_err(|source| MarkdownFileError::ParseFrontmatter {
                path: self.path.to_path_buf(),
                source: FrontmatterParseError(source),
            })
    }
}

#[derive(Debug)]
struct FrontmatterProperty {
    range: Range<usize>,
}

impl FrontmatterProperty {
    fn name<'frontmatter>(&self, frontmatter: &'frontmatter str) -> &'frontmatter str {
        frontmatter[self.range.clone()]
            .lines()
            .next()
            .and_then(|line| line.split_once(':'))
            .map_or("", |(name, _)| name.trim_end())
    }

    fn value<'frontmatter>(&self, frontmatter: &'frontmatter str) -> &'frontmatter str {
        frontmatter[self.range.clone()]
            .split_once(':')
            .map_or("", |(_, value)| value.trim())
    }
}

/// Reports a typed Markdown read, frontmatter, or persistence failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MarkdownFileError {
    #[error("Cannot read Markdown file {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Cannot create Markdown file {}: {source}", path.display())]
    Create {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Cannot write Markdown file {}: {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Cannot parse YAML frontmatter in {}: {source}", path.display())]
    ParseFrontmatter {
        path: PathBuf,
        #[source]
        source: FrontmatterParseError,
    },
    #[error("Cannot serialize YAML frontmatter for {}: {source}", path.display())]
    SerializeFrontmatter {
        path: PathBuf,
        #[source]
        source: FrontmatterSerializeError,
    },
    #[error("YAML frontmatter must be a mapping in {}", path.display())]
    FrontmatterMustBeMapping { path: PathBuf },
    #[error("Markdown file has an unterminated YAML frontmatter block: {}", path.display())]
    MalformedFrontmatter { path: PathBuf },
    #[error(
        "YAML frontmatter in {} exceeds the {byte_count_max} byte limit",
        path.display()
    )]
    FrontmatterTooLarge {
        path: PathBuf,
        byte_count_max: usize,
    },
    #[error("Invalid YAML frontmatter property name {name:?}")]
    InvalidPropertyName { name: String },
    #[error("YAML frontmatter property {name:?} appears more than once in {}", path.display())]
    DuplicateProperty { path: PathBuf, name: String },
}

impl MarkdownFileError {
    /// Returns whether no-clobber creation found an existing destination.
    #[must_use]
    pub fn is_already_exists(&self) -> bool {
        matches!(
            self,
            Self::Create { source, .. } if source.kind() == io::ErrorKind::AlreadyExists
        )
    }

    pub(super) fn into_io_error(self) -> io::Error {
        match self {
            Self::Read { source, .. }
            | Self::Create { source, .. }
            | Self::Write { source, .. } => source,
            error => io::Error::other(error),
        }
    }
}

/// Retains a frontmatter parser failure without exposing its provider.
#[derive(Debug)]
pub struct FrontmatterParseError(gray_matter::Error);

impl fmt::Display for FrontmatterParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for FrontmatterParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Retains a frontmatter serializer failure without exposing its provider.
#[derive(Debug)]
pub struct FrontmatterSerializeError(serde_json::Error);

impl fmt::Display for FrontmatterSerializeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for FrontmatterSerializeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

#[derive(Clone)]
struct FrontmatterBounds {
    properties: Range<usize>,
    body_start: usize,
    newline: &'static str,
}

fn render_new_source(
    path: &Path,
    properties: serde_json::Map<String, serde_json::Value>,
    body: &str,
) -> Result<String, MarkdownFileError> {
    let mut source = String::from("---\n");
    for (name, value) in properties {
        validate_property_name(&name)?;
        let rendered = serde_json::to_string(&value).map_err(|source| {
            MarkdownFileError::SerializeFrontmatter {
                path: path.to_path_buf(),
                source: FrontmatterSerializeError(source),
            }
        })?;
        source.push_str(&name);
        source.push_str(": ");
        source.push_str(&rendered);
        source.push('\n');
    }
    source.push_str("---\n\n");
    source.push_str(body);
    Ok(source)
}

fn set_property_rendered_source(
    source: &mut String,
    path: &Path,
    name: &str,
    value: Option<&str>,
    insert_after: &[&str],
) -> Result<bool, MarkdownFileError> {
    validate_property_name(name)?;
    let Some(bounds) = frontmatter_bounds(path, source)? else {
        add_property_to_document_without_frontmatter(source, name, value);
        return Ok(false);
    };
    let frontmatter = &source[bounds.properties.clone()];
    let ranges = target_property_ranges(path, frontmatter, name, insert_after)?;
    match value {
        Some(value) => Ok(set_rendered_property(source, &bounds, name, value, ranges)),
        None => Ok(remove_rendered_property(source, &bounds, ranges.existing)),
    }
}

fn add_property_to_document_without_frontmatter(
    source: &mut String,
    name: &str,
    value: Option<&str>,
) {
    let Some(value) = value else {
        return;
    };
    *source = add_frontmatter(source, name, value);
}

fn remove_rendered_property(
    source: &mut String,
    bounds: &FrontmatterBounds,
    range: Option<Range<usize>>,
) -> bool {
    let Some(mut range) = range else {
        return false;
    };
    range.start += bounds.properties.start;
    range.end += bounds.properties.start;
    if source.as_bytes().get(range.end) == Some(&b'\n') {
        range.end += 1;
    }
    source.replace_range(range, "");
    true
}

fn set_rendered_property(
    source: &mut String,
    bounds: &FrontmatterBounds,
    name: &str,
    value: &str,
    ranges: TargetPropertyRanges,
) -> bool {
    let line = format!("{name}: {value}");
    if let Some(range) = ranges.existing {
        replace_rendered_property(source, bounds, range, line);
        return true;
    }
    if let Some(range) = ranges.anchor {
        insert_rendered_property_after(source, bounds, range, &line);
        return false;
    }
    source.insert_str(bounds.properties.end, &format!("{line}{}", bounds.newline));
    false
}

fn replace_rendered_property(
    source: &mut String,
    bounds: &FrontmatterBounds,
    mut range: Range<usize>,
    line: String,
) {
    let carriage_return = source
        [bounds.properties.start + range.start..bounds.properties.start + range.end]
        .ends_with('\r');
    range.start += bounds.properties.start;
    range.end += bounds.properties.start;
    let replacement = if carriage_return {
        format!("{line}\r")
    } else {
        line
    };
    source.replace_range(range, &replacement);
}

fn insert_rendered_property_after(
    source: &mut String,
    bounds: &FrontmatterBounds,
    range: Range<usize>,
    line: &str,
) {
    let absolute_end = bounds.properties.start + range.end;
    let carriage_return = source.as_bytes().get(absolute_end - 1) == Some(&b'\r');
    let insertion = absolute_end - usize::from(carriage_return);
    source.insert_str(insertion, &format!("{}{line}", bounds.newline));
}

struct FrontmatterReadState {
    source: Vec<u8>,
    first_line: bool,
    line_start: usize,
}

impl FrontmatterReadState {
    fn new() -> Self {
        Self {
            source: Vec::with_capacity(256),
            first_line: true,
            line_start: 0,
        }
    }
}

#[derive(Clone, Copy)]
enum FrontmatterReadProgress {
    Continue,
    NotFrontmatter,
    Complete,
}

fn read_frontmatter_source(path: &Path) -> Result<Option<String>, MarkdownFileError> {
    let mut file = fs::File::open(path).map_err(|source| MarkdownFileError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut buffer = [0_u8; 4096];
    let mut state = FrontmatterReadState::new();

    loop {
        let byte_count = file
            .read(&mut buffer)
            .map_err(|source| MarkdownFileError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        if byte_count == 0 {
            return finish_frontmatter_read(path, state);
        }
        match inspect_frontmatter_chunk(path, &buffer[..byte_count], &mut state)? {
            FrontmatterReadProgress::Continue => {}
            FrontmatterReadProgress::NotFrontmatter => return Ok(None),
            FrontmatterReadProgress::Complete => {
                return frontmatter_string(path, state.source).map(Some);
            }
        }
    }
}

fn inspect_frontmatter_chunk(
    path: &Path,
    chunk: &[u8],
    state: &mut FrontmatterReadState,
) -> Result<FrontmatterReadProgress, MarkdownFileError> {
    for &byte in chunk {
        let progress = inspect_frontmatter_byte(path, byte, state)?;
        if !matches!(progress, FrontmatterReadProgress::Continue) {
            return Ok(progress);
        }
    }
    Ok(FrontmatterReadProgress::Continue)
}

fn inspect_frontmatter_byte(
    path: &Path,
    byte: u8,
    state: &mut FrontmatterReadState,
) -> Result<FrontmatterReadProgress, MarkdownFileError> {
    state.source.push(byte);
    if state.source.len() > FRONTMATTER_BYTE_COUNT_MAX {
        return Err(MarkdownFileError::FrontmatterTooLarge {
            path: path.to_path_buf(),
            byte_count_max: FRONTMATTER_BYTE_COUNT_MAX,
        });
    }
    if opening_fence_is_impossible(byte, state) {
        return Ok(FrontmatterReadProgress::NotFrontmatter);
    }
    if byte != b'\n' {
        return Ok(FrontmatterReadProgress::Continue);
    }
    let line = inspect_frontmatter_line(path, &state.source[state.line_start..], state.first_line)?;
    if matches!(line, FrontmatterLine::Continue) {
        state.first_line = false;
        state.line_start = state.source.len();
    }
    Ok(match line {
        FrontmatterLine::NotFrontmatter => FrontmatterReadProgress::NotFrontmatter,
        FrontmatterLine::Continue => FrontmatterReadProgress::Continue,
        FrontmatterLine::Complete => FrontmatterReadProgress::Complete,
    })
}

fn opening_fence_is_impossible(byte: u8, state: &FrontmatterReadState) -> bool {
    state.first_line
        && byte != b'\n'
        && !can_still_be_opening_fence(&state.source[state.line_start..])
}

fn finish_frontmatter_read(
    path: &Path,
    state: FrontmatterReadState,
) -> Result<Option<String>, MarkdownFileError> {
    if state.first_line && state.source.is_empty() {
        return Ok(None);
    }
    if state.line_start < state.source.len() {
        return finish_partial_frontmatter_line(path, state);
    }
    Err(MarkdownFileError::MalformedFrontmatter {
        path: path.to_path_buf(),
    })
}

fn finish_partial_frontmatter_line(
    path: &Path,
    state: FrontmatterReadState,
) -> Result<Option<String>, MarkdownFileError> {
    match inspect_frontmatter_line(path, &state.source[state.line_start..], state.first_line)? {
        FrontmatterLine::NotFrontmatter => Ok(None),
        FrontmatterLine::Complete => frontmatter_string(path, state.source).map(Some),
        FrontmatterLine::Continue => Err(MarkdownFileError::MalformedFrontmatter {
            path: path.to_path_buf(),
        }),
    }
}

#[derive(Clone, Copy)]
enum FrontmatterLine {
    NotFrontmatter,
    Continue,
    Complete,
}

fn inspect_frontmatter_line(
    path: &Path,
    line: &[u8],
    first_line: bool,
) -> Result<FrontmatterLine, MarkdownFileError> {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let line = std::str::from_utf8(line).map_err(|source| MarkdownFileError::Read {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidData, source),
    })?;
    if first_line {
        let line = line.strip_prefix(UTF8_BYTE_ORDER_MARK).unwrap_or(line);
        return Ok(if is_frontmatter_fence(line) {
            FrontmatterLine::Continue
        } else {
            FrontmatterLine::NotFrontmatter
        });
    }
    Ok(if is_frontmatter_fence(line) {
        FrontmatterLine::Complete
    } else {
        FrontmatterLine::Continue
    })
}

fn can_still_be_opening_fence(line: &[u8]) -> bool {
    const UTF8_BYTE_ORDER_MARK_BYTES: &[u8] = b"\xef\xbb\xbf";
    if line.len() < UTF8_BYTE_ORDER_MARK_BYTES.len() && UTF8_BYTE_ORDER_MARK_BYTES.starts_with(line)
    {
        return true;
    }
    let line = line
        .strip_prefix(UTF8_BYTE_ORDER_MARK_BYTES)
        .unwrap_or(line);
    line.iter().enumerate().all(|(index, byte)| {
        if index < 3 {
            *byte == b'-'
        } else {
            matches!(byte, b' ' | b'\t') || (*byte == b'\r' && index + 1 == line.len())
        }
    })
}

fn frontmatter_string(path: &Path, source: Vec<u8>) -> Result<String, MarkdownFileError> {
    String::from_utf8(source).map_err(|source| MarkdownFileError::Read {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidData, source),
    })
}

fn frontmatter_view<'document>(
    path: &'document Path,
    source: &'document str,
) -> Result<Option<FrontmatterView<'document>>, MarkdownFileError> {
    let Some(bounds) = frontmatter_bounds(path, source)? else {
        return Ok(None);
    };
    let source = &source[bounds.properties];
    let properties = frontmatter_properties(path, source)?;
    Ok(Some(FrontmatterView {
        path,
        source,
        properties,
    }))
}

fn frontmatter_properties(
    path: &Path,
    frontmatter: &str,
) -> Result<Vec<FrontmatterProperty>, MarkdownFileError> {
    let property_count = markdown_line::lines(frontmatter)
        .filter_map(|line| property_name(line.text))
        .count();
    let mut properties = Vec::<FrontmatterProperty>::with_capacity(property_count);
    for line in markdown_line::lines(frontmatter) {
        let Some(name) = property_name(line.text) else {
            continue;
        };
        if properties
            .iter()
            .any(|property| property.name(frontmatter) == name)
        {
            return Err(MarkdownFileError::DuplicateProperty {
                path: path.to_path_buf(),
                name: name.to_string(),
            });
        }
        let range = property_range(frontmatter, line);
        properties.push(FrontmatterProperty { range });
    }
    Ok(properties)
}

fn target_property_ranges(
    path: &Path,
    frontmatter: &str,
    name: &str,
    insert_after: &[&str],
) -> Result<TargetPropertyRanges, MarkdownFileError> {
    let property_count = markdown_line::lines(frontmatter)
        .filter_map(|line| property_name(line.text))
        .count();
    let mut names = Vec::with_capacity(property_count);
    let mut existing = None;
    let mut anchor = None::<(usize, Range<usize>)>;
    for line in markdown_line::lines(frontmatter) {
        let Some(property_name) = property_name(line.text) else {
            continue;
        };
        if names.contains(&property_name) {
            return Err(MarkdownFileError::DuplicateProperty {
                path: path.to_path_buf(),
                name: property_name.to_string(),
            });
        }
        names.push(property_name);

        let anchor_index = insert_after
            .iter()
            .position(|candidate| *candidate == property_name);
        if property_name != name && anchor_index.is_none() {
            continue;
        }
        let range = property_range(frontmatter, line);
        if property_name == name {
            existing = Some(range.clone());
        }
        if let Some(anchor_index) = anchor_index
            && anchor
                .as_ref()
                .is_none_or(|(current_index, _)| anchor_index < *current_index)
        {
            anchor = Some((anchor_index, range));
        }
    }
    Ok(TargetPropertyRanges {
        existing,
        anchor: anchor.map(|(_, range)| range),
    })
}

struct TargetPropertyRanges {
    existing: Option<Range<usize>>,
    anchor: Option<Range<usize>>,
}

fn frontmatter_bounds(
    path: &Path,
    source: &str,
) -> Result<Option<FrontmatterBounds>, MarkdownFileError> {
    let without_byte_order_mark = source.strip_prefix(UTF8_BYTE_ORDER_MARK).unwrap_or(source);
    let byte_order_mark_length = source.len() - without_byte_order_mark.len();
    let Some(opening) = markdown_line::lines(without_byte_order_mark).next() else {
        return Ok(None);
    };
    if opening.start != 0 || !is_frontmatter_fence(opening.text) {
        return Ok(None);
    }
    let Some(closing) = markdown_line::lines(without_byte_order_mark)
        .filter(|line| line.start >= opening.end)
        .find(|line| is_frontmatter_fence(line.text))
    else {
        return Err(MarkdownFileError::MalformedFrontmatter {
            path: path.to_path_buf(),
        });
    };
    Ok(Some(FrontmatterBounds {
        properties: byte_order_mark_length + opening.end..byte_order_mark_length + closing.start,
        body_start: byte_order_mark_length + closing.end,
        newline: if opening.newline.is_empty() {
            "\n"
        } else {
            opening.newline
        },
    }))
}

fn is_frontmatter_fence(line: &str) -> bool {
    line.strip_prefix("---").is_some_and(|suffix| {
        suffix
            .chars()
            .all(|character| matches!(character, ' ' | '\t'))
    })
}

fn validate_property_name(name: &str) -> Result<(), MarkdownFileError> {
    let mut characters = name.chars();
    let valid = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
        && characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));
    if valid {
        Ok(())
    } else {
        Err(MarkdownFileError::InvalidPropertyName {
            name: name.to_string(),
        })
    }
}

fn property_name(line: &str) -> Option<&str> {
    if line.starts_with([' ', '\t', '#', '-']) {
        return None;
    }
    let (name, _) = line.split_once(':')?;
    let name = name.trim_end();
    (!name.is_empty()).then_some(name)
}

fn property_range(frontmatter: &str, first: markdown_line::MarkdownLine<'_>) -> Range<usize> {
    let has_block_value = first
        .text
        .split_once(':')
        .is_some_and(|(_, value)| value.trim().is_empty() || value.trim().starts_with(['|', '>']));
    let end = markdown_line::lines(&frontmatter[first.end..])
        .take_while(|line| property_continues(has_block_value, line.text))
        .map(|line| first.end + line.content_end)
        .last()
        .unwrap_or(first.content_end);
    first.start..end
}

fn property_continues(has_block_value: bool, line: &str) -> bool {
    if has_block_value {
        return property_name(line).is_none();
    }
    line.starts_with([' ', '\t'])
}

fn add_frontmatter(source: &str, name: &str, value: &str) -> String {
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let (byte_order_mark, body) = source
        .strip_prefix(UTF8_BYTE_ORDER_MARK)
        .map_or(("", source), |body| ("\u{feff}", body));
    format!("{byte_order_mark}---{newline}{name}: {value}{newline}---{newline}{newline}{body}")
}

fn write_text_atomic_new(path: &Path, source: &str) -> io::Result<()> {
    let temporary = prepare_temporary_file(path, source)?;
    temporary
        .persist_noclobber(path)
        .map(|_| ())
        .map_err(|error| error.error)
}

fn prepare_temporary_file(path: &Path, source: &str) -> io::Result<tempfile::NamedTempFile> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.exists() {
        fs::create_dir_all(parent)?;
    }
    let mut builder = tempfile::Builder::new();
    builder.prefix(".markdown-").suffix(".tmp");
    let destination_metadata = fs::metadata(path);
    if let Ok(metadata) = &destination_metadata {
        builder.permissions(metadata.permissions());
    }
    #[cfg(unix)]
    if destination_metadata.is_err() {
        use std::os::unix::fs::PermissionsExt as _;

        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let mut temporary = builder.tempfile_in(parent)?;
    temporary.write_all(source.as_bytes())?;
    temporary.as_file_mut().sync_all()?;
    Ok(temporary)
}
