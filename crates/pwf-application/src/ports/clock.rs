use pwf_models::pending_work::Timestamp;

pub trait Clock: Clone + Send + Sync + 'static {
    fn today(&self) -> Timestamp;
}
