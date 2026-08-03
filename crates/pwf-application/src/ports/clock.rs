use pwf_models::task::Timestamp;

pub trait Clock: Clone + Send + Sync + 'static {
    fn today(&self) -> Timestamp;
}
