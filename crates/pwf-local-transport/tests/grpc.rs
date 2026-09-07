use std::time::Duration;

use pwf_local_transport::{LocalEndpoint, LocalListener};
use tonic_health::pb::{HealthCheckRequest, health_client::HealthClient};

#[tokio::test]
async fn tonic_serves_multiple_clients_over_the_native_transport()
-> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_secs(5), multiple_clients()).await?
}

async fn multiple_clients() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let endpoint = LocalEndpoint::from_root(root.path().join("runtime"))?;
    let listener = LocalListener::bind(&endpoint, Duration::from_millis(100)).await?;
    let (reporter, health) = tonic_health::server::health_reporter();
    reporter
        .set_service_status("", tonic_health::ServingStatus::Serving)
        .await;
    let (shutdown, stop) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (incoming, _ownership) = listener.into_parts();
        tonic::transport::Server::builder()
            .add_service(health)
            .serve_with_incoming_shutdown(incoming, async {
                let _ = stop.await;
            })
            .await
    });
    let mut clients = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let endpoint = endpoint.clone();
        clients.spawn(async move {
            let channel = endpoint
                .connect(Duration::from_secs(1), Duration::from_secs(1))
                .await?;
            let status = HealthClient::new(channel)
                .check(HealthCheckRequest {
                    service: String::new(),
                })
                .await?
                .into_inner()
                .status;
            assert_eq!(status, tonic_health::ServingStatus::Serving as i32);
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        });
    }
    while let Some(result) = clients.join_next().await {
        result?.map_err(|error| -> Box<dyn std::error::Error> { error })?;
    }
    let _ = shutdown.send(());
    server.await??;
    Ok::<_, Box<dyn std::error::Error>>(())
}
