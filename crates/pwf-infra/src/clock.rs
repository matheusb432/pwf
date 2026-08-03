use pwf_application::ports::clock::Clock;
use pwf_models::task::Timestamp;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalClock;

impl Clock for LocalClock {
    fn today(&self) -> Timestamp {
        Timestamp::new(jiff::Zoned::now().strftime("%Y-%m-%d").to_string())
    }
}
