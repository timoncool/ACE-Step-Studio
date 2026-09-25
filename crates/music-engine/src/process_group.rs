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
        assigned
    }
}

#[cfg(not(windows))]
pub fn bind_children_to_this_process() -> bool {
    false
}
