use jiff::{Timestamp, tz::TimeZone};
use pwf_application::ports::clock::Clock;
use pwf_models::task::{TaskTimestamp, TaskTimestampError};

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalClock;

impl Clock for LocalClock {
    fn now(&self) -> Result<TaskTimestamp, TaskTimestampError> {
        let timestamp = Timestamp::now();
        let offset = TimeZone::system().to_offset(timestamp);
        TaskTimestamp::from_timestamp_at_offset(timestamp, offset)
    }
}
