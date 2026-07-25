use std::error::Error;

use pwf_domain::pending_work::{ProjectName, Timestamp, WorkItemId};

/// Request sent to an out-of-process pending-work allocator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocatePendingWork {
    /// Date supplied to pending-work creation.
    pub created: Timestamp,
    /// Managed project receiving the allocated item.
    pub project: ProjectName,
    /// Canonical tag applied by the allocator command.
    pub tag: String,
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
    use std::convert::Infallible;

    use pwf_domain::pending_work::{ProjectName, Timestamp, WorkItemId};

    use super::{AllocatePendingWork, PendingWorkAllocatorClient};

    #[derive(Clone)]
    struct Client;

    impl PendingWorkAllocatorClient for Client {
        type Error = Infallible;

        fn allocate(&self, request: &AllocatePendingWork) -> Result<WorkItemId, Self::Error> {
            assert_eq!(request.created.as_str(), "2026-01-01");
            assert_eq!(request.project.as_ref(), "test-project");
            assert_eq!(request.tag, "handoff");
            Ok(WorkItemId::try_new("TST-0001").unwrap())
        }
    }

    #[test]
    fn allocator_port_returns_a_typed_identifier() {
        let identifier = Client
            .allocate(&AllocatePendingWork {
                created: Timestamp::new("2026-01-01"),
                project: ProjectName::try_new("test-project").unwrap(),
                tag: "handoff".to_string(),
            })
            .unwrap();

        assert_eq!(identifier.as_ref(), "TST-0001");
    }
}
