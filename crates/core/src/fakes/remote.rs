use std::sync::Mutex;

use crate::Core;
use crate::protocol::remote::{Device, Pairing, RemoteStatus};
use crate::protocol::{ErrorCode, ProtocolError};
use crate::services::{RemoteAccess, Result};

use super::lock;

/// Phone access that listens nowhere: it records what the person turned
/// on, hands out a fixed pairing and keeps devices in memory.
#[derive(Debug, Default)]
pub struct FakeRemote {
    enabled: Mutex<Option<String>>,
    pub devices: Mutex<Vec<Device>>,
    pub answers: Mutex<Vec<(String, bool)>>,
}

const ADDRESS: &str = "192.168.1.20";

impl FakeRemote {
    fn now(&self) -> RemoteStatus {
        let enabled = lock(&self.enabled).clone();
        RemoteStatus {
            enabled: enabled.is_some(),
            url: enabled.map(|a| format!("https://{a}:52044")),
            addresses: vec![ADDRESS.into()],
            connected: 0,
            problem: None,
        }
    }
}

impl RemoteAccess for FakeRemote {
    fn status(&self) -> Result<RemoteStatus> {
        Ok(self.now())
    }
    fn enable(&self, _: Core, address: Option<String>) -> Result<RemoteStatus> {
        let address = address.unwrap_or_else(|| ADDRESS.into());
        if address != ADDRESS {
            return Err(ProtocolError::new(
                ErrorCode::BadRequest,
                format!("{address} is not a private address of this computer"),
            ));
        }
        *lock(&self.enabled) = Some(address);
        Ok(self.now())
    }
    fn disable(&self) -> Result<RemoteStatus> {
        *lock(&self.enabled) = None;
        Ok(self.now())
    }
    fn start_pairing(&self) -> Result<Pairing> {
        let Some(url) = self.now().url else {
            return Err(ProtocolError::new(
                ErrorCode::Conflict,
                "phone access is off",
            ));
        };
        Ok(Pairing {
            url: format!("{url}/pair#fake-secret"),
            qr_svg: "<svg xmlns=\"http://www.w3.org/2000/svg\"/>".into(),
            certificate_url: url.replace("https://", "http://") + "/ca.crt",
            certificate_fingerprint: "AB12 CD34".into(),
            expires_in: crate::protocol::remote::PAIRING_SECONDS,
        })
    }
    fn answer_pairing(&self, id: &str, allow: bool) -> Result<()> {
        lock(&self.answers).push((id.to_owned(), allow));
        Ok(())
    }
    fn devices(&self) -> Result<Vec<Device>> {
        Ok(lock(&self.devices).clone())
    }
    fn forget(&self, device: &str) -> Result<()> {
        let mut devices = lock(&self.devices);
        let before = devices.len();
        devices.retain(|d| d.id != device);
        if devices.len() == before {
            return Err(ProtocolError::new(
                ErrorCode::NotFound,
                format!("device {device} not found"),
            ));
        }
        Ok(())
    }
}
