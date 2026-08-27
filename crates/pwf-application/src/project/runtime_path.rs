use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use pwf_models::project::HomeDirectory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    path: PathBuf,
    identity: RuntimePathIdentity,
}

impl ResolvedPath {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn identity(&self) -> &RuntimePathIdentity {
        &self.identity
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuntimePathIdentity(OsString);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePathError {
    #[error("path must not be empty")]
    Empty,
    #[error("path must not contain repeated separators")]
    RepeatedSeparator,
    #[error("path must not end with a separator")]
    TrailingSeparator,
    #[error("path must not contain `.` or `..` components")]
    DotComponent,
    #[error("drive-relative paths are not supported")]
    DriveRelative,
    #[error("home-relative path must contain only normal path components")]
    HomeRelativeComponent,
    #[cfg(windows)]
    #[error("path prefix is not supported")]
    UnsupportedPrefix,
    #[error("home directory must be absolute")]
    RelativeHome,
}

/// Resolves one managed-project path for runtime use.
pub fn resolve(path: &str, home: &HomeDirectory) -> Result<ResolvedPath, RuntimePathError> {
    host::resolve(path, home.as_path())
}

fn home_relative_remainder(path: &str) -> Option<&str> {
    path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\"))
}

#[cfg(unix)]
mod host {
    use std::{
        ffi::{OsStr, OsString},
        os::unix::ffi::{OsStrExt, OsStringExt},
        path::{Component, Path, PathBuf},
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
            let bytes = path.as_bytes();
            if bytes.is_empty() {
                return Err(RuntimePathError::Empty);
            }

            let (root, remainder) = parse_root(bytes)?;
            let root_only = root != PathRoot::Relative && remainder.is_empty();
            let components = parse_components(remainder, root_only)?;

            Ok(Self { root, components })
        }

        fn append(&mut self, components: Vec<OsString>) {
            self.components.extend(components);
        }

        fn identity(&self) -> RuntimePathIdentity {
            let mut identity = match self.root {
                PathRoot::Relative => b"relative:".to_vec(),
                PathRoot::Unix => b"unix:/".to_vec(),
                PathRoot::Drive(drive) => {
                    vec![
                        b'd',
                        b'r',
                        b'i',
                        b'v',
                        b'e',
                        b':',
                        drive.to_ascii_lowercase(),
                        b':',
                        b'/',
                    ]
                }
            };

            for (index, component) in self.components.iter().enumerate() {
                if index > 0 {
                    identity.push(b'/');
                }
                let mut bytes = component.as_bytes().to_vec();
                if matches!(self.root, PathRoot::Drive(_)) {
                    bytes.make_ascii_lowercase();
                }
                identity.extend(bytes);
            }

            RuntimePathIdentity(OsString::from_vec(identity))
        }

        fn to_path_buf(&self) -> PathBuf {
            let mut bytes = match self.root {
                PathRoot::Relative => Vec::new(),
                PathRoot::Unix => vec![b'/'],
                PathRoot::Drive(drive) => {
                    vec![drive.to_ascii_uppercase(), b':', b'/']
                }
            };

            for (index, component) in self.components.iter().enumerate() {
                if index > 0 {
                    bytes.push(b'/');
                }
                bytes.extend(component.as_bytes());
            }

            PathBuf::from(OsString::from_vec(bytes))
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
        let bytes = OsStr::new(remainder).as_bytes();
        if bytes.first().is_some_and(|byte| is_separator(*byte)) {
            return Err(RuntimePathError::RepeatedSeparator);
        }
        let components = parse_components(bytes, false)?;
        if components
            .iter()
            .any(|component| has_portable_drive_prefix(component) || !is_normal(component))
        {
            return Err(RuntimePathError::HomeRelativeComponent);
        }
        Ok(components)
    }

    fn parse_root(bytes: &[u8]) -> Result<(PathRoot, &[u8]), RuntimePathError> {
        if is_separator(bytes[0]) {
            if bytes.get(1).is_some_and(|byte| is_separator(*byte)) {
                return Err(RuntimePathError::RepeatedSeparator);
            }
            return Ok((PathRoot::Unix, &bytes[1..]));
        }

        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            let remainder = &bytes[2..];
            let Some(remainder) = remainder
                .first()
                .filter(|byte| is_separator(**byte))
                .map(|_| &remainder[1..])
            else {
                return Err(RuntimePathError::DriveRelative);
            };
            if remainder.first().is_some_and(|byte| is_separator(*byte)) {
                return Err(RuntimePathError::RepeatedSeparator);
            }
            return Ok((PathRoot::Drive(bytes[0]), remainder));
        }

        Ok((PathRoot::Relative, bytes))
    }

    fn parse_components(
        remainder: &[u8],
        root_only: bool,
    ) -> Result<Vec<OsString>, RuntimePathError> {
        if remainder.is_empty() {
            return if root_only {
                Ok(Vec::new())
            } else {
                Err(RuntimePathError::Empty)
            };
        }
        if remainder.last().is_some_and(|byte| is_separator(*byte)) {
            return Err(RuntimePathError::TrailingSeparator);
        }

        remainder
            .split(|byte| is_separator(*byte))
            .map(|component| {
                if component.is_empty() {
                    return Err(RuntimePathError::RepeatedSeparator);
                }
                if matches!(component, b"." | b"..") {
                    return Err(RuntimePathError::DotComponent);
                }
                Ok(OsString::from_vec(component.to_vec()))
            })
            .collect()
    }

    fn is_normal(component: &OsStr) -> bool {
        let mut components = Path::new(component).components();
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
    }

    fn has_portable_drive_prefix(component: &OsStr) -> bool {
        let bytes = component.as_bytes();
        bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
    }

    fn is_separator(byte: u8) -> bool {
        matches!(byte, b'/' | b'\\')
    }
}

#[cfg(windows)]
mod host {
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
                Component::Normal(component) => {
                    (PathRoot::Relative, vec![component.to_os_string()])
                }
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
            let mut identity = match self.root {
                PathRoot::Relative => "relative:".encode_utf16().collect(),
                PathRoot::Unix => "unix:/".encode_utf16().collect(),
                PathRoot::Drive(drive) => {
                    format!("drive:{}:/", char::from(drive).to_ascii_lowercase())
                        .encode_utf16()
                        .collect()
                }
            };

            for (index, component) in self.components.iter().enumerate() {
                if index > 0 {
                    identity.push(u16::from(b'/'));
                }
                let component = component.encode_wide().map(|unit| {
                    if matches!(self.root, PathRoot::Drive(_)) {
                        lower_ascii(unit)
                    } else {
                        unit
                    }
                });
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
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use pwf_models::project::HomeDirectory;

    use super::{ResolvedPath, RuntimePathError};

    fn resolve(path: &str, home: &Path) -> Result<ResolvedPath, RuntimePathError> {
        super::resolve(path, &HomeDirectory::new(PathBuf::from(home)))
    }

    #[test]
    fn home_relative_and_absolute_paths_have_the_same_identity() {
        let home = Path::new("/home/tester");
        let home_relative = resolve("~/tasks/shared", home).unwrap();
        let absolute = resolve("/home/tester/tasks/shared", home).unwrap();

        assert_eq!(home_relative.identity(), absolute.identity());
        assert_eq!(home_relative.path(), Path::new("/home/tester/tasks/shared"));
    }

    #[test]
    fn windows_and_mixed_separators_have_a_portable_identity() {
        let home = Path::new(r"C:\Users\tester");
        let home_relative = resolve(r"~\tasks/shared", home).unwrap();
        let absolute = resolve(r"C:/Users\tester\tasks\shared", home).unwrap();

        assert_eq!(home_relative.identity(), absolute.identity());
    }

    #[test]
    fn repeated_separators_are_rejected() {
        for raw in [
            "~//tmp/tasks",
            r"~\\tmp\tasks",
            r"~/\tmp/tasks",
            r"~\/tmp/tasks",
            "~/tasks//shared",
            r"~\tasks\\shared",
        ] {
            let error = resolve(raw, Path::new("/home/tester")).unwrap_err();

            assert_eq!(
                error.to_string(),
                "path must not contain repeated separators",
                "accepted or misclassified {raw:?}"
            );
        }
    }

    #[test]
    fn home_relative_windows_prefixes_are_rejected_portably() {
        for raw in [
            "~/D:/tasks",
            "~/D:tasks",
            r"~\D:\tasks",
            r"~\D:tasks",
            "~/nested/D:/tasks",
        ] {
            let error = resolve(raw, Path::new("/home/tester")).unwrap_err();

            assert_eq!(
                error.to_string(),
                "home-relative path must contain only normal path components",
                "accepted or misclassified {raw:?}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn native_home_relative_prefixes_and_roots_are_rejected() {
        for raw in [
            r"~\D:\tasks",
            r"~\D:tasks",
            r"~\\server\share\tasks",
            r"~\\?\D:\tasks",
            r"~\nested\D:\tasks",
        ] {
            assert!(
                resolve(raw, Path::new(r"C:\Users\tester")).is_err(),
                "accepted {raw:?}"
            );
        }
    }

    #[test]
    fn dot_components_are_rejected_for_home_and_absolute_paths() {
        for raw in [
            "~/tasks/./shared",
            "~/tasks/../shared",
            r"~\tasks\..\shared",
            "/srv/tasks/./shared",
            r"C:\tasks\..\shared",
        ] {
            let error = resolve(raw, Path::new("/home/tester")).unwrap_err();

            assert_eq!(
                error.to_string(),
                "path must not contain `.` or `..` components",
                "accepted or misclassified {raw:?}"
            );
        }
    }

    #[test]
    fn safe_non_home_absolute_paths_remain_unchanged() {
        let resolved = resolve("/srv/pwf/tasks", Path::new("/home/tester")).unwrap();

        assert_eq!(resolved.path(), Path::new("/srv/pwf/tasks"));
    }
}
