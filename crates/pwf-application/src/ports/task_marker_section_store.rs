use std::{future::Future, sync::Arc};

use crate::task::{TaskMarkerSections, TaskMarkerSectionsError};

pub trait TaskMarkerSectionStore: Send + Sync + 'static {
    fn get_task_marker_sections(
        &self,
    ) -> impl Future<Output = Result<Arc<TaskMarkerSections>, TaskMarkerSectionsError>> + Send;
}
