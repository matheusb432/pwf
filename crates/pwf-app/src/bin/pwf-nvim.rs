#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    pwf_nvim::run().await
}
