use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf, Prefix},
};

use super::{ResolvedPath, RuntimePathError, RuntimePathIdentity, home_relative_remainder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathRoot {
    Relative,
    Unix,
    Drive(u8),
}

struct ParsedPath {
    root: PathRoot,
    components: Vec<OsString>,
}

impl ParsedPath {
    fn parse(path: &OsStr) -> Result<Self, RuntimePathError> {
        validate_separator_layout(path)?;
        let mut native_components = Path::new(path).components();
        let Some(first) = native_components.next() else {
            return Err(RuntimePathError::Empty);
        };

        let (root, mut components) = match first {
            Component::Prefix(prefix) => {
                let Prefix::Disk(drive) = prefix.kind() else {
                    return Err(RuntimePathError::UnsupportedPrefix);
                };
                if !matches!(native_components.next(), Some(Component::RootDir)) {
                    return Err(RuntimePathError::DriveRelative);
                }
                (PathRoot::Drive(drive), Vec::new())
            }
            Component::RootDir => (PathRoot::Unix, Vec::new()),
            Component::CurDir | Component::ParentDir => {
                return Err(RuntimePathError::DotComponent);
            }
            Component::Normal(component) => (PathRoot::Relative, vec![component.to_os_string()]),
        };

        for component in native_components {
            match component {
                Component::Normal(component) => components.push(component.to_os_string()),
                Component::CurDir | Component::ParentDir => {
                    return Err(RuntimePathError::DotComponent);
                }
                Component::Prefix(_) | Component::RootDir => {
                    return Err(RuntimePathError::UnsupportedPrefix);
                }
            }
        }

        let root_only = root != PathRoot::Relative && components.is_empty();
        if ends_with_separator(path) && !root_only {
            return Err(RuntimePathError::TrailingSeparator);
        }

        Ok(Self { root, components })
    }

    fn append(&mut self, components: Vec<OsString>) {
        self.components.extend(components);
    }

    fn identity(&self) -> RuntimePathIdentity {
        let mut identity: Vec<u16> = match self.root {
            PathRoot::Relative => "relative:".encode_utf16().collect(),
            PathRoot::Unix => "unix:/".encode_utf16().collect(),
            PathRoot::Drive(drive) => format!("drive:{}:/", char::from(drive).to_ascii_lowercase())
                .encode_utf16()
                .collect(),
        };

        let normalize: fn(u16) -> u16 = match self.root {
            PathRoot::Drive(_) => lower_ascii,
            _ => std::convert::identity,
        };
        for (index, component) in self.components.iter().enumerate() {
            if index > 0 {
                identity.push(u16::from(b'/'));
            }
            let component = component.encode_wide().map(normalize);
            identity.extend(component);
        }

        RuntimePathIdentity(OsString::from_wide(&identity))
    }

    fn to_path_buf(&self) -> PathBuf {
        let mut path = match self.root {
            PathRoot::Relative => Vec::new(),
            PathRoot::Unix => vec![u16::from(b'\\')],
            PathRoot::Drive(drive) => vec![
                u16::from(drive.to_ascii_uppercase()),
                u16::from(b':'),
                u16::from(b'\\'),
            ],
        };

        for (index, component) in self.components.iter().enumerate() {
            if index > 0 {
                path.push(u16::from(b'\\'));
            }
            path.extend(component.encode_wide());
        }

        PathBuf::from(OsString::from_wide(&path))
    }
}

pub(super) fn resolve(path: &str, home: &Path) -> Result<ResolvedPath, RuntimePathError> {
    let parsed = if path == "~" {
        parse_home(home)?
    } else if let Some(remainder) = home_relative_remainder(path) {
        let components = parse_home_relative(remainder)?;
        let mut home = parse_home(home)?;
        home.append(components);
        home
    } else {
        ParsedPath::parse(OsStr::new(path))?
    };

    Ok(ResolvedPath {
        path: parsed.to_path_buf(),
        identity: parsed.identity(),
    })
}

fn parse_home(home: &Path) -> Result<ParsedPath, RuntimePathError> {
    let home = ParsedPath::parse(home.as_os_str())?;
    if home.root == PathRoot::Relative {
        return Err(RuntimePathError::RelativeHome);
    }
    Ok(home)
}

fn parse_home_relative(remainder: &str) -> Result<Vec<OsString>, RuntimePathError> {
    let remainder = OsStr::new(remainder);
    if starts_with_separator(remainder) {
        return Err(RuntimePathError::RepeatedSeparator);
    }
    validate_separator_layout(remainder)?;
    if ends_with_separator(remainder) {
        return Err(RuntimePathError::TrailingSeparator);
    }

    let mut components = Vec::new();
    for component in Path::new(remainder).components() {
        let Component::Normal(component) = component else {
            return match component {
                Component::CurDir | Component::ParentDir => Err(RuntimePathError::DotComponent),
                _ => Err(RuntimePathError::HomeRelativeComponent),
            };
        };
        if has_portable_drive_prefix(component) || !is_normal(component) {
            return Err(RuntimePathError::HomeRelativeComponent);
        }
        components.push(component.to_os_string());
    }
    if components.is_empty() {
        return Err(RuntimePathError::Empty);
    }
    Ok(components)
}

fn validate_separator_layout(path: &OsStr) -> Result<(), RuntimePathError> {
    let units: Vec<_> = path.encode_wide().collect();
    let mut previous_separator = false;
    for &unit in &units {
        let separator = is_separator(unit);
        if separator && previous_separator {
            return Err(RuntimePathError::RepeatedSeparator);
        }
        previous_separator = separator;
    }
    if units.split(|unit| is_separator(*unit)).any(|component| {
        component == [u16::from(b'.')] || component == [u16::from(b'.'), u16::from(b'.')]
    }) {
        return Err(RuntimePathError::DotComponent);
    }
    Ok(())
}

fn starts_with_separator(path: &OsStr) -> bool {
    path.encode_wide().next().is_some_and(is_separator)
}

fn ends_with_separator(path: &OsStr) -> bool {
    path.encode_wide().last().is_some_and(is_separator)
}

fn is_normal(component: &OsStr) -> bool {
    let mut components = Path::new(component).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn has_portable_drive_prefix(component: &OsStr) -> bool {
    let mut units = component.encode_wide();
    let Some(first) = units.next() else {
        return false;
    };
    let Some(second) = units.next() else {
        return false;
    };
    is_ascii_alphabetic(first) && second == u16::from(b':')
}

fn lower_ascii(unit: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&unit) {
        unit + u16::from(b'a' - b'A')
    } else {
        unit
    }
}

fn is_ascii_alphabetic(unit: u16) -> bool {
    (u16::from(b'A')..=u16::from(b'Z')).contains(&unit)
        || (u16::from(b'a')..=u16::from(b'z')).contains(&unit)
}

fn is_separator(unit: u16) -> bool {
    unit == u16::from(b'/') || unit == u16::from(b'\\')
}
