#[cfg(target_os = "linux")]
#[path = "tui_benchmark/runner.rs"]
mod runner;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    runner::run()
}

#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("The TUI resource benchmark requires Linux /proc and a PTY.")
}
