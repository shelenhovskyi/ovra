//! The Win32 side of the sandbox: SIDs, ACEs, the write-restricted token
//! and the processes that carry it. The only module of the crate that
//! may use `unsafe`: every block says why it is sound.

#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt as _;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows_sys::Win32::Security::PSID;

pub(crate) mod acl;
pub(crate) mod job;
pub(crate) mod token;

/// A SID Windows allocated, freed on drop.
#[derive(Debug)]
pub(crate) struct Sid(PSID);

impl Sid {
    /// Parses `S-1-...`.
    pub(crate) fn parse(text: &str) -> std::io::Result<Self> {
        let mut sid: PSID = std::ptr::null_mut();
        // SAFETY: the string is NUL-terminated; the SID is freed on drop.
        let ok = unsafe { ConvertStringSidToSidW(wide(OsStr::new(text)).as_ptr(), &mut sid) };
        if ok == 0 || sid.is_null() {
            return Err(std::io::Error::other(format!("not a SID: {text}")));
        }
        Ok(Self(sid))
    }

    pub(crate) fn as_ptr(&self) -> PSID {
        self.0
    }
}

impl Drop for Sid {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSidToSidW, freed once.
        unsafe { LocalFree(self.0) };
    }
}

// SAFETY: the SID is plain memory nobody else frees or changes.
unsafe impl Send for Sid {}

/// `text` as a NUL-terminated UTF-16 string.
pub(crate) fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain([0]).collect()
}
