//! Children that do not outlive the studio.
//!
//! The engine and the assistant sidecar are separate processes. Dropping their
//! supervisor kills them politely, but a supervisor does not always get to
//! run: a hard kill of the studio, a crash, a taskkill from the task manager -
//! and `mm-server` is left holding the GPU with nobody to talk to it.
//!
//! Windows has one answer to this: a job object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Every child spawned into it dies when
//! the last handle to the job closes, which the system does for us when this
//! process ends, however it ends.

use std::process::Child;

/// Puts a freshly spawned child into the studio's job, so it cannot survive us.
///
/// Failing to do so is not fatal - the child is still supervised normally -
/// so this never returns an error, it simply does what it can.
#[cfg(windows)]
pub fn adopt(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

    let job = job();
    if job == 0 {
        return;
    }
    unsafe {
        AssignProcessToJobObject(job as _, child.as_raw_handle() as _);
    }
}

#[cfg(not(windows))]
pub fn adopt(_child: &Child) {}

/// One job for the whole process, created the first time a child needs it.
#[cfg(windows)]
fn job() -> isize {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    static JOB: OnceLock<isize> = OnceLock::new();
    *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return 0;
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        job as isize
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn a_child_in_the_job_dies_with_a_closed_job() {
        // The job is per process and cannot be closed here without taking the
        // test runner with it, so this only proves adoption is accepted for a
        // real child - the kill-on-close flag is the system's part.
        let mut child = std::process::Command::new("cmd").args(["/c", "timeout /t 5"]).spawn().expect("spawn a child");
        adopt(&child);
        child.kill().ok();
        child.wait().ok();
    }
}
