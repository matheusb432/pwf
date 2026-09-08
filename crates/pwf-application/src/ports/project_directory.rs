pub trait ProjectDirectoryClient: Clone + Send + Sync + 'static {
    fn canonicalize(&self, path: &std::path::Path) -> std::io::Result<std::path::PathBuf>;

    fn is_directory(&self, path: &std::path::Path) -> bool;
}
