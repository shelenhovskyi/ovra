//! The VAPID key that names this install to push services (RFC 8292).

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};

/// This install's VAPID key: made once, kept as `vapid.key` next to the
/// CA key and, like it, readable by this user only.
pub(crate) struct Vapid {
    key: EcdsaKeyPair,
}

impl std::fmt::Debug for Vapid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vapid").finish_non_exhaustive()
    }
}

impl Vapid {
    pub(crate) fn load_or_create(dir: &Path) -> Result<Self, String> {
        let path = dir.join("vapid.key");
        let pem = match std::fs::read_to_string(&path) {
            Ok(pem) => pem,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                let pem = rcgen::KeyPair::generate()
                    .map_err(|e| e.to_string())?
                    .serialize_pem();
                crate::ca::write_private(&path, &pem)?;
                pem
            }
            Err(e) => return Err(e.to_string()),
        };
        let der = rcgen::KeyPair::from_pem(&pem)
            .map_err(|e| e.to_string())?
            .serialize_der();
        let key =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &der, &SystemRandom::new())
                .map_err(|e| e.to_string())?;
        Ok(Self { key })
    }

    /// The public key, an uncompressed P-256 point, base64url: the
    /// browser's `applicationServerKey`.
    pub(crate) fn public(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.key.public_key().as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vapid_key_is_kept_private() {
        let dir = tempfile::tempdir().expect("dir");
        let vapid = Vapid::load_or_create(dir.path()).expect("vapid");
        let again = Vapid::load_or_create(dir.path()).expect("vapid");
        assert_eq!(vapid.public(), again.public());
        assert_eq!(
            URL_SAFE_NO_PAD
                .decode(vapid.public())
                .expect("base64url")
                .len(),
            65
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("vapid.key"))
                .expect("key")
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "the key is readable by others");
        }
    }
}
