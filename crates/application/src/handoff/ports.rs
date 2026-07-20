use std::{error::Error, path::PathBuf};

use pwf_domain::pending_work::{ProjectName, Timestamp, WorkItemId};

/// Request sent to an out-of-process pending-work allocator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocatePendingWork {
    /// Manifest passed through the allocator CLI protocol.
    pub config_path: PathBuf,
    /// Date supplied to pending-work creation.
    pub created: Timestamp,
    /// Managed project receiving the allocated item.
    pub project: ProjectName,
}

/// Allocates one pending-work identifier through an external boundary.
pub trait PendingWorkAllocatorClient: Clone + Send + Sync + 'static {
    /// Concrete process or transport failure.
    type Error: Error + Send + Sync + 'static;

    /// Allocates one canonical identifier.
    ///
    /// # Errors
    ///
    /// Returns the client error when allocation or response validation fails.
    fn allocate(&self, request: &AllocatePendingWork) -> Result<WorkItemId, Self::Error>;
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, path::PathBuf};

    use pwf_domain::pending_work::{ProjectName, Timestamp, WorkItemId};

    use super::{AllocatePendingWork, PendingWorkAllocatorClient};

    #[derive(Clone)]
    struct Client;

    impl PendingWorkAllocatorClient for Client {
        type Error = Infallible;

        fn allocate(&self, request: &AllocatePendingWork) -> Result<WorkItemId, Self::Error> {
            assert_eq!(request.config_path, PathBuf::from("/tmp/config.json"));
            assert_eq!(request.created.as_str(), "2026-01-01");
            assert_eq!(request.project.as_ref(), "test-project");
            Ok(WorkItemId::try_new("TST-0001").unwrap())
        }
    }

    #[test]
    fn allocator_port_returns_a_typed_identifier() {
        let identifier = Client
            .allocate(&AllocatePendingWork {
                config_path: PathBuf::from("/tmp/config.json"),
                created: Timestamp::new("2026-01-01"),
                project: ProjectName::try_new("test-project").unwrap(),
            })
            .unwrap();

        assert_eq!(identifier.as_ref(), "TST-0001");
    }
}
