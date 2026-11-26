//! The real [`PushSend`]: one HTTP/1.1 POST over TLS, checked against the
//! system's trusted roots. Push services are reached directly, never
//! through a proxy from the environment.

use std::sync::{Arc, OnceLock};

use axum::body::Bytes;
use axum::http::{Request, Uri, header};
use futures_util::future::BoxFuture;
use http_body_util::Full;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use super::{Push, PushSend};

#[derive(Debug, Default)]
pub(crate) struct HttpPush {
    /// Loaded on the first push, not when phone access starts.
    tls: OnceLock<Result<Arc<ClientConfig>, String>>,
}

fn config() -> Result<Arc<ClientConfig>, String> {
    let mut roots = RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    if roots.is_empty() {
        return Err("this system has no trusted root certificates".into());
    }
    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

impl PushSend for HttpPush {
    fn send(&self, push: Push) -> BoxFuture<'static, Result<u16, String>> {
        let tls = self.tls.get_or_init(config).clone();
        Box::pin(async move { post(tls?, push).await })
    }
}

async fn post(tls: Arc<ClientConfig>, push: Push) -> Result<u16, String> {
    let uri: Uri = push.endpoint.parse().map_err(|_| "bad endpoint")?;
    let path = uri.path_and_query().map_or("/", |p| p.as_str()).to_owned();
    let tcp = TcpStream::connect((push.host.as_str(), 443))
        .await
        .map_err(|e| e.to_string())?;
    let name = ServerName::try_from(push.host.clone()).map_err(|e| e.to_string())?;
    let stream = TlsConnector::from(tls)
        .connect(name, tcp)
        .await
        .map_err(|e| e.to_string())?;
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|e| e.to_string())?;
    let mut request = Request::post(path).header(header::HOST, push.host.as_str());
    for (name, value) in &push.headers {
        request = request.header(*name, value.as_str());
    }
    let request = request
        .body(Full::new(Bytes::from(push.body)))
        .map_err(|e| e.to_string())?;
    // The connection is driven here, so dropping this future (the
    // caller's timeout) closes it.
    tokio::pin!(conn);
    let response = tokio::select! {
        response = sender.send_request(request) => response.map_err(|e| e.to_string())?,
        closed = &mut conn => {
            return Err(closed.err().map_or_else(|| "connection closed".into(), |e| e.to_string()));
        }
    };
    Ok(response.status().as_u16())
}
