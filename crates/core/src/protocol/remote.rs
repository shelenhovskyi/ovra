//! `remote.*`: phone access over the LAN (ADR 0014), from the desktop.
//! A phone itself never uses these.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum RemoteRequest {
    #[serde(rename = "remote.status")]
    Status,
    /// Starts listening on `address`, one of `RemoteStatus::addresses`
    /// (the first when left out), and keeps it on across restarts.
    /// Answers `remote.status`.
    #[serde(rename = "remote.enable")]
    Enable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        address: Option<String>,
    },
    /// Stops listening and closes every phone's connection. Answers
    /// `remote.status`.
    #[serde(rename = "remote.disable")]
    Disable,
    /// Opens a pairing window of [`PAIRING_SECONDS`] for one phone.
    /// Refused (`conflict`) while phone access is off.
    #[serde(rename = "remote.start_pairing")]
    StartPairing,
    /// The person's answer to `remote.pairing_request`. Answers `ok`.
    #[serde(rename = "remote.answer_pairing")]
    AnswerPairing { id: String, allow: bool },
    #[serde(rename = "remote.devices")]
    Devices,
    /// Forgets a phone and closes its connections. Answers `ok`.
    #[serde(rename = "remote.forget")]
    Forget { device: String },
}

/// How long a pairing code works.
pub const PAIRING_SECONDS: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum RemoteResponse {
    #[serde(rename = "remote.status")]
    Status(RemoteStatus),
    #[serde(rename = "remote.pairing")]
    Pairing(Pairing),
    #[serde(rename = "remote.devices")]
    Devices(Vec<Device>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum RemoteEvent {
    /// Phone access was turned on or off, or a phone connected or left.
    #[serde(rename = "remote.changed")]
    Changed(RemoteStatus),
    /// A phone used the pairing code and waits for the person.
    #[serde(rename = "remote.pairing_request")]
    PairingRequest(PairingRequest),
    /// The pairing window closed: used, answered, or out of time.
    #[serde(rename = "remote.pairing_closed")]
    PairingClosed,
    #[serde(rename = "remote.devices_changed")]
    DevicesChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteStatus {
    /// Whether Ovra listens for phones.
    pub enabled: bool,
    /// The address phones open, e.g. `https://192.168.1.20:52044`, while
    /// listening.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// This computer's private addresses that phone access may use.
    pub addresses: Vec<String>,
    /// Phones connected now.
    pub connected: u32,
    /// Why phone access is not listening although it is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pairing {
    /// What the QR code holds: the app's address with the one-time secret.
    pub url: String,
    /// The QR code as an SVG image.
    pub qr_svg: String,
    /// Where the phone gets Ovra's certificate first, over plain HTTP.
    pub certificate_url: String,
    /// SHA-256 of the certificate, in groups, to compare on the phone.
    pub certificate_fingerprint: String,
    /// Seconds until the code stops working.
    pub expires_in: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingRequest {
    pub id: String,
    /// The name the phone gave, e.g. "iPhone".
    pub name: String,
    /// Four digits the phone shows too.
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// Milliseconds since the Unix epoch.
    pub paired_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen_at_ms: Option<u64>,
    pub connected: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Event, Request, Response};

    #[test]
    fn wire_format_is_stable() {
        let status = RemoteStatus {
            enabled: true,
            url: Some("https://192.168.1.20:52044".into()),
            addresses: vec!["192.168.1.20".into()],
            connected: 1,
            problem: None,
        };
        insta::assert_json_snapshot!(serde_json::json!({
            "requests": [
                Request::Remote(RemoteRequest::Status),
                Request::Remote(RemoteRequest::Enable { address: Some("192.168.1.20".into()) }),
                Request::Remote(RemoteRequest::Disable),
                Request::Remote(RemoteRequest::StartPairing),
                Request::Remote(RemoteRequest::AnswerPairing { id: "q1".into(), allow: true }),
                Request::Remote(RemoteRequest::Devices),
                Request::Remote(RemoteRequest::Forget { device: "d1".into() }),
            ],
            "responses": [
                Response::Remote(RemoteResponse::Status(status.clone())),
                Response::Remote(RemoteResponse::Pairing(Pairing {
                    url: "https://192.168.1.20:52044/pair#secret".into(),
                    qr_svg: "<svg/>".into(),
                    certificate_url: "http://192.168.1.20:52044/ca.crt".into(),
                    certificate_fingerprint: "AB12 CD34".into(),
                    expires_in: 300,
                })),
                Response::Remote(RemoteResponse::Devices(vec![Device {
                    id: "d1".into(),
                    name: "iPhone".into(),
                    paired_at_ms: 1,
                    seen_at_ms: Some(2),
                    connected: true,
                }])),
            ],
            "events": [
                Event::from(RemoteEvent::Changed(status)),
                Event::from(RemoteEvent::PairingRequest(PairingRequest {
                    id: "q1".into(),
                    name: "iPhone".into(),
                    code: "4821".into(),
                })),
                Event::from(RemoteEvent::PairingClosed),
                Event::from(RemoteEvent::DevicesChanged),
            ],
        }));
    }
}
