use std::{
    fmt, fs,
    io::{self, Read as _, Write as _},
    ops::Range,
    path::{Path, PathBuf},
};

use gray_matter::engine::{Engine as _, YAML};
use serde::{Serialize, de::DeserializeOwned};

use super::markdown_line;

const UTF8_BYTE_ORDER_MARK: char = '\u{feff}';
const FRONTMATTER_BYTE_COUNT_MAX: usize = 1024 * 1024;

/// Owns one UTF-8 Markdown document and its filesystem path.
#[derive(Debug)]
pub struct MarkdownFile {
    path: PathBuf,
    source: String,
}

impl MarkdownFile {
    /// Reads a Markdown file without normalizing its contents.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, MarkdownFileError> {
        let path = path.into();
        match fs::read_to_string(&path) {
            Ok(source) => Ok(Self { path, source }),
            Err(source) => Err(MarkdownFileError::Read { path, source }),
        }
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
        let mut source = String::from("---\n");
        for (name, value) in properties {
            validate_property_name(&name)?;
            let rendered = serde_json::to_string(&value).map_err(|source| {
                MarkdownFileError::SerializeFrontmatter {
                    path: path.clone(),
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
        Self::create_rendered_new(path, source)
    }

    /// Returns the file's current path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the complete in-memory Markdown source.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the body after the closing frontmatter fence.
    ///
    /// A document without a complete opening frontmatter block is entirely body text.
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
        write_text_atomic(&self.path, &self.source).map_err(|source| MarkdownFileError::Write {
            path: self.path.clone(),
            source,
        })
    }

    pub(super) fn from_source(path: impl Into<PathBuf>, source: String) -> Self {
        Self {
            path: path.into(),
            source,
        }
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
        Self::from_source(path, source).save()
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
        validate_property_name(name)?;
        let bounds = frontmatter_bounds(&self.path, &self.source)?;
        let Some(bounds) = bounds else {
            if let Some(value) = value {
                self.source = add_frontmatter(&self.source, name, value);
            }
            return Ok(false);
        };
        let TargetPropertyRanges { existing, anchor } = {
            let frontmatter = &self.source[bounds.properties.clone()];
            target_property_ranges(&self.path, frontmatter, name, insert_after)?
        };
        let Some(value) = value else {
            let Some(mut range) = existing else {
                return Ok(false);
            };
            range.start += bounds.properties.start;
            range.end += bounds.properties.start;
            if self.source.as_bytes().get(range.end) == Some(&b'\n') {
                range.end += 1;
            }
            self.source.replace_range(range, "");
            return Ok(true);
        };
        let line = format!("{name}: {value}");
        if let Some(mut range) = existing {
            let carriage_return = self.source
                [bounds.properties.start + range.start..bounds.properties.start + range.end]
                .ends_with('\r');
            range.start += bounds.properties.start;
            range.end += bounds.properties.start;
            let replacement = if carriage_return {
                format!("{line}\r")
            } else {
                line
            };
            self.source.replace_range(range, &replacement);
            return Ok(true);
        }

        if let Some(range) = anchor {
            let absolute_end = bounds.properties.start + range.end;
            let carriage_return = self.source.as_bytes().get(absolute_end - 1) == Some(&b'\r');
            let insertion = absolute_end - usize::from(carriage_return);
            self.source
                .insert_str(insertion, &format!("{}{line}", bounds.newline));
            return Ok(false);
        }

        self.source
            .insert_str(bounds.properties.end, &format!("{line}{}", bounds.newline));
        Ok(false)
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

fn read_frontmatter_source(path: &Path) -> Result<Option<String>, MarkdownFileError> {
    let mut file = fs::File::open(path).map_err(|source| MarkdownFileError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut buffer = [0_u8; 4096];
    let mut source = Vec::with_capacity(256);
    let mut first_line = true;
    let mut line_start = 0;

    loop {
        let byte_count = file
            .read(&mut buffer)
            .map_err(|source| MarkdownFileError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        if byte_count == 0 {
            if first_line && source.is_empty() {
                return Ok(None);
            }
            if line_start < source.len() {
                match inspect_frontmatter_line(path, &source[line_start..], first_line)? {
                    FrontmatterLine::NotFrontmatter => return Ok(None),
                    FrontmatterLine::Complete => return frontmatter_string(path, source).map(Some),
                    FrontmatterLine::Continue => {}
                }
            }
            return Err(MarkdownFileError::MalformedFrontmatter {
                path: path.to_path_buf(),
            });
        }

        for byte in &buffer[..byte_count] {
            source.push(*byte);
            if source.len() > FRONTMATTER_BYTE_COUNT_MAX {
                return Err(MarkdownFileError::FrontmatterTooLarge {
                    path: path.to_path_buf(),
                    byte_count_max: FRONTMATTER_BYTE_COUNT_MAX,
                });
            }
            if first_line && *byte != b'\n' && !can_still_be_opening_fence(&source[line_start..]) {
                return Ok(None);
            }
            if *byte != b'\n' {
                continue;
            }
            match inspect_frontmatter_line(path, &source[line_start..], first_line)? {
                FrontmatterLine::NotFrontmatter => return Ok(None),
                FrontmatterLine::Complete => return frontmatter_string(path, source).map(Some),
                FrontmatterLine::Continue => {
                    first_line = false;
                    line_start = source.len();
                }
            }
        }
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
    let mut end = first.content_end;
    for line in markdown_line::lines(&frontmatter[first.end..]) {
        if has_block_value {
            if property_name(line.text).is_some() {
                break;
            }
        } else if !line.text.starts_with([' ', '\t']) {
            break;
        }
        end = first.end + line.content_end;
    }
    first.start..end
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

fn write_text_atomic(path: &Path, source: &str) -> io::Result<()> {
    let temporary = prepare_temporary_file(path, source)?;
    temporary
        .persist(path)
        .map(|_| ())
        .map_err(|error| error.error)
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
