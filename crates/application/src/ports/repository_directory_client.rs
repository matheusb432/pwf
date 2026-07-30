pub trait RepositoryDirectoryClient: Clone + Send + Sync + 'static {
    fn is_directory(&self, path: &str) -> bool;
}
