use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use tonic::{Status, body::Body, codegen::http};
use tower::{Layer, Service};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy)]
pub(crate) struct ReleaseLayer;

impl<S> Layer<S> for ReleaseLayer {
    type Service = ReleaseService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ReleaseService(inner)
    }
}

#[derive(Clone)]
pub(crate) struct ReleaseService<S>(S);

impl<S> Service<http::Request<Body>> for ReleaseService<S>
where
    S: Service<http::Request<Body>, Response = http::Response<Body>>,
    S::Future: Send + 'static,
    S::Error: 'static,
{
    type Response = http::Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(context)
    }

    fn call(&mut self, request: http::Request<Body>) -> Self::Future {
        if request.uri().path().starts_with("/pwf.v1.")
            && request.headers().get("pwf-client-version")
                != Some(&http::HeaderValue::from_static(VERSION))
        {
            let mut response = Status::failed_precondition(format!(
                "pwf-server requires client version {VERSION}. Install matching binaries and restart the server."
            )).into_http();
            response.headers_mut().insert(
                "pwf-server-version",
                http::HeaderValue::from_static(VERSION),
            );
            return Box::pin(async { Ok(response) });
        }
        let response = self.0.call(request);
        Box::pin(async move {
            let mut response = response.await?;
            response.headers_mut().insert(
                "pwf-server-version",
                http::HeaderValue::from_static(VERSION),
            );
            Ok(response)
        })
    }
}
