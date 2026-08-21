use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use secrecy::{ExposeSecret as _, SecretString};
use subtle::ConstantTimeEq as _;

use crate::{LocalAuthError, private_directory::PrivateDirectory};

pub(crate) const CAPABILITY_FILE_NAME: &str = "capability.token";
const CAPABILITY_BYTE_LENGTH: usize = 32;

#[derive(Clone)]
pub struct CapabilityToken(SecretString);

impl CapabilityToken {
    pub fn generate() -> Result<Self, LocalAuthError> {
        let mut bytes = [0_u8; CAPABILITY_BYTE_LENGTH];
        getrandom::fill(&mut bytes).map_err(LocalAuthError::GenerateToken)?;
        Ok(Self(SecretString::from(URL_SAFE_NO_PAD.encode(bytes))))
    }

    #[must_use]
    pub fn expose_secret(&self) -> &str {
        self.0.expose_secret()
    }

    #[must_use]
    pub fn authenticates(&self, candidate: &str) -> bool {
        self.expose_secret()
            .as_bytes()
            .ct_eq(candidate.as_bytes())
            .into()
    }

    fn parse(contents: &[u8]) -> Option<Self> {
        let encoded = std::str::from_utf8(contents).ok()?;
        let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
        if decoded.len() != CAPABILITY_BYTE_LENGTH || URL_SAFE_NO_PAD.encode(decoded) != encoded {
            return None;
        }
        Some(Self(SecretString::from(encoded.to_owned())))
    }
}

impl std::fmt::Debug for CapabilityToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CapabilityToken(REDACTED)")
    }
}

pub(crate) fn load_or_create(
    directory: &PrivateDirectory,
) -> Result<CapabilityToken, LocalAuthError> {
    let generated = CapabilityToken::generate()?;
    if directory.create(CAPABILITY_FILE_NAME, generated.expose_secret().as_bytes())? {
        return Ok(generated);
    }
    load(directory)
}

pub(crate) fn load(directory: &PrivateDirectory) -> Result<CapabilityToken, LocalAuthError> {
    let path = directory.path(CAPABILITY_FILE_NAME);
    CapabilityToken::parse(&directory.read(CAPABILITY_FILE_NAME)?)
        .ok_or(LocalAuthError::MalformedToken { path })
}
