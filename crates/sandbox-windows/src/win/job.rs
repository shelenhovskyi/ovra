//! The job object that holds every process of the session: when the helper
//! ends, however it ends, Windows ends them all.

use std::sync::OnceLock;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};

use super::token::Handle;

struct Job(Handle);

// SAFETY: a job handle may be used from any thread.
unsafe impl Send for Job {}
// SAFETY: as above; the handle is never changed after it is made.
unsafe impl Sync for Job {}

static JOB: OnceLock<std::io::Result<Job>> = OnceLock::new();

fn make() -> std::io::Result<Job> {
    // SAFETY: the limits struct outlives the call; the handle is owned.
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Handle(job);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Job(job))
    }
}

/// Puts `process` (started suspended) in this helper's job.
pub(crate) fn assign(process: HANDLE) -> std::io::Result<()> {
    let job = JOB
        .get_or_init(make)
        .as_ref()
        .map_err(|e| std::io::Error::new(e.kind(), format!("making the session's job: {e}")))?;
    // SAFETY: both handles are open.
    if unsafe { AssignProcessToJobObject(job.0.0, process) } == 0 {
        let error = std::io::Error::last_os_error();
        return Err(std::io::Error::new(
            error.kind(),
            format!("putting the session in its job: {error}"),
        ));
    }
    Ok(())
}
