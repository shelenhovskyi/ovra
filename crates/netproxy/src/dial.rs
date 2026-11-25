//! Name resolution and outgoing connections, behind a trait so tests never
//! touch the network.

use std::fmt::Debug;
use std::io;
use std::net::SocketAddr;

use ovra_core::services::BoxFuture;
use tokio::io::{AsyncRead, AsyncWrite};

pub(crate) trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

pub(crate) type Conn = Box<dyn Stream>;

pub(crate) trait Dial: Send + Sync + Debug {
    fn resolve(&self, host: String, port: u16) -> BoxFuture<'_, io::Result<Vec<SocketAddr>>>;
    /// `host` as the user's proxy may resolve it: not fully qualified, so
    /// `/etc/hosts`, search domains and the other sources of the
    /// machine's resolver apply. Only ever checked, never connected to.
    fn resolve_as_given(
        &self,
        host: String,
        port: u16,
    ) -> BoxFuture<'_, io::Result<Vec<SocketAddr>>>;
    fn connect(&self, addr: SocketAddr) -> BoxFuture<'_, io::Result<Conn>>;
}

/// The machine's own resolver and TCP.
#[derive(Debug)]
pub(crate) struct Net;

impl Dial for Net {
    fn resolve(&self, host: String, port: u16) -> BoxFuture<'_, io::Result<Vec<SocketAddr>>> {
        // Fully qualified, so the resolver's search domains never apply.
        let name = if host.parse::<std::net::IpAddr>().is_ok() {
            host
        } else {
            format!("{host}.")
        };
        Box::pin(async move { Ok(tokio::net::lookup_host((name, port)).await?.collect()) })
    }

    fn resolve_as_given(
        &self,
        host: String,
        port: u16,
    ) -> BoxFuture<'_, io::Result<Vec<SocketAddr>>> {
        Box::pin(async move { Ok(tokio::net::lookup_host((host, port)).await?.collect()) })
    }

    fn connect(&self, addr: SocketAddr) -> BoxFuture<'_, io::Result<Conn>> {
        Box::pin(async move {
            let stream = tokio::net::TcpStream::connect(addr).await?;
            stream.set_nodelay(true)?;
            Ok(Box::new(stream) as Conn)
        })
    }
}
