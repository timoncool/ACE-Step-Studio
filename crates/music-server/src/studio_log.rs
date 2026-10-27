//! The studio's log on disk, `<root>/logs/ace-step-studio.log` (Speech Studio's `diagnostics` log): a crash leaves no
//! other trace, so every line goes straight to the file, with no buffer of ours to lose. A panic of any thread and, on
//! Windows, an exception nobody handles (an engine DLL running an instruction this processor lacks, a bad access) are
//! written there too. Anything in a line that looks like a key is masked.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// A line for the console and the log file, formatted as `eprintln!` formats.
#[macro_export]
macro_rules! studio_log {
    ($($arg:tt)*) => {
        $crate::studio_log::line(&format!($($arg)*))
    };
}

pub const LOG_FILE: &str = "ace-step-studio.log";
pub const LOG_FILE_OLD: &str = "ace-step-studio.old.log";
const LOG_FILE_MAX: u64 = 8 * 1024 * 1024;

struct LogSink {
    file: std::fs::File,
    written: u64,
}

static SINK: Mutex<Option<LogSink>> = Mutex::new(None);
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// The folder the log is written to, once `log_to_file` has run.
pub fn log_dir() -> Option<&'static Path> {
    LOG_DIR.get().map(PathBuf::as_path)
}

/// One line to the console and, once the log is on disk, to the file with the time in front.
pub fn line(text: &str) {
    eprintln!("{text}");
    to_file(&masked(&format!("{} {text}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))));
}

/// Starts writing the log to `<root>/logs` and records crashes there. Called once the data folder is known.
pub fn log_to_file(root: &Path) {
    let dir = root.join("logs");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("[ERROR] {}: the log folder was not created, the log goes to the console only: {error}", dir.display());
        return;
    }
    if LOG_DIR.set(dir.clone()).is_err() {
        return;
    }
    match open_sink(&dir, true) {
        Ok(sink) => *SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(sink),
        Err(error) => {
            eprintln!("[ERROR] {}: the log file was not opened, the log goes to the console only: {error}", dir.join(LOG_FILE).display());
            return;
        }
    }
    to_file(&format!(
        "==== {} {} started {} (pid {})",
        music_core::studio().name,
        crate::studio_version(),
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        std::process::id()
    ));
    let earlier = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        append_apart(&masked(&format!("{} [PANIC] thread {}: {info}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), thread.name().unwrap_or("?"))));
        earlier(info);
    }));
    #[cfg(windows)]
    crash::watch();
}

/// The log file, the one of the last start moved aside when it has grown past `LOG_FILE_MAX` (`rotate_big`).
fn open_sink(dir: &Path, rotate_big: bool) -> std::io::Result<LogSink> {
    let path = dir.join(LOG_FILE);
    if rotate_big && std::fs::metadata(&path).is_ok_and(|meta| meta.len() > LOG_FILE_MAX) {
        std::fs::rename(&path, dir.join(LOG_FILE_OLD))?;
    }
    let file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    let written = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    Ok(LogSink { file, written })
}

/// One line to the log file through a handle of its own: a panicking or crashing thread may hold the log's lock.
fn append_apart(line: &str) {
    let Some(dir) = LOG_DIR.get() else { return };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(LOG_FILE)) {
        let _ = writeln!(file, "{line}");
    }
}

/// One line to the log file, when there is one; past `LOG_FILE_MAX` the file is moved aside and a new one begins.
/// A file that cannot be moved aside takes the lines on, and the move is tried again after another `LOG_FILE_MAX`.
fn to_file(line: &str) {
    let Some(dir) = LOG_DIR.get() else { return };
    let mut sink = SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(open) = sink.as_mut() else { return };
    if open.written > LOG_FILE_MAX {
        *sink = None;
        match open_sink(dir, true) {
            Ok(fresh) => *sink = Some(fresh),
            Err(error) => {
                eprintln!("[ERROR] {}: the log file was not rotated: {error}", dir.join(LOG_FILE).display());
                match open_sink(dir, false) {
                    Ok(mut same) => {
                        same.written = 0;
                        *sink = Some(same);
                    }
                    Err(error) => {
                        eprintln!("[ERROR] {}: the log file was not opened again: {error}", dir.join(LOG_FILE).display());
                        return;
                    }
                }
            }
        }
    }
    let Some(open) = sink.as_mut() else { return };
    let bytes = format!("{line}\n");
    match open.file.write_all(bytes.as_bytes()) {
        Ok(()) => open.written += bytes.len() as u64,
        Err(error) => eprintln!("[ERROR] the log file did not take a line: {error}"),
    }
}

#[cfg(windows)]
mod crash {
    use windows_sys::Win32::Foundation::{EXCEPTION_ACCESS_VIOLATION, EXCEPTION_ILLEGAL_INSTRUCTION, EXCEPTION_STACK_OVERFLOW, HMODULE};
    use windows_sys::Win32::System::Diagnostics::Debug::{SetUnhandledExceptionFilter, EXCEPTION_POINTERS};
    use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};

    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

    pub fn watch() {
        // SAFETY: the filter is a plain function that lives as long as the process.
        unsafe { SetUnhandledExceptionFilter(Some(on_crash)) };
    }

    /// The module holding `addr` and the offset in it, as the Windows event log names a crash.
    fn module_of(addr: usize) -> (String, usize) {
        let mut module: HMODULE = std::ptr::null_mut();
        // SAFETY: the address is only looked up; the module's refcount is left as it is.
        let found = unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, addr as *const u16, &mut module) } != 0;
        if !found {
            return ("?".to_string(), addr);
        }
        let mut name = [0u16; 520];
        // SAFETY: the buffer's length is passed with it.
        let n = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) } as usize;
        (String::from_utf16_lossy(&name[..n.min(name.len())]), addr - module as usize)
    }

    unsafe extern "system" fn on_crash(info: *const EXCEPTION_POINTERS) -> i32 {
        // SAFETY: Windows passes the record of the exception being raised.
        let record = unsafe { info.as_ref().and_then(|info| info.ExceptionRecord.as_ref()) };
        let Some(record) = record else { return EXCEPTION_CONTINUE_SEARCH };
        let code = record.ExceptionCode;
        let what = match code {
            EXCEPTION_ILLEGAL_INSTRUCTION => " (an instruction this processor does not have)",
            EXCEPTION_ACCESS_VIOLATION => " (access violation)",
            EXCEPTION_STACK_OVERFLOW => " (stack overflow)",
            _ => "",
        };
        let (module, offset) = module_of(record.ExceptionAddress as usize);
        let line = format!(
            "{} [CRASH] exception 0x{:08x}{what} in {module} at offset 0x{offset:x}, thread {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            code as u32,
            std::thread::current().name().unwrap_or("?"),
        );
        super::append_apart(&line);
        EXCEPTION_CONTINUE_SEARCH
    }
}

/// Anything that looks like a key masked: OpenRouter, Google, Hugging Face, the network access key, bearer headers.
pub(crate) fn masked(text: &str) -> String {
    static KEYS: OnceLock<regex::Regex> = OnceLock::new();
    let keys = KEYS.get_or_init(|| {
        regex::Regex::new(r"(sk-[A-Za-z0-9_-]{8,}|AIza[0-9A-Za-z_-]{20,}|hf_[A-Za-z0-9]{20,}|key=[A-Za-z0-9_-]{12,}|(?i:bearer)\s+[A-Za-z0-9._-]{12,})").expect("key pattern")
    });
    keys.replace_all(text, "[key]").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_never_reach_the_log() {
        assert_eq!(masked("OpenRouter answered for sk-or-v1-0123456789abcdef"), "OpenRouter answered for [key]");
        assert_eq!(masked("GET /?key=AbCdEf0123456789xyz"), "GET /?[key]");
        assert_eq!(masked("Authorization: Bearer abcdefghijklmnop"), "Authorization: [key]");
        assert_eq!(masked("hf_ABCDEFGHIJKLMNOPQRSTUV token"), "[key] token");
        assert_eq!(masked("the engine started on CUDA0"), "the engine started on CUDA0");
    }

    #[test]
    fn a_line_lands_in_the_file_and_a_big_file_is_moved_aside() {
        let dir = std::env::temp_dir().join(format!("studio-log-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LOG_FILE), vec![b'x'; LOG_FILE_MAX as usize + 1]).unwrap();
        let mut sink = open_sink(&dir, true).unwrap();
        assert_eq!(std::fs::metadata(dir.join(LOG_FILE_OLD)).unwrap().len(), LOG_FILE_MAX + 1, "the big file is kept as the old one");
        sink.file.write_all(b"a line\n").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(LOG_FILE)).unwrap(), "a line\n");
        std::fs::remove_dir_all(&dir).ok();
    }
}
