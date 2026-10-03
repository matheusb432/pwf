#[cfg(target_os = "linux")]
#[path = "resource_benchmark/runner.rs"]
mod runner;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    runner::run()
}

#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("The server and CLI resource benchmark requires Linux /proc and GNU time.")
}
