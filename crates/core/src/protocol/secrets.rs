//! `secrets.*`: tokens kept in the OS keychain and added by the netproxy to
//! requests for one host (`docs/specs/secrets.md`). No method returns a
//! value; only the person sets them.

use serde::{Deserialize, Serialize};

use super::projects::ProjectId;

/// A secret without its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretInfo {
    /// 1 to 64 of `A-Z a-z 0-9 _ . -`, starting with a letter or digit.
    pub name: String,
    /// The one host it is sent to: a DNS name, optionally with `:port`.
    pub host: String,
    /// The request header that carries it, such as `Authorization`.
    pub header: String,
    /// Text before the value in the header, such as `Bearer `.
    #[serde(default)]
    pub prefix: String,
}

/// A secret's value. `Debug` never shows it, so a logged request does not
/// either.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SecretValue(String);

/// Errors never quote the input: the transport answers a bad frame with
/// the parser's message.
impl<'de> Deserialize<'de> for SecretValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = SecretValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a secret's value as a string")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<SecretValue, E> {
                Ok(SecretValue(v.to_owned()))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<SecretValue, E> {
                Ok(SecretValue(v))
            }
        }
        deserializer
            .deserialize_string(Visitor)
            .map_err(|_| serde::de::Error::custom("a secret's value must be a string"))
    }
}

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Wipes the memory it held when dropped.
impl Drop for SecretValue {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<secret>")
    }
}

/// Whether values can be stored on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SecretStoreStatus {
    /// `name` says where values go, such as `Secret Service`.
    Available { name: String },
    /// Setting a secret fails with `unavailable`; there is no fallback.
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretList {
    pub store: SecretStoreStatus,
    pub secrets: Vec<SecretInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum SecretsRequest {
    /// Answers `secrets.list`.
    #[serde(rename = "secrets.list")]
    List { project: ProjectId },
    /// Stores or replaces a secret. `bad_request` for a field that breaks
    /// the rules, `conflict` if another secret has the same host and
    /// header, `unavailable` without a keychain. Answers `ok`.
    #[serde(rename = "secrets.set")]
    Set {
        project: ProjectId,
        secret: SecretInfo,
        value: SecretValue,
    },
    /// Answers `ok`; `not_found` if there is no such secret.
    #[serde(rename = "secrets.remove")]
    Remove { project: ProjectId, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SecretsResponse {
    #[serde(rename = "secrets.list")]
    List(SecretList),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SecretsEvent {
    /// A project's secrets were set or removed.
    #[serde(rename = "secrets.changed")]
    Changed { project: ProjectId },
}
