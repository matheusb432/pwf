use nutype::nutype;
use thiserror::Error;

/// Number of lowercase hexadecimal characters in one persisted-content revision.
pub const CONTENT_REVISION_LENGTH: usize = 64;

/// Reports a malformed opaque persisted-content revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("must be a 64-character lowercase hexadecimal value")]
pub struct ContentRevisionError;

/// Identifies the exact persisted bytes observed by a resource read.
#[nutype(
    validate(with = validate_content_revision, error = ContentRevisionError),
    derive(
        Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display, FromStr, TryFrom,
    )
)]
pub struct ContentRevision(String);

fn validate_content_revision(value: &str) -> Result<(), ContentRevisionError> {
    if value.len() == CONTENT_REVISION_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(ContentRevisionError)
    }
}

#[cfg(test)]
mod tests {
    use super::ContentRevision;

    #[test]
    fn content_revision_accepts_only_lowercase_hexadecimal_digest_text() {
        let valid = "0123456789abcdef".repeat(4);

        assert_eq!(
            ContentRevision::try_new(valid.clone()).unwrap().as_ref(),
            valid
        );
        for invalid in [
            "0".repeat(63),
            "0".repeat(65),
            "G".repeat(64),
            "A".repeat(64),
            "-".repeat(64),
        ] {
            assert!(ContentRevision::try_new(invalid).is_err());
        }
    }
}
