//! Ovra's own CA for phone access (ADR 0014): made once per install, kept in
//! the data directory, and limited by critical name constraints to private
//! IP addresses and a name that never resolves, so even a stolen CA key
//! cannot vouch for another site. It signs a short-lived leaf for the
//! address the listener uses.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    NameConstraints, SanType,
};
use time::{Duration, OffsetDateTime};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

const CA_LIFE: Duration = Duration::days(3650);
const LEAF_LIFE: Duration = Duration::days(30);
const SKEW: Duration = Duration::hours(1);
/// The only DNS name the CA may sign for; `.invalid` never resolves
/// (RFC 6761), so in practice it signs for IP addresses only.
const ONLY_NAME: &str = "ovra.invalid";

/// The address ranges a phone-access leaf may name: private IPv4, the
/// shared range Tailscale uses, IPv6 unique-local and link-local.
pub(crate) fn private_ranges() -> Vec<CidrSubnet> {
    vec![
        CidrSubnet::V4([10, 0, 0, 0], [255, 0, 0, 0]),
        CidrSubnet::V4([172, 16, 0, 0], [255, 240, 0, 0]),
        CidrSubnet::V4([192, 168, 0, 0], [255, 255, 0, 0]),
        CidrSubnet::V4([100, 64, 0, 0], [255, 192, 0, 0]),
        CidrSubnet::V6(
            [0xfc, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0xfe, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ),
        CidrSubnet::V6(
            [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0xff, 0xc0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ),
    ]
}

pub(crate) struct Ca {
    issuer: Issuer<'static, KeyPair>,
    der: CertificateDer<'static>,
}

impl std::fmt::Debug for Ca {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ca").finish_non_exhaustive()
    }
}

fn params() -> CertificateParams {
    let now = OffsetDateTime::now_utc();
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "Ovra phone access");
    name.push(DnType::OrganizationName, "Ovra on this computer");
    params.distinguished_name = name;
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params.not_before = now - SKEW;
    params.not_after = now + CA_LIFE;
    let mut permitted: Vec<GeneralSubtree> = private_ranges()
        .into_iter()
        .map(GeneralSubtree::IpAddress)
        .collect();
    permitted.push(GeneralSubtree::DnsName(ONLY_NAME.into()));
    // Tests listen on loopback; a real CA never names it.
    #[cfg(test)]
    permitted.push(GeneralSubtree::IpAddress(CidrSubnet::V4(
        [127, 0, 0, 0],
        [255, 0, 0, 0],
    )));
    params.name_constraints = Some(NameConstraints {
        permitted_subtrees: permitted,
        excluded_subtrees: Vec::new(),
    });
    params
}

impl Ca {
    /// The CA in `dir` (`ca.pem`, `ca.key`), made on first use. The key
    /// file is readable by this user only.
    pub(crate) fn load_or_create(dir: &Path) -> Result<Self, String> {
        let (cert_path, key_path) = (dir.join("ca.pem"), dir.join("ca.key"));
        if let (Ok(pem), Ok(key)) = (
            std::fs::read_to_string(&cert_path),
            std::fs::read_to_string(&key_path),
        ) {
            let key = KeyPair::from_pem(&key).map_err(|e| e.to_string())?;
            // The subject and key identifier come from `params` and the
            // key, so this signs as the kept certificate.
            let issuer = Issuer::new(params(), key);
            let der = CertificateDer::from_pem_slice(pem.as_bytes()).map_err(|e| e.to_string())?;
            return Ok(Self { issuer, der });
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let params = params();
        let key = KeyPair::generate().map_err(|e| e.to_string())?;
        let cert = params.self_signed(&key).map_err(|e| e.to_string())?;
        write_private(&key_path, &key.serialize_pem())?;
        std::fs::write(&cert_path, cert.pem()).map_err(|e| e.to_string())?;
        Ok(Self {
            der: cert.der().clone(),
            issuer: Issuer::new(params, key),
        })
    }

    pub(crate) fn der(&self) -> &[u8] {
        &self.der
    }

    /// SHA-256 of the certificate as uppercase hex in groups of four, the
    /// way phones show it.
    pub(crate) fn fingerprint(&self) -> String {
        let digest = ring::digest::digest(&ring::digest::SHA256, &self.der);
        let hex: String = digest.as_ref().iter().map(|b| format!("{b:02X}")).collect();
        hex.as_bytes()
            .chunks(4)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A TLS config that answers with a new leaf for `ip`, HTTP/1.1 only.
    pub(crate) fn server_for(&self, ip: IpAddr) -> Result<Arc<ServerConfig>, String> {
        let now = OffsetDateTime::now_utc();
        let mut params = CertificateParams::default();
        params.subject_alt_names = vec![SanType::IpAddress(ip)];
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
        let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
            .with_single_cert(vec![leaf.der().clone(), self.der.clone()], key)
            .map_err(|e| e.to_string())?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}

/// Writes `text` to a new file only this user can read.
fn write_private(path: &PathBuf, text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ca_is_kept_and_signs_a_leaf_for_a_private_address() {
        let dir = tempfile::tempdir().expect("dir");
        let first = Ca::load_or_create(dir.path()).expect("ca");
        let again = Ca::load_or_create(dir.path()).expect("ca");
        assert_eq!(first.fingerprint(), again.fingerprint());
        assert_eq!(first.fingerprint().split(' ').count(), 16);
        assert!(
            first
                .server_for("192.168.1.20".parse().expect("ip"))
                .is_ok()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("ca.key"))
                .expect("key")
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "the key is readable by others");
        }
    }
}
