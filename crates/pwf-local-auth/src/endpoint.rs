use std::{net::SocketAddr, sync::Arc};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{LocalAuthError, private_directory::PrivateDirectory};

pub(crate) const ENDPOINT_FILE_NAME: &str = "endpoint.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInstanceId(Uuid);

impl ServerInstanceId {
    #[must_use]
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for ServerInstanceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEndpoint {
    address: SocketAddr,
    instance_id: ServerInstanceId,
}

impl ServerEndpoint {
    pub fn try_new(
        address: SocketAddr,
        instance_id: ServerInstanceId,
    ) -> Result<Self, LocalAuthError> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(LocalAuthError::InvalidEndpoint);
        }
        Ok(Self {
            address,
            instance_id,
        })
    }

    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    #[must_use]
    pub const fn instance_id(&self) -> &ServerInstanceId {
        &self.instance_id
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct EndpointRecord {
    address: SocketAddr,
    instance_id: Uuid,
}

impl From<&ServerEndpoint> for EndpointRecord {
    fn from(endpoint: &ServerEndpoint) -> Self {
        Self {
            address: endpoint.address,
            instance_id: endpoint.instance_id.0,
        }
    }
}

impl TryFrom<EndpointRecord> for ServerEndpoint {
    type Error = LocalAuthError;

    fn try_from(record: EndpointRecord) -> Result<Self, Self::Error> {
        Self::try_new(record.address, ServerInstanceId(record.instance_id))
    }
}

pub struct PublishedEndpoint {
    directory: Arc<PrivateDirectory>,
    instance_id: ServerInstanceId,
}

impl std::fmt::Debug for PublishedEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PublishedEndpoint")
            .field("instance_id", &self.instance_id)
            .finish_non_exhaustive()
    }
}

impl Drop for PublishedEndpoint {
    fn drop(&mut self) {
        let Ok(current) = load(&self.directory) else {
            return;
        };
        if current.instance_id == self.instance_id {
            let _ = self.directory.remove(ENDPOINT_FILE_NAME);
        }
    }
}

pub(crate) fn publish(
    directory: PrivateDirectory,
    endpoint: ServerEndpoint,
) -> Result<PublishedEndpoint, LocalAuthError> {
    let contents = serde_json::to_vec(&EndpointRecord::from(&endpoint)).map_err(|_| {
        LocalAuthError::MalformedEndpoint {
            path: directory.path(ENDPOINT_FILE_NAME),
        }
    })?;
    directory.write_and_replace(ENDPOINT_FILE_NAME, &contents)?;
    Ok(PublishedEndpoint {
        directory: Arc::new(directory),
        instance_id: endpoint.instance_id,
    })
}

pub(crate) fn load(directory: &PrivateDirectory) -> Result<ServerEndpoint, LocalAuthError> {
    let path = directory.path(ENDPOINT_FILE_NAME);
    let contents = match directory.read(ENDPOINT_FILE_NAME) {
        Ok(contents) => contents,
        Err(LocalAuthError::Inspect { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            return Err(LocalAuthError::EndpointNotPublished { path });
        }
        Err(error) => return Err(error),
    };
    let record = serde_json::from_slice::<EndpointRecord>(&contents)
        .map_err(|_| LocalAuthError::MalformedEndpoint { path: path.clone() })?;
    ServerEndpoint::try_from(record).map_err(|_| LocalAuthError::MalformedEndpoint { path })
}
