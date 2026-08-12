use pwf_models::session::SessionWorkingDirectory;

pub trait ProjectDirectoryClient: Clone + Send + Sync + 'static {
    fn is_directory(&self, path: &SessionWorkingDirectory) -> bool;
}
