use pwf_application::ports::clock::Clock;
use pwf_models::task::{TaskTimestamp, TaskTimestampError};

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalClock;

impl Clock for LocalClock {
    fn now(&self) -> Result<TaskTimestamp, TaskTimestampError> {
        TaskTimestamp::from_timestamp(jiff::Timestamp::now())
    }
}
