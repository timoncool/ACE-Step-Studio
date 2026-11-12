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

/// `adopt` for a raw handle, for children a tokio spawn owns.
#[cfg(windows)]
pub fn adopt_handle(handle: isize) {
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

    let job = job();
    if job == 0 || handle == 0 {
        return;
    }
    unsafe {
        AssignProcessToJobObject(job as _, handle as _);
    }
}

#[cfg(not(windows))]
pub fn adopt_handle(_handle: isize) {}

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

/// The library search path a bundled program starts with on Linux: its own folder first, so it loads the libraries
/// beside it and not another version of the same name an AppImage put on the inherited path (the MIDI tool's older
/// ggml in `usr/lib`). None on other systems, where nothing is inherited this way.
pub fn own_libraries_first(program: impl AsRef<std::path::Path>) -> Option<std::ffi::OsString> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let mut paths = vec![program.as_ref().parent()?.to_path_buf()];
    if let Some(inherited) = std::env::var_os("LD_LIBRARY_PATH") {
        paths.extend(std::env::split_paths(&inherited));
    }
    std::env::join_paths(paths).ok()
}

/// Where a child that crashed without a word went down, as Windows recorded it: the module the fault happened in
/// (a file name, never a path), the exception code and the offset into that module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub module: String,
    pub exception: String,
    pub offset: String,
}

/// Whether a Windows exit code is an exception (0xC0000000 and up: an access violation, an illegal instruction, a
/// failed library load) - the exits Windows keeps a record of. A program that returns an error code of its own has
/// none, and waiting for one would only slow its error down.
pub fn exited_on_exception(code: Option<i32>) -> bool {
    code.is_some_and(|code| (code as u32) >= 0xC000_0000)
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "faulting module {}, exception 0x{}, offset 0x{}", self.module, self.exception, self.offset)
    }
}

/// The Windows Error Reporting record (Application log, Application Error, event 1000) of the process `pid`, which
/// Windows writes a moment after the crash: looked for until `wait` runs out. None anywhere else, and when Windows
/// kept no record (reporting turned off, or the process ended without a fault).
pub fn fault_of(pid: u32, wait: std::time::Duration) -> Option<Fault> {
    #[cfg(windows)]
    {
        let deadline = std::time::Instant::now() + wait;
        loop {
            if let Some(fault) = latest_faults().into_iter().find_map(|xml| fault_in_event(&xml, pid)) {
                return Some(fault);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, wait);
        None
    }
}

/// The Application Error records of the last five minutes, newest first, as event XML.
#[cfg(windows)]
fn latest_faults() -> Vec<String> {
    use windows_sys::Win32::System::EventLog::{EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender, EvtRenderEventXml};

    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let channel = wide("Application");
    let query = wide("*[System[Provider[@Name='Application Error'] and (EventID=1000) and TimeCreated[timediff(@SystemTime) <= 300000]]]");
    let mut found = Vec::new();
    unsafe {
        let results = EvtQuery(0, channel.as_ptr(), query.as_ptr(), EvtQueryChannelPath | EvtQueryReverseDirection);
        if results == 0 {
            return found;
        }
        let mut events = [0isize; 16];
        let mut returned = 0u32;
        if EvtNext(results, events.len() as u32, events.as_mut_ptr(), 1000, 0, &mut returned) != 0 {
            for &event in &events[..returned as usize] {
                let mut used = 0u32;
                let mut properties = 0u32;
                EvtRender(0, event, EvtRenderEventXml, 0, std::ptr::null_mut(), &mut used, &mut properties);
                let mut buffer = vec![0u16; (used as usize).div_ceil(2) + 1];
                if EvtRender(0, event, EvtRenderEventXml, (buffer.len() * 2) as u32, buffer.as_mut_ptr().cast(), &mut used, &mut properties) != 0 {
                    let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
                    found.push(String::from_utf16_lossy(&buffer[..end]));
                }
                EvtClose(event);
            }
        }
        EvtClose(results);
    }
    found
}

/// The fault in one Application Error record when it is the one of `pid`.
fn fault_in_event(xml: &str, pid: u32) -> Option<Fault> {
    let data = |name: &str| -> Option<String> {
        let open = format!("<Data Name='{name}'>");
        let start = xml.find(&open)? + open.len();
        let end = xml[start..].find("</Data>")? + start;
        Some(xml[start..end].trim().to_string())
    };
    let recorded = data("ProcessId")?;
    let recorded = u32::from_str_radix(recorded.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()?;
    if recorded != pid {
        return None;
    }
    // the module's own name only: its path can carry the user's name
    let module = data("ModuleName")?;
    let module = module.rsplit(['\\', '/']).next().unwrap_or(&module).to_string();
    Some(Fault { module, exception: data("ExceptionCode")?, offset: data("FaultingOffset")? })
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

    #[test]
    fn a_crash_record_gives_the_module_the_code_and_the_offset_of_its_own_process() {
        let xml = "<Event><EventData><Data Name='AppName'>crash.exe</Data><Data Name='ModuleName'>ggml-cpu-haswell.dll</Data><Data Name='ExceptionCode'>c000001d</Data><Data Name='FaultingOffset'>0000000000001000</Data><Data Name='ProcessId'>0x17918</Data><Data Name='AppPath'>C:\\Users\\someone\\crash.exe</Data><Data Name='ModulePath'>C:\\Users\\someone\\ggml-cpu-haswell.dll</Data></EventData></Event>";
        let fault = fault_in_event(xml, 0x17918).expect("the record of this process");
        assert_eq!(fault, Fault { module: "ggml-cpu-haswell.dll".into(), exception: "c000001d".into(), offset: "0000000000001000".into() });
        assert_eq!(fault.to_string(), "faulting module ggml-cpu-haswell.dll, exception 0xc000001d, offset 0x0000000000001000");
        assert!(!fault.to_string().contains("someone"), "no path, so no user name");
        assert_eq!(fault_in_event(xml, 42), None, "another process's record is not this one's");
        assert!(exited_on_exception(Some(0xC000_0005u32 as i32)), "an access violation");
        assert!(exited_on_exception(Some(0xC000_001Du32 as i32)), "an illegal instruction");
        assert!(!exited_on_exception(Some(1)), "an error code of the program's own");
        assert!(!exited_on_exception(None), "killed by a signal, no code");
    }

    /// Run with `MUSIC_CORE_CRASHER` naming a program that crashes on start (a null write): Windows' record of it
    /// is found by its process id.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn a_real_crash_is_found_in_the_application_log() {
        let crasher = std::env::var("MUSIC_CORE_CRASHER").expect("MUSIC_CORE_CRASHER names a program that crashes");
        let mut child = std::process::Command::new(&crasher).spawn().expect("start the crasher");
        let pid = child.id();
        let status = child.wait().expect("the crasher ends");
        assert!(!status.success());
        let fault = fault_of(pid, std::time::Duration::from_secs(10)).expect("Windows recorded the crash");
        assert_eq!(fault.exception, "c0000005");
        println!("{fault}");
    }
}
