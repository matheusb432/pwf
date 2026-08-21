#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pwf_server::run().await
}
