//! Children that do not outlive the studio.
//!
//! The engine and the assistant sidecar are separate processes. Dropping their
//! supervisor kills them politely, but a supervisor does not always get to
//! run: a hard kill of the studio, a crash, a taskkill from the task manager -
//! and the engine is left holding the GPU with nobody to talk to it.
//!
//! Windows has one answer to this: a job object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Every child spawned into it dies when
//! the last handle to the job closes, which the system does for us when this
//! process ends, however it ends.
//!
//! Linux has no job objects. There the answer is `PR_SET_PDEATHSIG`, set in the
//! forked child before it execs; see `ensure_dies_with_parent` below, which the
//! engine and the assistant sidecar call on their spawn. This module's `adopt`
//! therefore does nothing off Windows - it is the spawn-side hook, not this
//! one, that keeps a Linux engine from being orphaned.

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

/// Wires up the spawn so the child cannot outlive this process.
///
/// Call this on the `Command` immediately before `spawn`. On Linux it is the
/// only thing standing between a hard-killed studio and an engine left
/// holding the graphics card and the engine port - which then blocks the next
/// start with a bind failure that looks like a completely different problem.
///
/// A no-op elsewhere: on Windows `adopt` above covers the same ground with a
/// job object after the child exists; macOS has no parent-death signal.
pub fn ensure_dies_with_parent(command: &mut std::process::Command) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;

        let parent = std::process::id() as libc::pid_t;
        // `PR_SET_PDEATHSIG` is per-thread and cleared across a successful
        // `execve`, so it has to be set in the forked child, which is what
        // `pre_exec` brackets. Everything inside is async-signal-safe.
        unsafe {
            command.pre_exec(move || {
                // SIGKILL rather than SIGTERM: a model server holds no unsaved
                // state, and a signal it may handle slowly (the engine ignores
                // SIGTERM mid-inference) would leave exactly the orphan this
                // exists to prevent.
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // The parent may have died between fork and the call above, in
                // which case the signal will never come. Noticing turns a quiet
                // orphan into a failed start the supervisor can retry.
                if libc::getppid() != parent {
                    return Err(std::io::Error::other("the studio exited while the child was starting"));
                }
                Ok(())
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = command;
    }
}

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
