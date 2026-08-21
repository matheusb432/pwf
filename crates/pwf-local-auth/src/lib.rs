//! Private local bootstrap credentials and endpoint discovery for `pwf-server`.

mod endpoint;
mod error;
mod private_directory;
mod token;

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
pub use endpoint::{PublishedEndpoint, ServerEndpoint, ServerInstanceId};
pub use error::LocalAuthError;
pub use token::CapabilityToken;

const DATA_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "PWF_DATA_DIR";
const SERVER_DIRECTORY_NAME: &str = "server";

#[derive(Debug, Clone)]
pub struct LocalAuth {
    data_root: PathBuf,
}

impl LocalAuth {
    /// Resolves the local bootstrap store from the process environment.
    pub fn from_environment() -> Result<Self, LocalAuthError> {
        let data_root = std::env::var_os(DATA_DIRECTORY_ENVIRONMENT_VARIABLE)
            .map(PathBuf::from)
            .or_else(|| {
                ProjectDirs::from("", "", "pwf")
                    .map(|directories| directories.data_dir().to_path_buf())
            })
            .ok_or(LocalAuthError::DataDirectoryUnavailable)?;

        Self::from_data_root(data_root)
    }

    /// Uses an explicit application data root.
    pub fn from_data_root(data_root: impl Into<PathBuf>) -> Result<Self, LocalAuthError> {
        let data_root = data_root.into();
        if !data_root.is_absolute() {
            return Err(LocalAuthError::DataDirectoryRelative { path: data_root });
        }
        Ok(Self { data_root })
    }

    /// Loads the persistent capability, creating it when the server first starts.
    pub fn load_or_create_server_token(&self) -> Result<CapabilityToken, LocalAuthError> {
        token::load_or_create(&self.server_directory()?)
    }

    /// Loads the capability provisioned by `pwf-server`.
    pub fn load_client_token(&self) -> Result<CapabilityToken, LocalAuthError> {
        token::load(&self.server_directory()?)
    }

    /// Publishes the currently listening server endpoint.
    pub fn publish_endpoint(
        &self,
        endpoint: ServerEndpoint,
    ) -> Result<PublishedEndpoint, LocalAuthError> {
        endpoint::publish(self.server_directory()?, endpoint)
    }

    /// Loads the currently published server endpoint.
    pub fn load_endpoint(&self) -> Result<ServerEndpoint, LocalAuthError> {
        endpoint::load(&self.server_directory()?)
    }

    #[must_use]
    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    fn server_directory(&self) -> Result<private_directory::PrivateDirectory, LocalAuthError> {
        private_directory::PrivateDirectory::open(self.data_root.join(SERVER_DIRECTORY_NAME))
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    use super::*;

    fn local_auth(directory: &tempfile::TempDir) -> LocalAuth {
        LocalAuth::from_data_root(directory.path()).expect("absolute temporary data root")
    }

    #[test]
    fn capability_persists_across_server_restarts() {
        let directory = tempfile::tempdir().expect("temporary data root");
        let auth = local_auth(&directory);

        let provisioned = auth
            .load_or_create_server_token()
            .expect("provision capability");
        let restarted = auth
            .load_or_create_server_token()
            .expect("load persisted capability");
        let client = auth.load_client_token().expect("load client capability");

        assert!(provisioned.authenticates(restarted.expose_secret()));
        assert!(provisioned.authenticates(client.expose_secret()));
        assert_eq!(format!("{provisioned:?}"), "CapabilityToken(REDACTED)");
    }

    #[test]
    fn endpoint_publication_is_instance_owned() {
        let directory = tempfile::tempdir().expect("temporary data root");
        let auth = local_auth(&directory);
        let endpoint = ServerEndpoint::try_new(
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4317)),
            ServerInstanceId::generate(),
        )
        .expect("loopback endpoint");

        let published = auth
            .publish_endpoint(endpoint.clone())
            .expect("publish endpoint");
        assert_eq!(auth.load_endpoint().expect("load endpoint"), endpoint);

        drop(published);
        assert!(matches!(
            auth.load_endpoint(),
            Err(LocalAuthError::EndpointNotPublished { .. })
        ));
    }

    #[test]
    fn endpoint_rejects_non_loopback_and_unbound_addresses() {
        let instance_id = ServerInstanceId::generate();

        assert!(
            ServerEndpoint::try_new("192.0.2.1:4317".parse().unwrap(), instance_id.clone())
                .is_err()
        );
        assert!(ServerEndpoint::try_new("127.0.0.1:0".parse().unwrap(), instance_id).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn capability_and_endpoint_are_user_private() {
        use std::os::unix::fs::MetadataExt as _;

        let directory = tempfile::tempdir().expect("temporary data root");
        let auth = local_auth(&directory);
        auth.load_or_create_server_token()
            .expect("provision capability");
        let endpoint = ServerEndpoint::try_new(
            "127.0.0.1:4317".parse().unwrap(),
            ServerInstanceId::generate(),
        )
        .expect("loopback endpoint");
        let _published = auth.publish_endpoint(endpoint).expect("publish endpoint");
        let server_directory = auth.data_root().join(SERVER_DIRECTORY_NAME);

        assert_eq!(
            std::fs::metadata(&server_directory).unwrap().mode() & 0o777,
            0o700
        );
        for name in [token::CAPABILITY_FILE_NAME, endpoint::ENDPOINT_FILE_NAME] {
            assert_eq!(
                std::fs::metadata(server_directory.join(name))
                    .unwrap()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
