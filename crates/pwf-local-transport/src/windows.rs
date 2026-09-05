use std::{
    io,
    os::windows::ffi::OsStrExt as _,
    path::Path,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer},
};
use tonic::transport::{Channel, Endpoint, server::Connected};

mod security;

#[derive(Debug, Clone)]
pub struct LocalEndpoint {
    name: String,
    user_sid: String,
}

impl LocalEndpoint {
    pub fn from_environment() -> io::Result<Self> {
        if let Some(root) = std::env::var_os("PWF_RUNTIME_DIR") {
            return Self::from_root(Path::new(&root));
        }
        let user_sid = security::current_user_sid()?;
        Ok(Self {
            name: format!(r"\\.\pipe\pwf-{user_sid}"),
            user_sid,
        })
    }

    /// Uses an absolute runtime namespace to isolate server instances without creating files.
    pub fn from_root(root: impl AsRef<Path>) -> io::Result<Self> {
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "PWF_RUNTIME_DIR must be absolute",
            ));
        }
        let root_bytes = root
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let namespace = blake3::hash(&root_bytes).to_hex();
        let user_sid = security::current_user_sid()?;
        Ok(Self {
            name: format!(r"\\.\pipe\pwf-{user_sid}-{:.32}", namespace.as_str()),
            user_sid,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        Path::new(&self.name)
    }

    pub async fn connect(
        &self,
        timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Channel, crate::ConnectError> {
        let name = self.name.clone();
        let channel = Endpoint::from_static("http://pwf.local")
            .connect_timeout(timeout)
            .timeout(request_timeout)
            .connect_with_connector(tower::service_fn(move |_| {
                let name = name.clone();
                async move {
                    open_pipe(&name, timeout)
                        .await
                        .map(hyper_util::rt::TokioIo::new)
                }
            }))
            .await?;
        Ok(channel)
    }
}

async fn open_pipe(name: &str, timeout: Duration) -> io::Result<NamedPipeClient> {
    tokio::time::timeout(timeout, async {
        loop {
            match ClientOptions::new().open(name) {
                Ok(pipe) => return Ok(pipe),
                Err(error)
                    if error.raw_os_error()
                        == Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY.cast_signed()) =>
                {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => return Err(error),
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "local pwf-server pipe stayed busy"))?
}

#[derive(Debug)]
pub struct LocalListener {
    endpoint: LocalEndpoint,
    server: NamedPipeServer,
}

impl LocalListener {
    pub fn bind(
        endpoint: &LocalEndpoint,
        _probe_timeout: Duration,
    ) -> std::future::Ready<io::Result<Self>> {
        std::future::ready(
            security::create_pipe(&endpoint.name, &endpoint.user_sid, true).map(|server| Self {
                endpoint: endpoint.clone(),
                server,
            }),
        )
    }

    pub fn into_parts(
        self,
    ) -> (
        impl futures::Stream<Item = io::Result<PipeConnection>> + Send,
        (),
    ) {
        let incoming = futures::stream::try_unfold(self, |listener| async move {
            listener.server.connect().await?;
            // Keep a listening instance alive before handing this connection to Tonic.
            let next =
                security::create_pipe(&listener.endpoint.name, &listener.endpoint.user_sid, false)?;
            Ok(Some((
                PipeConnection(listener.server),
                Self {
                    endpoint: listener.endpoint,
                    server: next,
                },
            )))
        });
        (incoming, ())
    }
}

#[derive(Debug)]
pub struct PipeConnection(NamedPipeServer);

impl Connected for PipeConnection {
    type ConnectInfo = ();
    fn connect_info(&self) {}
}

impl AsyncRead for PipeConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(context, buffer)
    }
}

impl AsyncWrite for PipeConnection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(context, buffer)
    }
    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(context)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(context)
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt as _;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    use super::*;

    #[tokio::test]
    async fn pipe_ownership_and_multiple_clients() {
        let root = tempfile::tempdir().unwrap();
        let endpoint = LocalEndpoint::from_root(root.path()).unwrap();
        let listener = LocalListener::bind(&endpoint, Duration::from_secs(1))
            .await
            .unwrap();
        assert!(
            LocalListener::bind(&endpoint, Duration::from_secs(1))
                .await
                .is_err()
        );
        let (incoming, ()) = listener.into_parts();
        let mut incoming = Box::pin(incoming);
        for _ in 0..3 {
            let mut client = open_pipe(&endpoint.name, Duration::from_secs(1))
                .await
                .unwrap();
            let mut server = incoming.next().await.unwrap().unwrap();
            client.write_all(b"pwf").await.unwrap();
            let mut bytes = [0; 3];
            server.read_exact(&mut bytes).await.unwrap();
            assert_eq!(&bytes, b"pwf");
        }
        drop(incoming);
        // Mio releases cancelled overlapped I/O handles when the runtime processes completions.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        let mut rebound = LocalListener::bind(&endpoint, Duration::from_secs(1)).await;
        while rebound
            .as_ref()
            .is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied)
        {
            assert!(
                tokio::time::Instant::now() < deadline,
                "pipe handles did not close"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
            rebound = LocalListener::bind(&endpoint, Duration::from_secs(1)).await;
        }
        rebound.unwrap();
    }

    #[tokio::test]
    async fn busy_pipe_connection_is_bounded() {
        let root = tempfile::tempdir().unwrap();
        let endpoint = LocalEndpoint::from_root(root.path()).unwrap();
        let _listener = LocalListener::bind(&endpoint, Duration::from_secs(1))
            .await
            .unwrap();
        let _first = open_pipe(&endpoint.name, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(
            open_pipe(&endpoint.name, Duration::from_millis(30))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
    }
}
