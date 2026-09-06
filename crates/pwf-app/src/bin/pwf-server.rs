use clap::Parser;

/// Run the local pwf background server in the foreground.
#[derive(Parser)]
#[command(name = "pwf-server", version)]
struct Arguments {}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Arguments::parse();
    pwf_server::run().await
}
