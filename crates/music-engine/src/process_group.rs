//! Everything the studio starts dies with the studio.
//!
//! The engine is stopped by its supervisor when the studio closes normally, but
//! a normal close is not the only way an application ends: Task Manager, a
//! crash, a `Stop-Process`, a debugger detaching. None of those run destructors,
//! and the engine was left behind holding a graphics card and a port.
//!
//! Windows has one honest answer for this - a job object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Every process started afterwards
//! inherits the job, and when the last handle to it goes away, which happens
//! when this process ends however it ends, the kernel terminates the whole
//! group.

/// Binds this process and everything it starts into one killable group.
///
/// Failure is not fatal: an unelevated process already inside someone else's
/// job (some sandboxes, some CI runners) simply keeps the old behaviour of
/// stopping the engine from its destructor.
#[cfg(windows)]
pub fn bind_children_to_this_process() -> bool {
    use std::mem::size_of;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    unsafe {
        let job: HANDLE = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return false;
        }

        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let assigned = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) != 0
            && AssignProcessToJobObject(job, GetCurrentProcess()) != 0;

        // The handle is deliberately never closed: the job must outlive this
        // function and die with the process, which is exactly what closing the
        // last handle at exit does.
        if assigned {
            let _ = JOB.set(job as isize);
        }
        assigned
    }
}

#[cfg(not(windows))]
pub fn bind_children_to_this_process() -> bool {
    false
}

#[cfg(windows)]
static JOB: std::sync::OnceLock<isize> = std::sync::OnceLock::new();

/// Lets the processes started from now on outlive this one.
///
/// The update installer is started by this process right before it exits; in
/// the kill-on-close group it would die with the studio before installing
/// anything. The engine is not released by this: it is also in its own
/// kill-on-close job (`music_core::process::adopt`) and still ends with us.
#[cfg(windows)]
pub fn release_children() -> bool {
    use std::mem::size_of;

    use windows_sys::Win32::System::JobObjects::{
        JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    };

    let Some(job) = JOB.get() else { return false };
    unsafe {
        let limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        SetInformationJobObject(
            *job as _,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) != 0
    }
}

#[cfg(not(windows))]
pub fn release_children() -> bool {
    false
}
