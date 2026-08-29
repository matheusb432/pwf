use pwf_models::{
    AppDate,
    task::{TaskTimestamp, TaskTimestampError},
};

pub trait Clock: Clone + Send + Sync + 'static {
    fn now(&self) -> Result<TaskTimestamp, TaskTimestampError>;

    fn today(&self) -> Result<AppDate, TaskTimestampError> {
        self.now().map(TaskTimestamp::date)
    }
}
