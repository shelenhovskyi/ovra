//! TLS for hosts that have a secret bound: the roots Ovra trusts upstream,
//! and the bundle the sandbox trusts (those roots plus the session's CA).

use std::sync::Arc;

use tokio_rustls::rustls::crypto::CryptoProvider;
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use crate::url::base64;

/// The person's own CA bundles; each is trusted upstream and kept in the
/// sandbox's bundle, so corporate roots keep working. `SSL_CERT_FILE` and
/// `SSL_CERT_DIR` are read with the system's roots.
const BUNDLE_VARS: [&str; 4] = [
    "NODE_EXTRA_CA_CERTS",
    "CURL_CA_BUNDLE",
    "REQUESTS_CA_BUNDLE",
    "GIT_SSL_CAINFO",
];

pub(crate) fn provider() -> Arc<CryptoProvider> {
    Arc::new(tokio_rustls::rustls::crypto::ring::default_provider())
}

/// The roots trusted upstream.
#[derive(Debug, Clone)]
pub(crate) struct Roots {
    certs: Vec<CertificateDer<'static>>,
}

impl Roots {
    /// The system's roots (or `SSL_CERT_FILE` / `SSL_CERT_DIR`) and every
    /// bundle the person's environment names. Unreadable files are skipped.
    pub(crate) fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Self {
        let mut certs = rustls_native_certs::load_native_certs().certs;
        for name in BUNDLE_VARS {
            let Some(path) = var(name).filter(|p| !p.trim().is_empty()) else {
                continue;
            };
            if let Ok(iter) = CertificateDer::pem_file_iter(path) {
                certs.extend(iter.flatten());
            }
        }
        Self { certs }
    }

    #[cfg(test)]
    pub(crate) fn of(certs: Vec<CertificateDer<'static>>) -> Self {
        Self { certs }
    }

    /// Every root as PEM, then `extra` (the session's CA).
    pub(crate) fn bundle(&self, extra: &str) -> String {
        let mut out = String::new();
        for cert in &self.certs {
            out.push_str("-----BEGIN CERTIFICATE-----\n");
            for line in base64(cert).as_bytes().chunks(64) {
                out.push_str(&String::from_utf8_lossy(line));
                out.push('\n');
            }
            out.push_str("-----END CERTIFICATE-----\n");
        }
        out.push_str(extra);
        out
    }

    /// A client config that verifies servers against these roots and
    /// offers only HTTP/1.1.
    pub(crate) fn client(&self) -> Result<Arc<ClientConfig>, String> {
        let mut store = RootCertStore::empty();
        let (_added, _ignored) = store.add_parsable_certificates(self.certs.iter().cloned());
        let mut config = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(store)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}
