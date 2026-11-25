//! The sandbox user, set up once with administrator rights
//! (`docs/specs/sandbox-windows.md`): the local account `OvraSandbox`, its
//! password kept with DPAPI for the person, and a firewall rule that blocks
//! all its outbound traffic. Removal undoes all three.
//!
//! The person's own process makes the password and keeps it; the elevated
//! process (which may run as another account) reads it from a file only
//! the person and administrators can read, deletes the file and creates
//! the account.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LocalFree};
use windows_sys::Win32::NetworkManagement::NetManagement::{
    LOCALGROUP_MEMBERS_INFO_3, NERR_Success, NERR_UserExists, NetLocalGroupAddMembers, NetUserAdd,
    NetUserDel, NetUserSetInfo, UF_DONT_EXPIRE_PASSWD, UF_PASSWD_CANT_CHANGE, UF_SCRIPT,
    USER_INFO_1, USER_INFO_1003, USER_PRIV_USER,
};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom, CRYPT_INTEGER_BLOB, CryptProtectData,
    CryptUnprotectData,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, RegCloseKey,
    RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};

use super::token::Handle;
use super::wide;

/// The sandbox user's name.
pub(crate) const USER: &str = "OvraSandbox";
/// The firewall rule's display name.
const RULE: &str = "Ovra sandbox: no network";
/// Where the person keeps the sandbox user's password, DPAPI-protected.
const PASSWORD_FILE: &str = "sandbox-user";
/// The elevated stage's flag.
pub(crate) const ADMIN_FLAG: &str = "--ovra-sandbox-setup-admin";
/// Where Windows hides accounts from the sign-in screen.
const USER_LIST: &str =
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts\UserList";

fn error(what: &str, code: u32) -> std::io::Error {
    std::io::Error::other(format!(
        "{what}: {}",
        std::io::Error::from_raw_os_error(code as i32)
    ))
}

/// The sandbox user's password, if setup ran for this data folder.
pub(crate) fn password(data_dir: &Path) -> std::io::Result<String> {
    let sealed = std::fs::read(data_dir.join(PASSWORD_FILE))?;
    let plain = dpapi(&sealed, false)?;
    String::from_utf8(plain).map_err(|_| std::io::Error::other("the sandbox password is damaged"))
}

/// Sets the sandbox up: makes and keeps a new password, then runs the
/// elevated stage (one UAC prompt) and waits for it.
pub(crate) fn setup(data_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let mut bytes = [0u8; 24];
    // SAFETY: the buffer is valid for its length.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(std::io::Error::other("no random numbers for the password"));
    }
    // Letters, digits and symbols, so any password policy accepts it.
    let mut password: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    password.push_str("Aa1!");
    let handoff = data_dir.join(format!("{PASSWORD_FILE}.handoff"));
    std::fs::write(&handoff, &password)?;
    let result = elevated(data_dir, &handoff);
    let _ = std::fs::remove_file(&handoff);
    result?;
    std::fs::write(
        data_dir.join(PASSWORD_FILE),
        dpapi(password.as_bytes(), true)?,
    )
}

/// Runs this program again elevated with [`ADMIN_FLAG`] and waits.
fn elevated(data_dir: &Path, handoff: &Path) -> std::io::Result<()> {
    let me = std::env::current_exe()?;
    let params = format!(
        "{ADMIN_FLAG} {} {}",
        super::token::quote(data_dir.as_os_str()).to_string_lossy(),
        super::token::quote(handoff.as_os_str()).to_string_lossy()
    );
    let (verb, file, params) = (
        wide(OsStr::new("runas")),
        wide(me.as_os_str()),
        wide(OsStr::new(&params)),
    );
    // SAFETY: the strings outlive the call; the process handle is owned.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        if ShellExecuteExW(&mut info) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let process = Handle(info.hProcess);
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, INFINITE, WaitForSingleObject,
        };
        WaitForSingleObject(process.0, INFINITE);
        let mut code = 1u32;
        GetExitCodeProcess(process.0, &mut code);
        if code != 0 {
            return Err(std::io::Error::other(format!(
                "setting the sandbox up failed ({code})"
            )));
        }
    }
    Ok(())
}

/// The elevated stage: the account, its place off the sign-in screen and
/// the firewall rule. Safe to run again: it resets the password and
/// replaces the rule.
pub(crate) fn admin(handoff: &Path) -> std::io::Result<()> {
    let password = std::fs::read_to_string(handoff)?;
    let _ = std::fs::remove_file(handoff);
    let name = wide(OsStr::new(USER));
    let pass = wide(OsStr::new(&password));
    // SAFETY: the structs point at strings that outlive the calls.
    unsafe {
        let mut info: USER_INFO_1 = std::mem::zeroed();
        info.usri1_name = name.as_ptr().cast_mut();
        info.usri1_password = pass.as_ptr().cast_mut();
        info.usri1_priv = USER_PRIV_USER;
        info.usri1_flags = UF_SCRIPT | UF_DONT_EXPIRE_PASSWD | UF_PASSWD_CANT_CHANGE;
        let code = NetUserAdd(
            std::ptr::null(),
            1,
            (&raw const info).cast(),
            std::ptr::null_mut(),
        );
        if code == NERR_UserExists {
            let reset = USER_INFO_1003 {
                usri1003_password: pass.as_ptr().cast_mut(),
            };
            let code = NetUserSetInfo(
                std::ptr::null(),
                name.as_ptr(),
                1003,
                (&raw const reset).cast(),
                std::ptr::null_mut(),
            );
            if code != NERR_Success {
                return Err(error("resetting the sandbox user's password", code));
            }
        } else if code != NERR_Success {
            return Err(error("creating the sandbox user", code));
        }
        // Members of `Users`, like any account; ignore "already a member".
        let member = LOCALGROUP_MEMBERS_INFO_3 {
            lgrmi3_domainandname: name.as_ptr().cast_mut(),
        };
        let users = wide(OsStr::new("Users"));
        let _ = NetLocalGroupAddMembers(
            std::ptr::null(),
            users.as_ptr(),
            3,
            (&raw const member).cast(),
            1,
        );
    }
    hide(true)?;
    firewall(true)
}

/// Removes the account, its sign-in entry and the firewall rule
/// (elevated). The ACEs Ovra added name a SID nobody holds any more.
pub(crate) fn remove() -> std::io::Result<()> {
    firewall(false)?;
    let _ = hide(false);
    let name = wide(OsStr::new(USER));
    // SAFETY: the name outlives the call.
    let code = unsafe { NetUserDel(std::ptr::null(), name.as_ptr()) };
    if code != NERR_Success && code != 2221 {
        return Err(error("removing the sandbox user", code));
    }
    Ok(())
}

/// Keeps the account off the sign-in screen, or lets it back.
fn hide(on: bool) -> std::io::Result<()> {
    let path = wide(OsStr::new(USER_LIST));
    let value = wide(OsStr::new(USER));
    // SAFETY: the key is closed before returning; buffers outlive calls.
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        let code = RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        );
        if code != ERROR_SUCCESS {
            return Err(error("opening the sign-in list", code));
        }
        let code = if on {
            let zero = 0u32;
            RegSetValueExW(
                key,
                value.as_ptr(),
                0,
                REG_DWORD,
                (&raw const zero).cast(),
                4,
            )
        } else {
            RegDeleteValueW(key, value.as_ptr())
        };
        RegCloseKey(key);
        if code != ERROR_SUCCESS && on {
            return Err(error("hiding the sandbox user", code));
        }
    }
    Ok(())
}

/// Adds (after removing any old one) or removes the rule that blocks all
/// outbound traffic of the sandbox user, through Windows' own PowerShell.
fn firewall(on: bool) -> std::io::Result<()> {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    let powershell: PathBuf =
        Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let mut script = format!(
        "$ErrorActionPreference='Stop'; Get-NetFirewallRule -DisplayName '{RULE}' -ErrorAction SilentlyContinue | Remove-NetFirewallRule;"
    );
    if on {
        script.push_str(&format!(
            " $sid=(New-Object System.Security.Principal.NTAccount('{USER}')).Translate([System.Security.Principal.SecurityIdentifier]).Value;\
             New-NetFirewallRule -DisplayName '{RULE}' -Direction Outbound -Action Block -Profile Any -LocalUser \"O:LSD:(A;;CC;;;$sid)\" | Out-Null"
        ));
    }
    let out = Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!(
            "changing the firewall rule: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// Seals (`protect`) or opens bytes with DPAPI for the current account.
fn dpapi(data: &[u8], protect: bool) -> std::io::Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: the input outlives the call; the output is freed with
    // LocalFree as the API asks.
    unsafe {
        let ok = if protect {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        Ok(bytes)
    }
}
