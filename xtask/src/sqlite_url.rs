use std::path::Path;

use percent_encoding::{AsciiSet, CONTROLS, percent_encode};

static PATH_ENCODE_SET: AsciiSet = CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

pub(crate) fn from_path(path: &Path) -> String {
    let path = percent_encode(path.as_os_str().as_encoded_bytes(), &PATH_ENCODE_SET);
    format!("sqlite://{path}")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn reserved_unix_path_characters_are_percent_encoded() {
        assert_eq!(
            from_path(Path::new("/tmp/pwf %?#.db")),
            "sqlite:///tmp/pwf%20%25%3F%23.db"
        );
    }

    #[test]
    fn verbatim_windows_path_cannot_expose_a_query_delimiter() {
        assert_eq!(
            from_path(Path::new(r"\\?\C:\pwf data\registry?#.db")),
            r"sqlite://%5C%5C%3F%5CC:%5Cpwf%20data%5Cregistry%3F%23.db"
        );
    }
}
