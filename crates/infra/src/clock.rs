use pwf_application::Clock;
use pwf_domain::pending_work::Timestamp;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalClock;

impl Clock for LocalClock {
    fn today(&self) -> Timestamp {
        Timestamp::new(chrono::Local::now().format("%Y-%m-%d").to_string())
    }
}
