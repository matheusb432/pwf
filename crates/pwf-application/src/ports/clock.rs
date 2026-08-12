use pwf_models::AppDate;

pub trait Clock: Clone + Send + Sync + 'static {
    fn today(&self) -> AppDate;
}
