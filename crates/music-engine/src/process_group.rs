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

use std::process::Command;

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
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    if JOB.get().is_some() {
        return true;
    }
    unsafe {
        let job: HANDLE = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return false;
        }

        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        // BREAKAWAY_OK lets nobody out by itself: only a process started with
        // CREATE_BREAKAWAY_FROM_JOB leaves the group (see detach_from_group).
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
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

/// Unix: the studio leads a process group of its own, which every child inherits, so the group can be ended with it
/// (`terminate_group_members`); macOS has nothing like Linux's PR_SET_PDEATHSIG to end children with their parent. A
/// process started from a terminal keeps the terminal's group, so Ctrl+C still reaches it.
#[cfg(unix)]
pub fn bind_children_to_this_process() -> bool {
    unsafe { libc::isatty(0) == 0 && libc::setpgid(0, 0) == 0 }
}

#[cfg(not(any(windows, unix)))]
pub fn bind_children_to_this_process() -> bool {
    false
}

#[cfg(windows)]
static JOB: std::sync::OnceLock<isize> = std::sync::OnceLock::new();

/// Lets the processes started from now on outlive this one.
///
/// The update installer is started by this process right before it exits; in
/// the kill-on-close group it would die with the studio before installing
/// anything. The sidecars still running would then outlive the studio, so
/// `terminate_group_members` ends them first. The engine also sits in its own
/// kill-on-close job (`music_core::process::adopt`) and ends with us either way.
/// BREAKAWAY_OK stays: detach_from_group keeps using CREATE_BREAKAWAY_FROM_JOB.
#[cfg(windows)]
pub fn release_children() -> bool {
    use std::mem::size_of;

    use windows_sys::Win32::System::JobObjects::{
        JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_BREAKAWAY_OK,
    };

    let Some(job) = JOB.get() else { return false };
    unsafe {
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        SetInformationJobObject(
            *job as _,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) != 0
    }
}

/// Ends every process of the group but this one (the sidecars: the engine,
/// llama-server, bs_roformer-cli, Whisper, the HOT-Step tools).
#[cfg(windows)]
pub fn terminate_group_members() {
    let Some(&job) = JOB.get() else { return };
    use std::mem::size_of;

    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{JobObjectBasicProcessIdList, QueryInformationJobObject};
    use windows_sys::Win32::System::Threading::{GetCurrentProcessId, OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    #[repr(C)]
    struct PidList {
        assigned: u32,
        listed: u32,
        pids: [usize; 1024],
    }

    let me = unsafe { GetCurrentProcessId() } as usize;
    let mut list = PidList { assigned: 0, listed: 0, pids: [0; 1024] };
    let ok = unsafe {
        QueryInformationJobObject(
            job as _,
            JobObjectBasicProcessIdList,
            (&raw mut list).cast(),
            size_of::<PidList>() as u32,
            std::ptr::null_mut(),
        ) != 0
    };
    if !ok {
        eprintln!("[ERROR] the studio process group list was not read: sidecars may outlive the exit");
        return;
    }
    for &pid in list.pids.iter().take(list.listed as usize) {
        if pid == me {
            continue;
        }
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid as u32);
            if h.is_null() {
                eprintln!("[ERROR] process {pid} of the studio group was not opened for termination");
                continue;
            }
            if TerminateProcess(h, 1) == 0 {
                eprintln!("[ERROR] process {pid} of the studio group was not terminated");
            }
            CloseHandle(h);
        }
    }
}

#[cfg(not(windows))]
pub fn release_children() -> bool {
    false
}

/// Unix: ends every process of the studio's own group (the servers and engines it started); the studio ignores the
/// signal itself, as it is exiting anyway. Nothing when the group is not the studio's own.
#[cfg(unix)]
pub fn terminate_group_members() {
    unsafe {
        if libc::getpgrp() == libc::getpid() {
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
            libc::killpg(libc::getpgrp(), libc::SIGTERM);
            // an ignored signal is dropped when sent, so the default can come back: what starts after inherits it
            libc::signal(libc::SIGTERM, libc::SIG_DFL);
        }
    }
}

#[cfg(not(any(windows, unix)))]
pub fn terminate_group_members() {}

/// What the studio opens for the person (the file manager, a browser) must not
/// close with the studio. The flag is set only when the group is the studio's
/// own: inside someone else's job without BREAKAWAY_OK, CreateProcess with it
/// fails with access denied.
#[cfg(windows)]
pub fn detach_from_group(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_BREAKAWAY_FROM_JOB;

    if JOB.get().is_some() {
        cmd.creation_flags(CREATE_BREAKAWAY_FROM_JOB);
    }
    cmd
}

/// Unix: a process group of its own, so the studio's `killpg` at exit passes it by.
#[cfg(unix)]
pub fn detach_from_group(cmd: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt;

    cmd.process_group(0)
}

#[cfg(not(any(windows, unix)))]
pub fn detach_from_group(cmd: &mut Command) -> &mut Command {
    cmd
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Stdio};
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::JobObjects::{
        IsProcessInJob, JobObjectExtendedLimitInformation, QueryInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    const OWNER_ENV: &str = "ACE_PROCESS_GROUP_OWNER";
    const TERMINATOR_ENV: &str = "ACE_PROCESS_GROUP_TERMINATOR";

    fn long_child(detached: bool) -> Child {
        let mut cmd = Command::new("ping");
        cmd.args(["-n", "60", "127.0.0.1"]).stdout(Stdio::null()).stderr(Stdio::null());
        if detached {
            detach_from_group(&mut cmd);
        }
        cmd.spawn().expect("start ping")
    }

    fn in_our_job(child: &Child) -> bool {
        let job = *JOB.get().expect("the group exists");
        let mut result = 0;
        let ok = unsafe { IsProcessInJob(child.as_raw_handle() as _, job as _, &mut result) };
        assert_ne!(ok, 0, "IsProcessInJob: {}", std::io::Error::last_os_error());
        result != 0
    }

    fn limit_flags() -> u32 {
        let job = *JOB.get().expect("the group exists");
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            QueryInformationJobObject(
                job as _,
                JobObjectExtendedLimitInformation,
                (&raw mut info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(ok, 0, "QueryInformationJobObject: {}", std::io::Error::last_os_error());
        info.BasicLimitInformation.LimitFlags
    }

    #[test]
    fn children_inherit_the_group_unless_detached_and_release_lifts_the_kill() {
        assert!(bind_children_to_this_process(), "the test process did not get a job of its own");
        let flags = limit_flags();
        assert_ne!(flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, 0);
        assert_ne!(flags & JOB_OBJECT_LIMIT_BREAKAWAY_OK, 0);

        let mut inherited = long_child(false);
        let mut detached = long_child(true);
        let inherited_in = in_our_job(&inherited);
        let detached_in = in_our_job(&detached);
        for c in [&mut inherited, &mut detached] {
            c.kill().ok();
            c.wait().ok();
        }
        assert!(inherited_in, "an ordinary child inherits the group");
        assert!(!detached_in, "detach_from_group takes a process out of the group");

        assert!(release_children());
        let flags = limit_flags();
        assert_eq!(flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, 0);
        assert_ne!(flags & JOB_OBJECT_LIMIT_BREAKAWAY_OK, 0, "after release a process can still leave the group");
    }

    /// The group's owner for terminate_kills_members_but_not_the_owner: a process of its own, so the children of the
    /// other tests of this process are left alone.
    #[test]
    #[ignore = "started by terminate_kills_members_but_not_the_owner"]
    fn terminating_owner() {
        if std::env::var_os(TERMINATOR_ENV).is_none() {
            return;
        }
        assert!(bind_children_to_this_process());
        let mut child = long_child(false);
        terminate_group_members();
        let exited = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            matches!(child.try_wait(), Ok(Some(_)))
        });
        println!("CHILD_EXITED={exited}");
    }

    #[test]
    fn terminate_kills_members_but_not_the_owner() {
        let exe = std::env::current_exe().expect("the test executable");
        let out = Command::new(exe)
            .args(["process_group::tests::terminating_owner", "--exact", "--ignored", "--nocapture"])
            .env(TERMINATOR_ENV, "1")
            .stderr(Stdio::null())
            .output()
            .expect("start the group's owner");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("CHILD_EXITED=true"), "the group's child was not ended: {text}");
        assert!(out.status.success(), "the owner ended itself: {text}");
    }

    /// The group's owner for a_killed_owner_takes_its_children_down: started by it as a process of its own (this same
    /// test executable), it does nothing by itself.
    #[test]
    #[ignore = "started by a_killed_owner_takes_its_children_down"]
    fn group_owner() {
        if std::env::var_os(OWNER_ENV).is_none() {
            return;
        }
        assert!(bind_children_to_this_process());
        let mut child = long_child(false);
        println!("GRANDCHILD={}", child.id());
        std::thread::sleep(std::time::Duration::from_secs(60));
        child.kill().ok();
        child.wait().ok();
    }

    #[test]
    fn a_killed_owner_takes_its_children_down() {
        let exe = std::env::current_exe().expect("the test executable");
        let mut owner = Command::new(exe)
            .args(["process_group::tests::group_owner", "--exact", "--ignored", "--nocapture"])
            .env(OWNER_ENV, "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the group's owner");
        let out = owner.stdout.take().expect("the owner's stdout");
        let pid = BufReader::new(out)
            .lines()
            .map_while(Result::ok)
            .find_map(|l| l.strip_prefix("GRANDCHILD=").and_then(|p| p.trim().parse::<u32>().ok()))
            .expect("the owner did not name its child's pid");
        // the handle is opened before the owner is killed, so the child's pid cannot pass to another process
        let grandchild =
            unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        assert!(!grandchild.is_null(), "OpenProcess: {}", std::io::Error::last_os_error());

        owner.kill().expect("kill the owner");
        owner.wait().ok();
        let waited = unsafe { WaitForSingleObject(grandchild, 10_000) };
        unsafe { CloseHandle(grandchild) };
        assert_eq!(waited, WAIT_OBJECT_0, "the child outlived the killed owner of its group");
    }
}
