use pwf_application::ports::clock::Clock;
use pwf_models::AppDate;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalClock;

impl Clock for LocalClock {
    #[allow(
        clippy::expect_used,
        reason = "a current Jiff date always satisfies AppDate's canonical range"
    )]
    fn today(&self) -> AppDate {
        let today = jiff::Zoned::now().date();
        AppDate::from_calendar_date(today.year(), today.month(), today.day())
            .expect("the system's local date must fit AppDate")
    }
}
