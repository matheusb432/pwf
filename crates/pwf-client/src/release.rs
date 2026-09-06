use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use tonic::{Status, body::Body, codegen::http, transport::Channel};
use tower::Service;

#[derive(Clone)]
pub(crate) struct ReleaseChannel(pub(crate) Channel);

impl Service<http::Request<Body>> for ReleaseChannel {
    type Response = http::Response<Body>;
    type Error = <Channel as Service<http::Request<Body>>>::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(context)
    }

    fn call(&mut self, request: http::Request<Body>) -> Self::Future {
        let response = self.0.call(request);
        Box::pin(async move {
            let response = response.await?;
            let version = response.headers().get("pwf-server-version");
            if version == Some(&http::HeaderValue::from_static(env!("CARGO_PKG_VERSION"))) {
                return Ok(response);
            }
            // Tonic can reject transport requests before the server's response layer runs.
            if version.is_none()
                && Status::from_header_map(response.headers())
                    .is_some_and(|status| status.code() != tonic::Code::Ok)
            {
                return Ok(response);
            }
            let mut rejection = Status::failed_precondition(format!(
                "pwf client version {} requires a matching server. Run pwf server status, then install matching binaries and restart the server.",
                env!("CARGO_PKG_VERSION")
            )).into_http();
            if let Some(version) = version {
                rejection
                    .headers_mut()
                    .insert("pwf-server-version", version.clone());
            }
            Ok(rejection)
        })
    }
}
