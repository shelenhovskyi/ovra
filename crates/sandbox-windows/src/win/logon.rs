//! Starting the helper's middle stage as the sandbox user.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use windows_sys::Win32::Security::{LookupAccountNameW, SID_NAME_USE};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessWithLogonW, LOGON_WITH_PROFILE,
    PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW,
};

use super::token::{Handle, command_line, env_block, sid_text_raw, std_handles};
use super::{Sid, wide};

/// The SID of the local account `name`.
pub(crate) fn account_sid(name: &str) -> std::io::Result<Sid> {
    let account = wide(OsStr::new(name));
    let mut sid = vec![0u64; 16];
    let mut sid_len = (sid.len() * 8) as u32;
    let mut domain = vec![0u16; 256];
    let mut domain_len = domain.len() as u32;
    let mut kind: SID_NAME_USE = 0;
    // SAFETY: the buffers are as large as the lengths passed.
    let ok = unsafe {
        LookupAccountNameW(
            std::ptr::null(),
            account.as_ptr(),
            sid.as_mut_ptr().cast(),
            &mut sid_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut kind,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Sid::parse(&sid_text_raw(sid.as_mut_ptr().cast())?)
}

/// Runs `program` with `args` as `user`, with this process's standard
/// handles and `env` as its environment, in this helper's job, and returns
/// its exit code.
pub(crate) fn run_as(
    user: &str,
    password: &str,
    program: &Path,
    args: &[OsString],
    env: &[(OsString, OsString)],
    cwd: &Path,
) -> std::io::Result<u32> {
    let mut line = wide(&command_line(program, args));
    let block = env_block(env);
    let (user, domain, password) = (
        wide(OsStr::new(user)),
        wide(OsStr::new(".")),
        wide(OsStr::new(password)),
    );
    let (program, cwd) = (wide(program.as_os_str()), wide(cwd.as_os_str()));
    // SAFETY: buffers outlive the call; handles are owned by `Handle`.
    unsafe {
        let mut startup: STARTUPINFOW = std::mem::zeroed();
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        startup.dwFlags = STARTF_USESTDHANDLES;
        (startup.hStdInput, startup.hStdOutput, startup.hStdError) = std_handles();
        let mut info: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessWithLogonW(
            user.as_ptr(),
            domain.as_ptr(),
            password.as_ptr(),
            LOGON_WITH_PROFILE,
            program.as_ptr(),
            line.as_mut_ptr(),
            CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
            block.as_ptr().cast(),
            cwd.as_ptr(),
            &startup,
            &mut info,
        );
        if ok == 0 {
            let error = std::io::Error::last_os_error();
            return Err(std::io::Error::new(
                error.kind(),
                format!("starting the session as the sandbox user: {error}"),
            ));
        }
        let (process, thread) = (Handle(info.hProcess), Handle(info.hThread));
        super::token::finish(&process, &thread)
    }
}
