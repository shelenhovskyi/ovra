//! ACEs on files and folders: the grants and denies of a plan. Each is one
//! explicit, inheritable ACE added to what is there; Windows writes the
//! inherited copy into everything below a folder.

use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    DENY_ACCESS, EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, SE_FILE_OBJECT,
    SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SUB_CONTAINERS_AND_OBJECTS_INHERIT,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_APPEND_DATA, FILE_DELETE_CHILD,
    FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_WRITE_ATTRIBUTES,
    FILE_WRITE_DATA, FILE_WRITE_EA, WRITE_DAC, WRITE_OWNER,
};

use super::{Sid, wide};
use crate::plan::Right;

/// What a deny takes away: every way to change the entry, its ACL or its
/// owner, or to put something new inside it.
const DENIED: u32 = FILE_WRITE_DATA
    | FILE_ADD_FILE
    | FILE_APPEND_DATA
    | FILE_ADD_SUBDIRECTORY
    | FILE_WRITE_EA
    | FILE_WRITE_ATTRIBUTES
    | DELETE
    | FILE_DELETE_CHILD
    | WRITE_DAC
    | WRITE_OWNER;

/// The access mask of `right`. Modify has no delete-child: a protected
/// entry must not be removable through its parent.
fn mask(right: Right) -> u32 {
    match right {
        Right::Modify => FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE,
        Right::Read => FILE_GENERIC_READ,
        Right::ReadExecute => FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
    }
}

/// Gives `sid` `right` on `path` (and what is in it).
pub(crate) fn grant(path: &Path, sid: &Sid, right: Right) -> std::io::Result<()> {
    add(path, sid, mask(right), GRANT_ACCESS)
}

/// Denies `sid` every change to `path` (and what is in it).
pub(crate) fn deny(path: &Path, sid: &Sid) -> std::io::Result<()> {
    add(path, sid, DENIED, DENY_ACCESS)
}

fn add(path: &Path, sid: &Sid, mask: u32, mode: i32) -> std::io::Result<()> {
    let name = wide(path.as_os_str());
    let fail = |code: u32| {
        Err(std::io::Error::other(format!(
            "changing the ACL of {}: {}",
            path.display(),
            std::io::Error::from_raw_os_error(code as i32)
        )))
    };
    // SAFETY: every pointer is valid for the call; the descriptor and the
    // new ACL are freed with LocalFree as the API asks.
    unsafe {
        let mut old: *mut ACL = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let code = GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut old,
            std::ptr::null_mut(),
            &mut descriptor,
        );
        if code != ERROR_SUCCESS {
            return fail(code);
        }
        let entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: mask,
            grfAccessMode: mode,
            grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: std::ptr::null_mut(),
                MultipleTrusteeOperation: 0,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_UNKNOWN,
                ptstrName: sid.as_ptr().cast(),
            },
        };
        let mut new: *mut ACL = std::ptr::null_mut();
        let code = SetEntriesInAclW(1, &entry, old, &mut new);
        LocalFree(descriptor);
        if code != ERROR_SUCCESS {
            return fail(code);
        }
        let code = SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            new,
            std::ptr::null(),
        );
        LocalFree(new.cast());
        if code != ERROR_SUCCESS {
            return fail(code);
        }
    }
    Ok(())
}
