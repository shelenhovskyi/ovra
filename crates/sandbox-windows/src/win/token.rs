//! The session's token and the process that carries it. The token is
//! write-restricted: a write needs the account's own right and one of
//! Everyone, the logon session or the project's SID, and only Ovra grants
//! the project's SID anything. Privileges and the administrators group are
//! taken away, and the integrity level is at most medium.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree, WAIT_FAILED};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    ACL, CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, GetSecurityDescriptorDacl,
    GetTokenInformation, PSECURITY_DESCRIPTOR, SID_AND_ATTRIBUTES, SetTokenInformation,
    TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DEFAULT_DACL, TOKEN_DUPLICATE, TOKEN_GROUPS,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenDefaultDacl, TokenGroups, TokenIntegrityLevel,
    WRITE_RESTRICTED,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, GetCurrentProcess,
    GetExitCodeProcess, INFINITE, OpenProcessToken, PROCESS_INFORMATION, ResumeThread,
    STARTF_USESTDHANDLES, STARTUPINFOW, WaitForSingleObject,
};

use super::{Sid, wide};

/// `SE_GROUP_LOGON_ID`: the group that is this logon session.
const LOGON_ID: u32 = 0xC000_0000;
/// `SE_GROUP_INTEGRITY`.
const GROUP_INTEGRITY: u32 = 0x20;
const EVERYONE: &str = "S-1-1-0";
const ADMINISTRATORS: &str = "S-1-5-32-544";
const MEDIUM: &str = "S-1-16-8192";

/// An owned handle, closed on drop.
#[derive(Debug)]
pub(crate) struct Handle(pub(crate) HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a handle this module opened, closed once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn last_error(what: &str) -> std::io::Error {
    let error = std::io::Error::last_os_error();
    std::io::Error::new(error.kind(), format!("{what}: {error}"))
}

/// This process's token, restricted for the project `project_sid`.
pub(crate) fn restricted(project_sid: &str) -> std::io::Result<Handle> {
    let project = Sid::parse(project_sid)?;
    let everyone = Sid::parse(EVERYONE)?;
    let admins = Sid::parse(ADMINISTRATORS)?;
    // SAFETY: every buffer outlives the calls that read it; handles are
    // owned by `Handle`.
    unsafe {
        let mut own = std::ptr::null_mut();
        let access = TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT;
        if OpenProcessToken(GetCurrentProcess(), access, &mut own) == 0 {
            return Err(last_error("opening this process's token"));
        }
        let own = Handle(own);
        let logon = logon_sid(own.0)?;
        let restrict = [
            SID_AND_ATTRIBUTES {
                Sid: everyone.as_ptr(),
                Attributes: 0,
            },
            SID_AND_ATTRIBUTES {
                Sid: logon.as_ptr(),
                Attributes: 0,
            },
            SID_AND_ATTRIBUTES {
                Sid: project.as_ptr(),
                Attributes: 0,
            },
        ];
        let disable = [SID_AND_ATTRIBUTES {
            Sid: admins.as_ptr(),
            Attributes: 0,
        }];
        let mut token = std::ptr::null_mut();
        let ok = CreateRestrictedToken(
            own.0,
            DISABLE_MAX_PRIVILEGE | WRITE_RESTRICTED,
            disable.len() as u32,
            disable.as_ptr(),
            0,
            std::ptr::null(),
            restrict.len() as u32,
            restrict.as_ptr(),
            &mut token,
        );
        if ok == 0 {
            return Err(last_error("making the session's token"));
        }
        let token = Handle(token);
        // What the session's processes make (pipes, their own processes)
        // must open again under the restricted token.
        let logon_text = sid_text(&logon)?;
        default_dacl(
            token.0,
            &format!("D:(A;;GA;;;{project_sid})(A;;GA;;;{logon_text})(A;;GA;;;SY)(A;;GA;;;OW)"),
        )?;
        let medium = Sid::parse(MEDIUM)?;
        let label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: medium.as_ptr(),
                Attributes: GROUP_INTEGRITY,
            },
        };
        // Lowering only: a medium token stays medium, a high one drops.
        let _ = SetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            (&raw const label).cast(),
            size_of::<TOKEN_MANDATORY_LABEL>() as u32,
        );
        Ok(token)
    }
}

/// The logon session's SID in `token`'s groups.
fn logon_sid(token: HANDLE) -> std::io::Result<Sid> {
    // SAFETY: the buffer is sized by the first call and outlives the reads.
    unsafe {
        let mut len = 0u32;
        GetTokenInformation(token, TokenGroups, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        if GetTokenInformation(token, TokenGroups, buf.as_mut_ptr().cast(), len, &mut len) == 0 {
            return Err(last_error("reading the token's groups"));
        }
        let groups = &*buf.as_ptr().cast::<TOKEN_GROUPS>();
        let all = std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize);
        let logon = all
            .iter()
            .find(|g| g.Attributes & LOGON_ID == LOGON_ID)
            .ok_or_else(|| std::io::Error::other("this process has no logon session SID"))?;
        Sid::parse(&sid_text_raw(logon.Sid)?)
    }
}

fn sid_text(sid: &Sid) -> std::io::Result<String> {
    sid_text_raw(sid.as_ptr())
}

pub(crate) fn sid_text_raw(sid: windows_sys::Win32::Security::PSID) -> std::io::Result<String> {
    // SAFETY: the string is allocated by the API and freed here.
    unsafe {
        let mut text: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(last_error("writing a SID"));
        }
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        let out = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Ok(out)
    }
}

fn default_dacl(token: HANDLE, sddl: &str) -> std::io::Result<()> {
    // SAFETY: the descriptor is freed after SetTokenInformation copied it.
    unsafe {
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let text = wide(OsStr::new(sddl));
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(last_error("building the default DACL"));
        }
        let (mut present, mut defaulted) = (0, 0);
        let mut dacl: *mut ACL = std::ptr::null_mut();
        GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted);
        let value = TOKEN_DEFAULT_DACL { DefaultDacl: dacl };
        let ok = SetTokenInformation(
            token,
            TokenDefaultDacl,
            (&raw const value).cast(),
            size_of::<TOKEN_DEFAULT_DACL>() as u32,
        );
        LocalFree(descriptor);
        if ok == 0 {
            return Err(last_error("setting the default DACL"));
        }
    }
    Ok(())
}

/// Runs `program` with `args` under `token`, with this process's standard
/// handles and `env` as its whole environment, and returns its exit code.
pub(crate) fn run(
    token: &Handle,
    program: &Path,
    args: &[OsString],
    env: &[(OsString, OsString)],
    cwd: &Path,
) -> std::io::Result<u32> {
    let mut line = wide(&command_line(program, args));
    let block = env_block(env);
    // SAFETY: buffers outlive the call; handles are owned by `Handle`.
    unsafe {
        let mut startup: STARTUPINFOW = std::mem::zeroed();
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        startup.dwFlags = STARTF_USESTDHANDLES;
        (startup.hStdInput, startup.hStdOutput, startup.hStdError) = std_handles();
        let mut info: PROCESS_INFORMATION = std::mem::zeroed();
        let program = wide(program.as_os_str());
        let cwd = wide(cwd.as_os_str());
        let ok = CreateProcessAsUserW(
            token.0,
            program.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
            block.as_ptr().cast(),
            cwd.as_ptr(),
            &startup,
            &mut info,
        );
        if ok == 0 {
            return Err(last_error("starting the sandboxed process"));
        }
        let process = Handle(info.hProcess);
        let thread = Handle(info.hThread);
        super::job::assign(process.0)?;
        ResumeThread(thread.0);
        if WaitForSingleObject(process.0, INFINITE) == WAIT_FAILED {
            return Err(last_error("waiting for the sandboxed process"));
        }
        let mut code = 0u32;
        GetExitCodeProcess(process.0, &mut code);
        Ok(code)
    }
}

/// `program` and `args` as one command line.
pub(crate) fn command_line(program: &Path, args: &[OsString]) -> OsString {
    let mut line = quote(program.as_os_str());
    for arg in args {
        line.push(" ");
        line.push(quote(arg));
    }
    line
}

/// `env` as the block `CreateProcess` takes: `NAME=value` strings, then
/// an empty one.
pub(crate) fn env_block(env: &[(OsString, OsString)]) -> Vec<u16> {
    let mut block: Vec<u16> = Vec::new();
    for (name, value) in env {
        let mut pair = name.clone();
        pair.push("=");
        pair.push(value);
        block.extend(wide(&pair));
    }
    if env.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// This process's standard input, output and error.
pub(crate) fn std_handles() -> (HANDLE, HANDLE, HANDLE) {
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    // SAFETY: GetStdHandle has no preconditions.
    unsafe {
        (
            GetStdHandle(STD_INPUT_HANDLE),
            GetStdHandle(STD_OUTPUT_HANDLE),
            GetStdHandle(STD_ERROR_HANDLE),
        )
    }
}

/// One argument as the C runtime splits a command line: quoted when it is
/// empty or holds a space, tab or quote; backslashes doubled before a
/// quote.
pub(crate) fn quote(arg: &OsStr) -> OsString {
    let text: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(arg).collect();
    let needs = text.is_empty() || text.iter().any(|&c| c == 32 || c == 9 || c == 34);
    if !needs {
        return arg.to_os_string();
    }
    let mut out: Vec<u16> = vec![34];
    let mut slashes = 0usize;
    for &c in &text {
        if c == 92 {
            slashes += 1;
            continue;
        }
        if c == 34 {
            out.extend(std::iter::repeat_n(92, slashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n(92, slashes));
        }
        slashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n(92, slashes * 2));
    out.push(34);
    std::os::windows::ffi::OsStringExt::from_wide(&out)
}
