//! A CA for one session (`docs/specs/secrets.md`): made in memory when the
//! proxy starts, never written to disk except its certificate, and limited
//! by critical name constraints to the hosts that have a secret bound. It
//! signs a leaf for a bound host when the agent connects to it.

use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    NameConstraints,
};
use time::{Duration, OffsetDateTime};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// Leaves are valid this long; the CA as long as a session could run.
const LEAF_LIFE: Duration = Duration::days(7);
const CA_LIFE: Duration = Duration::days(30);
/// Clocks inside and outside the sandbox may differ a little.
const SKEW: Duration = Duration::hours(1);

pub(crate) struct SessionCa {
    issuer: Issuer<'static, KeyPair>,
    der: CertificateDer<'static>,
    pem: String,
}

impl std::fmt::Debug for SessionCa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionCa").finish_non_exhaustive()
    }
}

impl SessionCa {
    /// A new CA that can sign only for `hosts` (names without ports): no
    /// other name and no IP address.
    pub(crate) fn new(hosts: &[String]) -> Result<Self, rcgen::Error> {
        let now = OffsetDateTime::now_utc();
        let mut params = CertificateParams::default();
        let mut name = DistinguishedName::new();
        name.push(DnType::CommonName, "Ovra session CA");
        params.distinguished_name = name;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params.not_before = now - SKEW;
        params.not_after = now + CA_LIFE;
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: hosts
                .iter()
                .map(|h| GeneralSubtree::DnsName(h.clone()))
                .collect(),
            excluded_subtrees: vec![
                GeneralSubtree::IpAddress(CidrSubnet::V4([0; 4], [0; 4])),
                GeneralSubtree::IpAddress(CidrSubnet::V6([0; 16], [0; 16])),
            ],
        });
        let key = KeyPair::generate()?;
        let cert = params.self_signed(&key)?;
        Ok(Self {
            der: cert.der().clone(),
            pem: cert.pem(),
            issuer: Issuer::new(params, key),
        })
    }

    /// The CA certificate as PEM, for the sandbox's bundle.
    pub(crate) fn pem(&self) -> &str {
        &self.pem
    }

    /// A TLS server config that answers as `host` with a new leaf, offering
    /// only HTTP/1.1.
    pub(crate) fn server_for(&self, host: &str) -> Result<Arc<ServerConfig>, String> {
        let now = OffsetDateTime::now_utc();
        let mut params =
            CertificateParams::new(vec![host.to_owned()]).map_err(|e| e.to_string())?;
        params.distinguished_name = DistinguishedName::new();
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.not_before = now - SKEW;
        params.not_after = now + LEAF_LIFE;
        let key = KeyPair::generate().map_err(|e| e.to_string())?;
        let leaf = params
            .signed_by(&key, &self.issuer)
            .map_err(|e| e.to_string())?;
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let mut config = ServerConfig::builder_with_provider(crate::tls::provider())
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
            .with_single_cert(vec![leaf.der().clone(), self.der.clone()], key)
            .map_err(|e| e.to_string())?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}
