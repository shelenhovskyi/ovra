//! Web Push message encryption (RFC 8291: `aes128gcm`, one record) and the
//! VAPID key and token that name this install to push services (RFC 8292).

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::agreement::{ECDH_P256, EphemeralPrivateKey, UnparsedPublicKey};
use ring::error::Unspecified;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use ring::{aead, hkdf};

/// The one record's size: the content, its delimiter and the tag fit in it.
const RECORD_SIZE: u32 = 4096;
const TAG: usize = 16;
/// Every message's content, delimiter and padding together, so its size
/// tells a watcher nothing of its kind.
const PADDED: usize = 128;
/// Apple requires a contact; this names the project, not the person.
const SUBJECT: &str = "https://github.com/shelenhovskyi/ovra";

struct Len(usize);

impl hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], out: &mut [u8]) -> Result<(), Unspecified> {
    hkdf::Salt::new(hkdf::HKDF_SHA256, salt)
        .extract(ikm)
        .expand(&[info], Len(out.len()))?
        .fill(out)
}

/// The content key and nonce of RFC 8291, section 3.4.
fn keys(
    ecdh_secret: &[u8],
    ua_public: &[u8],
    as_public: &[u8],
    auth: &[u8],
    salt: &[u8],
) -> Result<([u8; 16], [u8; 12]), Unspecified> {
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(as_public);
    let mut ikm = [0u8; 32];
    hkdf(auth, ecdh_secret, &key_info, &mut ikm)?;
    let mut cek = [0u8; 16];
    hkdf(salt, &ikm, b"Content-Encoding: aes128gcm\0", &mut cek)?;
    let mut nonce = [0u8; 12];
    hkdf(salt, &ikm, b"Content-Encoding: nonce\0", &mut nonce)?;
    Ok((cek, nonce))
}

/// The body of a push message once the ECDH secret is known: the header
/// (salt, record size, `as_public`) and one encrypted record, padded with
/// zeros after its delimiter (RFC 8188) to `padded` bytes before the tag.
pub(crate) fn seal(
    ecdh_secret: &[u8],
    ua_public: &[u8],
    as_public: &[u8],
    auth: &[u8],
    salt: &[u8; 16],
    plaintext: &[u8],
    padded: usize,
) -> Result<Vec<u8>, Unspecified> {
    let padded = padded.max(plaintext.len() + 1);
    if padded + TAG > RECORD_SIZE as usize || as_public.len() > 255 {
        return Err(Unspecified);
    }
    let (cek, nonce) = keys(ecdh_secret, ua_public, as_public, auth, salt)?;
    let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_128_GCM, &cek)?);
    let mut record = plaintext.to_vec();
    // The last (and only) record's padding delimiter.
    record.push(2);
    record.resize(padded, 0);
    key.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key(nonce),
        aead::Aad::empty(),
        &mut record,
    )?;
    let mut body = salt.to_vec();
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(as_public.len() as u8);
    body.extend_from_slice(as_public);
    body.extend_from_slice(&record);
    Ok(body)
}

/// Encrypts `plaintext` for the browser key `ua_public` and secret `auth`
/// with a new key and salt.
pub(crate) fn encrypt(
    ua_public: &[u8],
    auth: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, Unspecified> {
    let rng = SystemRandom::new();
    let private = EphemeralPrivateKey::generate(&ECDH_P256, &rng)?;
    let as_public = private.compute_public_key()?;
    let mut salt = [0u8; 16];
    rng.fill(&mut salt)?;
    ring::agreement::agree_ephemeral(
        private,
        &UnparsedPublicKey::new(&ECDH_P256, ua_public),
        |secret| {
            seal(
                secret,
                ua_public,
                as_public.as_ref(),
                auth,
                &salt,
                plaintext,
                PADDED,
            )
        },
    )?
}

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

    /// The `Authorization` header for a push service at `origin`, valid
    /// until `exp` (seconds since the epoch).
    pub(crate) fn authorization(&self, origin: &str, exp: u64) -> Result<String, Unspecified> {
        let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = serde_json::json!({"aud": origin, "exp": exp, "sub": SUBJECT});
        let claims = URL_SAFE_NO_PAD.encode(claims.to_string());
        let signed = format!("{header}.{claims}");
        let signature = self.key.sign(&SystemRandom::new(), signed.as_bytes())?;
        let signature = URL_SAFE_NO_PAD.encode(signature.as_ref());
        Ok(format!("vapid t={signed}.{signature}, k={}", self.public()))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn b64(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        URL_SAFE_NO_PAD.decode(s).expect("base64url")
    }

    /// Opens a body made by [`seal`] with the ECDH secret and drops its
    /// padding.
    pub(crate) fn open(ecdh_secret: &[u8], ua_public: &[u8], auth: &[u8], body: &[u8]) -> Vec<u8> {
        let (salt, rest) = body.split_at(16);
        let id_len = rest[4] as usize;
        let (as_public, record) = rest[5..].split_at(id_len);
        let (cek, nonce) = keys(ecdh_secret, ua_public, as_public, auth, salt).expect("keys");
        let key =
            aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_128_GCM, &cek).expect("key"));
        let mut record = record.to_vec();
        let plain = key
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut record,
            )
            .expect("decrypts");
        let end = plain.iter().rposition(|b| *b != 0).expect("delimiter");
        assert_eq!(plain[end], 2, "one last record");
        plain[..end].to_vec()
    }

    #[test]
    fn the_rfc_8291_example_encrypts_to_its_body() {
        let body = seal(
            &b64("kyrL1jIIOHEzg3sM2ZWRHDRB62YACZhhSlknJ672kSs"),
            &b64("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcx
                  aOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4"),
            &b64("BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIg
                  Dll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"),
            &b64("BTBZMqHH6r4Tts7J_aSIgg"),
            &b64("DGv6ra1nlYgDCS1FRnbzlw").try_into().expect("salt"),
            b"When I grow up, I want to be a watermelon",
            0,
        )
        .expect("seal");
        let expected = b64(
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml
             mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT
             pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN",
        );
        assert_eq!(
            URL_SAFE_NO_PAD.encode(&body),
            URL_SAFE_NO_PAD.encode(&expected)
        );
    }

    #[test]
    fn the_vapid_key_is_kept_private_and_signs_its_token() {
        let dir = tempfile::tempdir().expect("dir");
        let vapid = Vapid::load_or_create(dir.path()).expect("vapid");
        let again = Vapid::load_or_create(dir.path()).expect("vapid");
        assert_eq!(vapid.public(), again.public());
        assert_eq!(b64(&vapid.public()).len(), 65);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("vapid.key"))
                .expect("key")
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "the key is readable by others");
        }

        let header = vapid
            .authorization("https://fcm.googleapis.com", 1_800_000_000)
            .expect("token");
        let (token, key) = header
            .strip_prefix("vapid t=")
            .and_then(|h| h.split_once(", k="))
            .expect("vapid header");
        assert_eq!(key, vapid.public());
        let (signed, signature) = token.rsplit_once('.').expect("jwt");
        ring::signature::UnparsedPublicKey::new(
            &ring::signature::ECDSA_P256_SHA256_FIXED,
            b64(key),
        )
        .verify(signed.as_bytes(), &b64(signature))
        .expect("signed by the key");
        let claims: serde_json::Value =
            serde_json::from_slice(&b64(signed.split('.').nth(1).expect("claims"))).expect("json");
        assert_eq!(
            claims,
            serde_json::json!({"aud": "https://fcm.googleapis.com", "exp": 1_800_000_000u64, "sub": SUBJECT})
        );
    }
}
