//! How a paired phone proves itself: the server sends a random challenge,
//! the phone signs `ovra-companion-v1\n<origin>\n<challenge>` with its
//! non-extractable WebCrypto key (ECDSA P-256, SHA-256, raw r||s), and the
//! server checks it against the key kept at pairing. No bearer tokens.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};

/// 32 random bytes, base64url.
pub(crate) fn random_token() -> String {
    let mut bytes = [0u8; 32];
    // The system's generator does not fail on supported platforms; if it
    // ever does, a zero token is refused by `challenge_ok`.
    let _ = SystemRandom::new().fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn message(origin: &str, challenge: &str) -> String {
    format!("ovra-companion-v1\n{origin}\n{challenge}")
}

/// Whether `key` is an uncompressed P-256 point, base64url.
pub(crate) fn key_ok(key: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(key)
        .is_ok_and(|k| k.len() == 65 && k[0] == 4)
}

/// Whether `signature` (base64url r||s) signs `message` with `key`.
pub(crate) fn verify(key: &str, message: &str, signature: &str) -> bool {
    let (Ok(key), Ok(signature)) = (
        URL_SAFE_NO_PAD.decode(key),
        URL_SAFE_NO_PAD.decode(signature),
    ) else {
        return false;
    };
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, key)
        .verify(message.as_bytes(), &signature)
        .is_ok()
}

/// Compares in constant time.
pub(crate) fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Four digits from the pairing secret and the phone's key, shown on both
/// screens so the person can tell it is their phone asking.
pub(crate) fn code(secret: &str, key: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, format!("{secret}\n{key}").as_bytes());
    let d = digest.as_ref();
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) % 10_000;
    format!("{n:04}")
}

#[cfg(test)]
pub(crate) mod testkey {
    //! A phone's key for tests, made with ring as WebCrypto would.
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use ring::rand::SystemRandom;
    use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};

    pub(crate) struct TestKey(EcdsaKeyPair);

    impl TestKey {
        pub(crate) fn new() -> Self {
            let rng = SystemRandom::new();
            let pkcs8 =
                EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).expect("key");
            Self(
                EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
                    .expect("key"),
            )
        }
        pub(crate) fn public(&self) -> String {
            URL_SAFE_NO_PAD.encode(self.0.public_key().as_ref())
        }
        pub(crate) fn sign(&self, message: &str) -> String {
            let sig = self
                .0
                .sign(&SystemRandom::new(), message.as_bytes())
                .expect("sign");
            URL_SAFE_NO_PAD.encode(sig.as_ref())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testkey::TestKey;
    use super::*;

    #[test]
    fn only_the_paired_key_signs_the_challenge_for_this_origin() {
        let phone = TestKey::new();
        let other = TestKey::new();
        let challenge = random_token();
        let origin = "https://192.168.1.20:52044";
        let signed = phone.sign(&message(origin, &challenge));
        assert!(key_ok(&phone.public()));
        assert!(verify(
            &phone.public(),
            &message(origin, &challenge),
            &signed
        ));
        assert!(!verify(
            &other.public(),
            &message(origin, &challenge),
            &signed
        ));
        assert!(!verify(
            &phone.public(),
            &message("https://evil", &challenge),
            &signed
        ));
        assert!(!verify(
            &phone.public(),
            &message(origin, &random_token()),
            &signed
        ));
        assert!(!key_ok("AAAA"));
        assert_eq!(code("s", &phone.public()).len(), 4);
        assert!(same("abc", "abc") && !same("abc", "abd"));
    }
}
