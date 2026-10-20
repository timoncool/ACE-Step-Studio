//! Agent CLIs as the writing assistant: one headless run per draft.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

/// A hanging probe counts as absent.
const PROBE_LIMIT: Duration = Duration::from_secs(15);
const DETECT_TTL: Duration = Duration::from_secs(5 * 60);
/// Whole-run ceiling, matching the HTTP providers.
const RUN_LIMIT: Duration = Duration::from_secs(600);
const OUTPUT_CAP_BYTES: u64 = 8 * 1024 * 1024;
/// Max prompt chars passed as argv (the Windows command line ends at 32767).
const MAX_ARGV_PROMPT_CHARS: usize = 24_000;

/// One agent CLI the studio knows how to run headless.
pub struct Kind {
    /// Stable id stored in the settings.
    pub id: &'static str,
    pub display_name: &'static str,
    pub command: &'static str,
    /// Model flag; `None` means the CLI always runs its configured model.
    pub model_flag: Option<&'static str>,
    pub suggested_models: &'static [&'static str],
    pub install: &'static str,
    pub signin: &'static str,
    pub docs: Option<&'static str>,
}

/// Order matters: UI order, and the default when none is chosen.
pub static KINDS: &[Kind] = &[
    Kind {
        id: "claude",
        display_name: "Claude Code",
        command: "claude",
        model_flag: Some("--model"),
        suggested_models: &["opus", "sonnet", "haiku"],
        install: "npm install -g @anthropic-ai/claude-code",
        signin: "run `claude auth login` once in a terminal, then try again",
        docs: Some("https://code.claude.com/docs/en/cli-reference"),
    },
    Kind {
        id: "codex",
        display_name: "Codex CLI",
        command: "codex",
        model_flag: Some("--model"),
        suggested_models: &[],
        install: "npm install -g @openai/codex",
        signin: "run `codex login` once in a terminal, then try again",
        docs: Some("https://learn.chatgpt.com/docs/codex/cli"),
    },
    Kind {
        id: "opencode",
        display_name: "opencode",
        command: "opencode",
        model_flag: Some("--model"),
        suggested_models: &[],
        install: "npm install -g opencode-ai",
        signin: "run `opencode auth login` once in a terminal, then try again",
        docs: Some("https://opencode.ai/docs/cli/"),
    },
    Kind {
        id: "antigravity",
        display_name: "Antigravity (agy)",
        command: "agy",
        // Whether `--model` is passed is decided per binary by the `--help` probe.
        model_flag: Some("--model"),
        suggested_models: &[],
        install: "download Antigravity, which ships the `agy` CLI",
        signin: "run `agy` once in a terminal and sign in, then try again",
        docs: Some("https://antigravity.google/docs/cli/headless/"),
    },
    Kind {
        id: "muse",
        display_name: "Muse Code",
        command: "muse",
        model_flag: Some("--model"),
        suggested_models: &[],
        install: "install Muse Code",
        signin: "run `muse` once in a terminal and sign in, then try again",
        docs: None,
    },
];

pub fn find_kind(id: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|kind| kind.id == id)
}

#[derive(Debug, Clone, Serialize)]
pub struct Detected {
    pub id: &'static str,
    pub display_name: &'static str,
    pub path: String,
    pub version: String,
    pub default_model: Option<String>,
    /// Version-dependent abilities of this binary.
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub default_model: Option<String>,
    pub features: Vec<String>,
    pub suggested_models: &'static [&'static str],
    pub install: &'static str,
    pub signin: &'static str,
    pub docs: Option<&'static str>,
}

pub fn list() -> Vec<AgentInfo> {
    let detected = detect();
    KINDS
        .iter()
        .map(|kind| {
            let found = detected.iter().find(|item| item.id == kind.id);
            AgentInfo {
                id: kind.id,
                display_name: kind.display_name,
                installed: found.is_some(),
                path: found.map(|item| item.path.clone()),
                version: found.map(|item| item.version.clone()),
                default_model: found.and_then(|item| item.default_model.clone()),
                features: found.map(|item| item.features.clone()).unwrap_or_default(),
                suggested_models: kind.suggested_models,
                install: kind.install,
                signin: kind.signin,
                docs: kind.docs,
            }
        })
        .collect()
}

static DETECT_CACHE: Mutex<Option<(Instant, Vec<Detected>)>> = Mutex::new(None);

/// Installed CLIs, probed in parallel and cached for five minutes.
pub fn detect() -> Vec<Detected> {
    if let Ok(cache) = DETECT_CACHE.lock() {
        if let Some((at, detected)) = cache.as_ref() {
            if at.elapsed() < DETECT_TTL {
                return detected.clone();
            }
        }
    }
    let detected: Vec<Detected> = std::thread::scope(|scope| {
        let probed: Vec<_> = KINDS.iter().map(|kind| scope.spawn(move || probe(kind))).collect();
        probed.into_iter().filter_map(|task| task.join().ok().flatten()).collect()
    });
    if let Ok(mut cache) = DETECT_CACHE.lock() {
        *cache = Some((Instant::now(), detected.clone()));
    }
    detected
}

fn probe(kind: &'static Kind) -> Option<Detected> {
    let resolved = resolve(kind, None).ok()?;
    let output = run_limited(&resolved, &["--version".into()], PROBE_LIMIT).ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).lines().next().unwrap_or_default().trim().chars().take(64).collect::<String>();
    if version.is_empty() {
        return None;
    }
    Some(Detected {
        id: kind.id,
        display_name: kind.display_name,
        path: resolved.program.to_string_lossy().into_owned(),
        version,
        default_model: default_model(kind),
        features: probe_features(kind, &resolved),
    })
}

/// Best-effort default model from the CLI's own config.
fn default_model(kind: &Kind) -> Option<String> {
    match kind.id {
        "claude" => home_dir()
            .map(|home| home.join(".claude").join("settings.json"))
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|json| json.get("model")?.as_str().map(str::to_string)),
        "codex" => {
            let path = std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .or_else(|| home_dir().map(|home| home.join(".codex")))?
                .join("config.toml");
            let text = std::fs::read_to_string(path).ok()?;
            let line = text.lines().map(str::trim).find(|line| line.starts_with("model") && line.contains('='))?;
            let value = line.split_once('=')?.1.trim().trim_matches('"').trim();
            (!value.is_empty()).then(|| value.to_string())
        }
        "opencode" => global_json_model(&[".config/opencode/opencode.json", "opencode/opencode.json"]),
        "antigravity" => home_dir()
            .map(|home| home.join(".gemini").join("antigravity-cli").join("settings.json"))
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|json| json.get("model")?.as_str().map(str::to_string)),
        "muse" => {
            let roots = [
                std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
                home_dir().map(|home| home.join(".config")),
            ];
            roots
                .into_iter()
                .flatten()
                .map(|root| root.join("muse").join("settings.json"))
                .find_map(|path| std::fs::read_to_string(path).ok())
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|json| json.get("model")?.as_str().map(str::to_string))
        }
        _ => None,
    }
    .map(|model| model.trim().to_string())
    .filter(|model| !model.is_empty())
}

/// `model` from a global JSON config, tried under home then roaming.
fn global_json_model(candidates: &[&str]) -> Option<String> {
    let home = home_dir()?;
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from);
    [&home]
        .into_iter()
        .chain(roaming.as_ref())
        .flat_map(|root| candidates.iter().map(|leaf| root.join(leaf)))
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json.get("model")?.as_str().map(str::to_string))
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// Version-dependent flags from the binary's own `--help` (`agy` only).
fn probe_features(kind: &Kind, resolved: &ResolvedCli) -> Vec<String> {
    if kind.id != "antigravity" {
        return Vec::new();
    }
    let Ok(output) = run_limited(&resolved, &["--help".into()], PROBE_LIMIT) else {
        return Vec::new();
    };
    let help = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let mut features = Vec::new();
    if help.contains("--output-format") {
        features.push("json-output".to_string());
    }
    if help.contains("--model ") || help.contains("--model,") || help.ends_with("--model") {
        features.push("model-flag".to_string());
    }
    if help.contains("--sandbox") {
        features.push("sandbox".to_string());
    }
    features
}

/// A CLI ready to spawn.
#[derive(Debug)]
pub struct ResolvedCli {
    pub program: PathBuf,
    pub prefix_args: Vec<std::ffi::OsString>,
    /// True when spawning through `cmd /c`: prompts must not travel as argv.
    pub via_shell: bool,
}

/// Locate a CLI: settings override, `MUSIC_CLI_<ID>_BIN`, then PATH.
pub fn resolve(kind: &Kind, override_path: Option<&str>) -> Result<ResolvedCli, String> {
    if let Some(path) = override_path.map(str::trim).filter(|path| !path.is_empty()) {
        let candidate = PathBuf::from(path);
        if !candidate.is_file() {
            return Err(format!("{} is set as the {} executable, but it is not a file", candidate.display(), kind.display_name));
        }
        return Ok(resolve_shim(kind, &candidate));
    }
    let env_name = format!("MUSIC_CLI_{}_BIN", kind.id.to_ascii_uppercase());
    if let Some(path) = std::env::var_os(&env_name).map(PathBuf::from).filter(|path| !path.as_os_str().is_empty()) {
        if !path.is_file() {
            return Err(format!("{} names {}, but it is not a file", env_name, path.display()));
        }
        return Ok(resolve_shim(kind, &path));
    }
    search_path(kind.command)
        .map(|path| resolve_shim(kind, &path))
        .ok_or_else(|| format!("{} is not installed (looked for `{}` on PATH): {}", kind.display_name, kind.command, kind.install))
}

/// First `command` on PATH; on Windows the registry PATH is included.
fn search_path(command: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let dirs = {
        let mut dirs = path_dirs(std::env::var_os("PATH"));
        for extra in registry_path_dirs() {
            if !dirs.contains(&extra) {
                dirs.push(extra);
            }
        }
        dirs
    };
    #[cfg(not(windows))]
    let dirs = path_dirs(std::env::var_os("PATH"));
    find_in_dirs(command, &dirs)
}

fn path_dirs(value: Option<std::ffi::OsString>) -> Vec<PathBuf> {
    let Some(value) = value else { return Vec::new() };
    std::env::split_paths(&value).collect()
}

fn find_in_dirs(command: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        return find_in_dirs_with_extensions(command, dirs, &pathext());
    }
    #[cfg(not(windows))]
    {
        for dir in dirs {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let candidate = dir.join(command);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }
}

/// Windows lookup with explicit extensions: an exact name wins, then each in order.
#[cfg(windows)]
fn find_in_dirs_with_extensions(command: &str, dirs: &[PathBuf], extensions: &[String]) -> Option<PathBuf> {
    let has_extension = std::path::Path::new(command).extension().is_some();
    for dir in dirs {
        if dir.as_os_str().is_empty() {
            continue;
        }
        if has_extension {
            let candidate = dir.join(command);
            if candidate.is_file() {
                return Some(candidate);
            }
            continue;
        }
        for extension in extensions {
            let candidate = dir.join(format!("{command}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn pathext() -> Vec<String> {
    std::env::var("PATHEXT")
        .map(|value| value.split(';').map(str::trim).filter(|part| !part.is_empty()).map(|part| part.to_ascii_lowercase()).collect::<Vec<String>>())
        .ok()
        .filter(|extensions| !extensions.is_empty())
        .unwrap_or_else(|| [".com", ".exe", ".bat", ".cmd"].iter().map(|extension| extension.to_string()).collect())
}

/// Current user + machine PATH from the registry.
#[cfg(windows)]
fn registry_path_dirs() -> Vec<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, HKEY};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn read_value(root: HKEY, subkey: &str, name: &str) -> Option<String> {
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            if RegOpenKeyExW(root, wide(subkey).as_ptr(), 0, KEY_READ, &mut key) != 0 {
                return None;
            }
            let mut kind = 0u32;
            let mut bytes = 0u32;
            let name_wide = wide(name);
            if RegQueryValueExW(key, name_wide.as_ptr(), std::ptr::null_mut(), &mut kind, std::ptr::null_mut(), &mut bytes) != 0 || bytes == 0 {
                RegCloseKey(key);
                return None;
            }
            let mut data = vec![0u16; bytes as usize / 2 + 1];
            let ok = RegQueryValueExW(key, name_wide.as_ptr(), std::ptr::null_mut(), &mut kind, data.as_mut_ptr() as *mut u8, &mut bytes) == 0;
            RegCloseKey(key);
            if !ok {
                return None;
            }
            let units = bytes as usize / 2;
            Some(std::ffi::OsString::from_wide(&data[..units]).to_string_lossy().trim_end_matches('\0').to_owned())
        }
    }

    let user = read_value(HKEY_CURRENT_USER, "Environment", "Path").unwrap_or_default();
    let machine = read_value(HKEY_LOCAL_MACHINE, "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment", "Path").unwrap_or_default();
    let expanded = [user, machine].into_iter().map(|value| expand_percent_vars(&value)).collect::<Vec<_>>();
    expanded.iter().flat_map(|value| std::env::split_paths(value)).collect()
}

/// Expand `%NAME%` against the environment; unknown names stay verbatim.
#[cfg(windows)]
fn expand_percent_vars(text: &str) -> String {
    let mut expanded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        expanded.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match std::env::var_os(name) {
                    Some(value) => expanded.push_str(&value.to_string_lossy()),
                    None => {
                        expanded.push('%');
                        expanded.push_str(name);
                        expanded.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                expanded.push('%');
                rest = after;
            }
        }
    }
    expanded.push_str(rest);
    expanded
}

/// Resolve shims to something spawnable: npm shims to `node script.js`,
/// `muse.cmd` to its `muse-bin-*.exe`, anything else to `cmd /d /c`.
fn resolve_shim(kind: &Kind, path: &Path) -> ResolvedCli {
    let direct = || ResolvedCli { program: path.to_path_buf(), prefix_args: Vec::new(), via_shell: false };
    let extension = path.extension().and_then(|extension| extension.to_str()).map(|extension| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("cmd") | Some("bat") => {
            if kind.id == "muse" {
                if let Some(binary) = muse_shim_binary(path) {
                    return ResolvedCli { program: binary, prefix_args: Vec::new(), via_shell: false };
                }
            }
            if let Some((node, script)) = npm_shim_target(path) {
                return ResolvedCli { program: node, prefix_args: vec![script.into_os_string()], via_shell: false };
            }
            let shell = std::env::var_os("ComSpec").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("cmd.exe"));
            ResolvedCli {
                program: shell,
                prefix_args: vec!["/d".into(), "/c".into(), path.as_os_str().to_os_string()],
                via_shell: true,
            }
        }
        _ => direct(),
    }
}

/// The `node script.js` an npm `.cmd` shim wraps, relative to its own folder.
fn npm_shim_target(shim: &Path) -> Option<(PathBuf, PathBuf)> {
    let text = std::fs::read_to_string(shim).ok()?;
    let folder = shim.parent()?;
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("node.exe") {
            continue;
        }
        let mut quoted = Vec::new();
        let mut rest = line;
        while let Some(open) = rest.find('"') {
            rest = &rest[open + 1..];
            let Some(close) = rest.find('"') else { break };
            quoted.push(rest[..close].trim().to_string());
            rest = &rest[close + 1..];
        }
        let script = quoted.iter().skip_while(|segment| !segment.to_ascii_lowercase().contains("node.exe")).nth(1)?;
        let node = folder.join("node.exe");
        let node = if node.is_file() { node } else { PathBuf::from("node") };
        let script = script.replace("%dp0%", &format!("{}\\", folder.display())).replace("%~dp0", &format!("{}\\", folder.display()));
        let script = PathBuf::from(script);
        if script.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("js")) {
            return Some((node, script));
        }
    }
    None
}

/// The `muse-bin-*.exe` behind `muse.cmd`.
fn muse_shim_binary(shim: &Path) -> Option<PathBuf> {
    let folder = shim.parent()?;
    if let Ok(version) = std::fs::read_to_string(folder.join(".muse-version")) {
        let candidate = folder.join(format!("muse-bin-{}.exe", version.trim()));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let mut binaries: Vec<PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
                let lower = name.to_ascii_lowercase();
                lower.starts_with("muse-bin-") && lower.ends_with(".exe")
            })
        })
        .collect();
    // By version, not by name: "1.4.10" sorts before "1.4.9" as text.
    binaries.sort_by(|left, right| version_key(left).cmp(&version_key(right)));
    binaries.pop()
}

fn version_key(path: &Path) -> Vec<u64> {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let version = name.strip_prefix("muse-bin-").or_else(|| name.strip_prefix("MUSE-BIN-")).unwrap_or(name);
    let version = version.strip_suffix(".exe").or_else(|| version.strip_suffix(".EXE")).unwrap_or(version);
    version.split(|char: char| !char.is_ascii_digit()).filter(|part| !part.is_empty()).filter_map(|part| part.parse().ok()).collect()
}

/// Run a short probe (`--version`, `--help`) with a timeout. Never for a run.
fn run_limited(resolved: &ResolvedCli, args: &[std::ffi::OsString], limit: Duration) -> std::io::Result<std::process::Output> {
    let program = &resolved.program;
    let mut command = command_line(resolved, args);
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_window(&mut command);
    let mut child = command.spawn()?;
    // Draining on threads: reading after the wait would deadlock on a full pipe.
    let stdout_thread = child.stdout.take().map(|mut out| std::thread::spawn(move || {
        use std::io::Read;
        let mut bytes = Vec::new();
        out.read_to_end(&mut bytes).ok();
        bytes
    }));
    let stderr_thread = child.stderr.take().map(|mut err| std::thread::spawn(move || {
        use std::io::Read;
        let mut bytes = Vec::new();
        err.read_to_end(&mut bytes).ok();
        bytes
    }));
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let stdout = stdout_thread.and_then(|thread| thread.join().ok()).unwrap_or_default();
            let stderr = stderr_thread.and_then(|thread| thread.join().ok()).unwrap_or_default();
            return Ok(std::process::Output { status, stdout, stderr });
        }
        if started.elapsed() > limit {
            child.kill().ok();
            child.wait().ok();
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("{} did not answer in time", program.display())));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Spawn without flashing a console window.
fn hide_window(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// Classified failures carry a trailing machine tag the settings panel translates.
#[derive(Debug)]
pub enum Failure {
    NotSignedIn(String),
    NoCredit(String),
    Throttled(String),
    Failed(String),
}

impl Failure {
    pub fn message(&self) -> String {
        match self {
            Failure::NotSignedIn(message) => format!("{message} [cli_not_signed_in]"),
            Failure::NoCredit(message) => format!("{message} [cli_no_credit]"),
            Failure::Throttled(message) => format!("{message} [cli_throttled]"),
            Failure::Failed(message) => message.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseMode {
    Claude,
    Codex,
    Opencode,
    AgyText,
    AgyJson,
    Muse,
}

#[derive(Debug)]
struct Invocation {
    args: Vec<std::ffi::OsString>,
    env: Vec<(String, String)>,
    stdin_text: Option<String>,
    parse: ParseMode,
}

/// A run in flight.
pub struct PendingRun {
    child: tokio::process::Child,
    kind: &'static Kind,
    parse: ParseMode,
    _workdir: tempfile::TempDir,
}

/// Real per-user temp, ignoring the studio's own redirection.
fn system_temp_dir() -> PathBuf {
    system_temp_dir_from(std::env::var_os("LOCALAPPDATA"))
}

fn system_temp_dir_from(local_app_data: Option<std::ffi::OsString>) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(root) = local_app_data.map(PathBuf::from).filter(|path| !path.as_os_str().is_empty()) {
            let temp = root.join("Temp");
            if std::fs::create_dir_all(&temp).is_ok() {
                return temp;
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = local_app_data;
        let tmp = PathBuf::from("/tmp");
        if tmp.is_dir() {
            return tmp;
        }
    }
    std::env::temp_dir()
}

fn sanitized_temp_env(temp: &Path) -> Vec<(String, String)> {
    let temp = temp.to_string_lossy().into_owned();
    vec![("TMPDIR".into(), temp.clone()), ("TMP".into(), temp.clone()), ("TEMP".into(), temp)]
}

/// Start one headless run in a scratch folder the CLI never leaves.
pub async fn spawn(kind: &'static Kind, resolved: &ResolvedCli, system: &str, user: &str, model: Option<&str>) -> Result<PendingRun, Failure> {
    let system_temp = system_temp_dir();
    let workdir = tempfile::Builder::new()
        .prefix("ace-cli-")
        .tempdir_in(&system_temp)
        .map_err(|error| Failure::Failed(format!("{} could not get a work folder: {error}", kind.display_name)))?;
    let invocation = build_invocation(kind, resolved, workdir.path(), system, user, model)?;
    let mut command = command_line(resolved, &invocation.args);
    for (name, value) in &invocation.env {
        command.env(name, value);
    }
    for (name, value) in sanitized_temp_env(&system_temp) {
        command.env(name, value);
    }
    command.current_dir(workdir.path());
    if invocation.stdin_text.is_some() {
        command.stdin(Stdio::piped());
    } else {
        // Closed, not inherited: a login prompt must fail, never hang unseen.
        command.stdin(Stdio::null());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_window(&mut command);
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| Failure::Failed(format!("{} did not start ({}): {error}", kind.display_name, resolved.program.display())))?;
    adopt_tokio_child(&child);
    if let Some(text) = invocation.stdin_text {
        use tokio::io::AsyncWriteExt;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .await
                .map_err(|error| Failure::Failed(format!("{} did not take the prompt: {error}", kind.display_name)))?;
            drop(stdin);
        }
    }
    Ok(PendingRun { child, kind, parse: invocation.parse, _workdir: workdir })
}

impl PendingRun {
    pub async fn kill(&mut self) {
        self.child.kill().await.ok();
    }

    /// Wait for the answer, at most ten minutes.
    pub async fn run_to_completion(&mut self) -> Result<String, Failure> {
        let kind = self.kind;
        let parse = self.parse;
        let stdout = self.child.stdout.take();
        let stderr = self.child.stderr.take();
        let waited = tokio::time::timeout(RUN_LIMIT, async {
            let (out, err, status) = tokio::join!(drain_stdout(stdout), drain_stderr(stderr), self.child.wait());
            (out, err, status)
        })
        .await;
        let (out, err, status) = match waited {
            Ok((out, err, Ok(status))) => (out, err, status),
            Ok((_, _, Err(error))) => return Err(Failure::Failed(format!("{} could not be waited on: {error}", kind.display_name))),
            Err(_) => {
                self.kill().await;
                return Err(Failure::Failed(format!("{} did not answer within ten minutes and was stopped", kind.display_name)));
            }
        };
        let stdout_text = String::from_utf8_lossy(&out);
        let stderr_text = String::from_utf8_lossy(&err);
        if status.success() {
            match parse_output(parse, &stdout_text) {
                Ok(text) if !text.trim().is_empty() => Ok(text),
                _ => Err(unreadable_answer(kind, &stderr_text, &stdout_text)),
            }
        } else {
            Err(run_failure(kind, parse, status.code(), &stderr_text, &stdout_text))
        }
    }
}

fn unreadable_answer(kind: &Kind, stderr: &str, stdout: &str) -> Failure {
    if let Some(classified) = classify(kind, stderr, stdout) {
        return classified;
    }
    if stdout.trim().is_empty() {
        let detail = tail(stderr, 200);
        if detail == "no detail" {
            return Failure::Failed(format!("{} answered nothing this time - try again", kind.display_name));
        }
        return Failure::Failed(format!("{} answered nothing: {detail}", kind.display_name));
    }
    Failure::Failed(format!("{} answered in a shape the studio cannot read ({}). {}", kind.display_name, tail(stdout, 200), update_hint(kind)))
}

fn run_failure(kind: &Kind, parse: ParseMode, code: Option<i32>, stderr: &str, stdout: &str) -> Failure {
    if let Some(classified) = classify(kind, stderr, stdout) {
        return classified;
    }
    if let Err(Some(detail)) = parse_output(parse, stdout) {
        if !detail.trim().is_empty() {
            return Failure::Failed(format!("{} failed: {detail}", kind.display_name));
        }
    }
    let detail = tail(stderr, 300);
    let detail = if detail == "no detail" { tail(stdout, 300) } else { detail };
    Failure::Failed(format!("{} exited with {}: {}", kind.display_name, code.map(|code| code.to_string()).unwrap_or_else(|| "a signal".into()), detail))
}

async fn drain_stdout(pipe: Option<tokio::process::ChildStdout>) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    if let Some(pipe) = pipe {
        pipe.take(OUTPUT_CAP_BYTES).read_to_end(&mut bytes).await.ok();
    }
    bytes
}

async fn drain_stderr(pipe: Option<tokio::process::ChildStderr>) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    if let Some(pipe) = pipe {
        pipe.take(OUTPUT_CAP_BYTES).read_to_end(&mut bytes).await.ok();
    }
    bytes
}

/// Adopt a tokio child into the studio's job.
#[cfg(windows)]
fn adopt_tokio_child(child: &tokio::process::Child) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    let Some(id) = child.id() else { return };
    unsafe {
        let handle = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, id);
        if handle.is_null() {
            return;
        }
        music_core::process::adopt_handle(handle as _);
        CloseHandle(handle);
    }
}

#[cfg(not(windows))]
fn adopt_tokio_child(_child: &tokio::process::Child) {}

/// Last non-empty line, truncated at a word boundary.
fn tail(text: &str, limit: usize) -> String {
    let line = text.lines().filter(|line| !line.trim().is_empty()).last().unwrap_or_default().trim();
    if line.is_empty() {
        return "no detail".to_string();
    }
    if line.chars().count() <= limit {
        return line.to_string();
    }
    let head: String = line.chars().take(limit).collect();
    match head.rfind([' ', '\t']) {
        Some(at) if at > limit / 2 => format!("{}…", head[..at].trim_end()),
        _ => format!("{head}…"),
    }
}

fn update_hint(kind: &Kind) -> String {
    format!("If {} updated recently, its output format may have changed - updating the studio usually catches up.", kind.display_name)
}

/// System contract and request as one prompt.
fn combined_prompt(system: &str, user: &str) -> String {
    format!("{}\n\n{}", system.trim(), user.trim())
}

fn build_invocation(kind: &Kind, resolved: &ResolvedCli, workdir: &Path, system: &str, user: &str, model: Option<&str>) -> Result<Invocation, Failure> {
    let model = model.map(str::trim).filter(|model| !model.is_empty());
    match kind.id {
        "claude" => invocation_claude(kind, workdir, system, user, model),
        "codex" => Ok(invocation_codex(kind, workdir, system, user, model)),
        "opencode" => invocation_opencode(kind, resolved, workdir, system, user, model),
        "antigravity" => invocation_agy(kind, resolved, workdir, system, user, model),
        "muse" => invocation_muse(kind, workdir, system, user, model),
        _ => Err(Failure::Failed(format!("unknown agent CLI: {}", kind.id))),
    }
}

fn push_model(args: &mut Vec<std::ffi::OsString>, kind: &Kind, model: Option<&str>) {
    if let (Some(flag), Some(model)) = (kind.model_flag, model) {
        args.push(flag.into());
        args.push(model.into());
    }
}

fn write_work_file(workdir: &Path, name: &str, content: &str, kind: &Kind) -> Result<PathBuf, Failure> {
    let path = workdir.join(name);
    std::fs::write(&path, content).map_err(|error| Failure::Failed(format!("{} could not stage its prompt: {error}", kind.display_name)))?;
    Ok(path)
}

/// `claude -p` with the prompt on stdin and an empty MCP config.
fn invocation_claude(kind: &Kind, workdir: &Path, system: &str, user: &str, model: Option<&str>) -> Result<Invocation, Failure> {
    let system_file = write_work_file(workdir, "system.txt", system, kind)?;
    let mcp_file = write_work_file(workdir, "mcp.json", "{\"mcpServers\":{}}", kind)?;
    let mut args: Vec<std::ffi::OsString> = [
        "-p",
        "--output-format",
        "json",
        "--input-format",
        "text",
        "--tools",
        "",
        "--strict-mcp-config",
        "--mcp-config",
    ]
    .into_iter()
    .map(std::ffi::OsString::from)
    .collect();
    args.push(mcp_file.into_os_string());
    args.push("--system-prompt-file".into());
    args.push(system_file.into_os_string());
    push_model(&mut args, kind, model);
    Ok(Invocation { args, env: Vec::new(), stdin_text: Some(user.to_string()), parse: ParseMode::Claude })
}

/// `codex exec` with the prompt on stdin (`-`).
fn invocation_codex(kind: &Kind, workdir: &Path, system: &str, user: &str, model: Option<&str>) -> Invocation {
    let mut args: Vec<std::ffi::OsString> =
        ["exec", "--json", "-s", "read-only", "--skip-git-repo-check", "--ephemeral", "-C"].into_iter().map(std::ffi::OsString::from).collect();
    args.push(workdir.as_os_str().to_os_string());
    push_model(&mut args, kind, model);
    args.push("-".into());
    Invocation { args, env: Vec::new(), stdin_text: Some(combined_prompt(system, user)), parse: ParseMode::Codex }
}

/// Neutral opencode config: every built-in tool denied.
const OPENCODE_NEUTRAL_CONFIG: &str = "{\"$schema\":\"https://opencode.ai/config.json\",\"permission\":{\"read\":\"deny\",\"edit\":\"deny\",\"glob\":\"deny\",\"grep\":\"deny\",\"bash\":\"deny\",\"task\":\"deny\",\"skill\":\"deny\",\"lsp\":\"deny\",\"question\":\"deny\",\"webfetch\":\"deny\",\"websearch\":\"deny\"}}";

/// `opencode run` with the prompt as its argument.
fn invocation_opencode(kind: &Kind, resolved: &ResolvedCli, workdir: &Path, system: &str, user: &str, model: Option<&str>) -> Result<Invocation, Failure> {
    let prompt = combined_prompt(system, user);
    argv_prompt(kind, resolved, &prompt)?;
    write_work_file(workdir, "opencode.json", OPENCODE_NEUTRAL_CONFIG, kind)?;
    let mut args: Vec<std::ffi::OsString> = ["run", "--format", "json"].into_iter().map(std::ffi::OsString::from).collect();
    push_model(&mut args, kind, model);
    args.push(prompt.into());
    Ok(Invocation {
        args,
        env: vec![
            ("OPENCODE_CONFIG_CONTENT".into(), OPENCODE_NEUTRAL_CONFIG.into()),
            ("OPENCODE_DISABLE_AUTOUPDATE".into(), "true".into()),
            ("OPENCODE_DISABLE_PRUNE".into(), "true".into()),
            ("OPENCODE_DISABLE_MODELS_FETCH".into(), "true".into()),
        ],
        stdin_text: None,
        parse: ParseMode::Opencode,
    })
}

/// `agy -p` with the prompt as its argument.
fn invocation_agy(kind: &Kind, resolved: &ResolvedCli, workdir: &Path, system: &str, user: &str, model: Option<&str>) -> Result<Invocation, Failure> {
    let prompt = combined_prompt(system, user);
    argv_prompt(kind, resolved, &prompt)?;
    let _ = workdir;
    // Probed on this exact binary: an override may point at another release.
    invocation_agy_with(kind, &probe_features(kind, resolved), prompt, model)
}

fn invocation_agy_with(kind: &Kind, features: &[String], prompt: String, model: Option<&str>) -> Result<Invocation, Failure> {
    // The lyrics travel inside the prompt, and without --sandbox a headless run
    // takes whatever tool permissions the user saved for their own sessions.
    if !features.iter().any(|feature| feature == "sandbox") {
        return Err(Failure::Failed(format!("the installed {} has no --sandbox flag, and the studio runs it only sandboxed: update {}", kind.display_name, kind.command)));
    }
    let mut args: Vec<std::ffi::OsString> = vec!["-p".into(), prompt.into(), "--sandbox".into()];
    let parse = if features.iter().any(|feature| feature == "json-output") {
        args.push("--output-format".into());
        args.push("json".into());
        ParseMode::AgyJson
    } else {
        ParseMode::AgyText
    };
    args.push("--print-timeout".into());
    args.push("10m".into());
    if let Some(model) = model {
        if features.iter().any(|feature| feature == "model-flag") {
            push_model(&mut args, kind, Some(model));
        } else {
            return Err(Failure::Failed(format!("the installed {} has no --model flag: clear the model or update {}", kind.display_name, kind.command)));
        }
    }
    Ok(Invocation { args, env: Vec::new(), stdin_text: None, parse })
}

/// `muse exec` with the prompt in a file.
fn invocation_muse(kind: &Kind, workdir: &Path, system: &str, user: &str, model: Option<&str>) -> Result<Invocation, Failure> {
    let prompt_file = write_work_file(workdir, "prompt.txt", &combined_prompt(system, user), kind)?;
    let mut args: Vec<std::ffi::OsString> = [
        "exec",
        "--json",
        "--prompt-file",
    ]
    .into_iter()
    .map(std::ffi::OsString::from)
    .collect();
    args.push(prompt_file.into_os_string());
    args.extend(
        [
            "--disable-approval",
            "--max-model-steps",
            "5",
            "--disable-web-tools",
            "--disable-shell",
            "--disable-write",
            "--disable-request-user-input",
            "--disable-reminders",
            "--no-session-log",
            "--workspace",
        ]
        .into_iter()
        .map(std::ffi::OsString::from),
    );
    args.push(workdir.as_os_str().to_os_string());
    push_model(&mut args, kind, model);
    Ok(Invocation { args, env: Vec::new(), stdin_text: None, parse: ParseMode::Muse })
}

fn argv_prompt(kind: &Kind, resolved: &ResolvedCli, prompt: &str) -> Result<(), Failure> {
    if resolved.via_shell {
        return Err(Failure::Failed(format!(
            "{} runs through a script shim the studio could not resolve, and this prompt cannot cross a shell intact: point the CLI path at the real {} binary",
            kind.display_name, kind.command
        )));
    }
    let chars = prompt.chars().count();
    if chars > MAX_ARGV_PROMPT_CHARS {
        return Err(Failure::Failed(format!(
            "{} takes the prompt as an argument, and this one is {chars} characters, past what a command line carries: shorten the lyrics or pick another provider",
            kind.display_name
        )));
    }
    Ok(())
}

fn quote_for_cmd(arg: &std::ffi::OsString) -> std::ffi::OsString {
    let text = arg.to_string_lossy();
    if !text.chars().any(|char| matches!(char, ' ' | '\t' | '"' | '&' | '|' | '<' | '>' | '^' | '%' | '!')) {
        return arg.clone();
    }
    std::ffi::OsString::from(format!("\"{}\"", text.replace('"', "\"\"")))
}

/// The program with its arguments. A script shim runs as `cmd /d /s /c "<shim> <args>"` in one raw argument: with
/// `/s` cmd takes off only the outer pair of quotes, so the shim path and every quoted argument reach the script as
/// written, where Rust's own quoting would escape those quotes a second time.
fn command_line(resolved: &ResolvedCli, args: &[std::ffi::OsString]) -> std::process::Command {
    let mut command = std::process::Command::new(&resolved.program);
    #[cfg(windows)]
    if resolved.via_shell {
        use std::os::windows::process::CommandExt;
        let shim = resolved.prefix_args.last().map(|shim| format!("\"{}\"", shim.to_string_lossy())).unwrap_or_default();
        let quoted: Vec<String> = args.iter().map(|arg| quote_for_cmd(arg).to_string_lossy().into_owned()).collect();
        command.raw_arg(format!("/d /s /c \"{shim} {}\"", quoted.join(" ")));
        return command;
    }
    command.args(&resolved.prefix_args).args(args);
    command
}

/// `Err(Some(_))` is quotable CLI error text; `Err(None)` is useless output.
fn parse_output(parse: ParseMode, stdout: &str) -> Result<String, Option<String>> {
    match parse {
        ParseMode::Claude => parse_claude(stdout),
        ParseMode::Codex => parse_codex(stdout),
        ParseMode::Opencode => parse_opencode(stdout),
        ParseMode::AgyText => parse_agy_text(stdout),
        ParseMode::AgyJson => parse_agy_json(stdout),
        ParseMode::Muse => parse_muse(stdout),
    }
}

/// Claude's JSON result object; anything but success is a failure.
fn parse_claude(stdout: &str) -> Result<String, Option<String>> {
    let trimmed = stdout.trim();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Err(None);
    };
    let subtype = value.get("subtype").and_then(serde_json::Value::as_str).unwrap_or_default();
    let is_error = value.get("is_error").and_then(serde_json::Value::as_bool).unwrap_or(false);
    if subtype != "success" || is_error {
        let detail = value.get("result").and_then(serde_json::Value::as_str).filter(|text| !text.trim().is_empty()).map(str::to_string).unwrap_or_else(|| {
            value.get("error").and_then(|error| error.as_str().or_else(|| error.get("message")?.as_str())).unwrap_or(subtype).to_string()
        });
        return Err(Some(if detail.is_empty() { format!("claude failed ({subtype})") } else { detail }));
    }
    value.get("result").and_then(serde_json::Value::as_str).map(str::to_string).filter(|text| !text.trim().is_empty()).ok_or(None)
}

/// codex's JSONL: the last `agent_message`, unless the turn failed.
fn parse_codex(stdout: &str) -> Result<String, Option<String>> {
    let mut text: Option<String> = None;
    let mut failed: Option<String> = None;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        match event.get("type").and_then(serde_json::Value::as_str) {
            Some("item.completed") => {
                let item = event.get("item");
                if item.and_then(|item| item.get("type")).and_then(serde_json::Value::as_str) == Some("agent_message") {
                    if let Some(answer) = item.and_then(|item| item.get("text")).and_then(serde_json::Value::as_str).filter(|answer| !answer.trim().is_empty()) {
                        text = Some(answer.to_string());
                    }
                }
            }
            Some("turn.failed") => {
                failed = Some(message_of(&event).unwrap_or_else(|| "the turn failed".to_string()));
            }
            Some("error") => {
                if text.is_none() {
                    failed = Some(message_of(&event).unwrap_or_else(|| "codex errored".to_string()));
                }
            }
            _ => {}
        }
    }
    if let Some(failure) = failed {
        return Err(Some(failure));
    }
    text.ok_or(None)
}

/// opencode's JSON events: the last `text` part wins.
fn parse_opencode(stdout: &str) -> Result<String, Option<String>> {
    let mut text: Option<String> = None;
    let mut failed: Option<String> = None;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        match event.get("type").and_then(serde_json::Value::as_str) {
            Some("text") => {
                if let Some(answer) = event.get("part").and_then(|part| part.get("text")).and_then(serde_json::Value::as_str).filter(|answer| !answer.trim().is_empty())
                {
                    text = Some(answer.to_string());
                }
            }
            Some("message.part.updated") => {
                let part = event.get("part");
                if part.and_then(|part| part.get("type")).and_then(serde_json::Value::as_str) == Some("text") {
                    if let Some(answer) = part.and_then(|part| part.get("text")).and_then(serde_json::Value::as_str).filter(|answer| !answer.trim().is_empty()) {
                        text = Some(answer.to_string());
                    }
                }
            }
            Some("error") => {
                if text.is_none() {
                    failed = Some(message_of(&event).unwrap_or_else(|| "opencode errored".to_string()));
                }
            }
            _ => {}
        }
    }
    if let Some(answer) = text {
        return Ok(answer);
    }
    Err(failed)
}

/// Old `agy`: whatever stdout holds is the answer.
fn parse_agy_text(stdout: &str) -> Result<String, Option<String>> {
    let text = stdout.trim();
    if text.is_empty() { Err(None) } else { Ok(text.to_string()) }
}

/// New `agy`: one JSON envelope with the answer at `.response`.
fn parse_agy_json(stdout: &str) -> Result<String, Option<String>> {
    let trimmed = stdout.trim();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Err(None);
    };
    if let Some(status) = value.get("status").and_then(serde_json::Value::as_str) {
        if !matches!(status.to_ascii_lowercase().as_str(), "success" | "ok" | "completed") {
            return Err(Some(message_of(&value).unwrap_or_else(|| format!("agy failed ({status})"))));
        }
    }
    value.get("response").and_then(serde_json::Value::as_str).map(str::to_string).filter(|text| !text.trim().is_empty()).ok_or(None)
}

/// muse's JSONL: the terminal event wins, deltas are the fallback.
fn parse_muse(stdout: &str) -> Result<String, Option<String>> {
    let mut deltas = String::new();
    let mut terminal: Option<(String, String)> = None;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let payload = event.get("payload");
        match event.get("payload_type").and_then(serde_json::Value::as_str) {
            Some("run.output.delta") => {
                if let Some(fragment) = payload.and_then(|payload| payload.get("text")).and_then(serde_json::Value::as_str) {
                    deltas.push_str(fragment);
                }
            }
            Some(kind) if kind.starts_with("run.terminal.") => {
                let status = payload.and_then(|payload| payload.get("terminal")).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
                let text = payload.and_then(|payload| payload.get("text")).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
                let reason = payload.and_then(|payload| payload.get("reason")).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
                terminal = Some((status, if text.trim().is_empty() { reason } else { text }));
            }
            _ => {}
        }
    }
    if let Some((status, text)) = terminal {
        if status.eq_ignore_ascii_case("completed") {
            if text.trim().is_empty() {
                return Err(None);
            }
            return Ok(text);
        }
        return Err(Some(if text.trim().is_empty() { format!("muse ended {status}") } else { text }));
    }
    if deltas.trim().is_empty() { Err(None) } else { Ok(deltas) }
}

/// Human-readable message out of an error event.
fn message_of(event: &serde_json::Value) -> Option<String> {
    let message = event
        .get("message")
        .and_then(serde_json::Value::as_str)
        .or_else(|| event.get("error")?.as_str())
        .or_else(|| event.get("error")?.get("message")?.as_str())
        .or_else(|| event.get("reason")?.as_str())?;
    let unwrapped = unwrap_envelope(message);
    let trimmed = unwrapped.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn unwrap_envelope(message: &str) -> String {
    let trimmed = message.trim();
    if !trimmed.starts_with('{') {
        return message.to_string();
    }
    let Ok(envelope) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return message.to_string();
    };
    envelope
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| envelope.get("message")?.as_str())
        .unwrap_or(message)
        .to_string()
}

fn failure_patterns() -> &'static FailurePatterns {
    static PATTERNS: std::sync::OnceLock<FailurePatterns> = std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| {
        let compile = |pattern: &str| regex::Regex::new(pattern).expect("a failure pattern that compiles");
        FailurePatterns {
            // Checked first: "quota" shows up in rate-limit prose too.
            throttled: compile("(?i)rate.?limit|too many requests|\\b429\\b|overloaded|over capacity|capacity exceeded|try again (in|later|after)|temporarily unavailable|throttl"),
            no_credit: compile("(?i)insufficient.{0,30}(credit|quota|balance|fund)|out of (credit|quota)|credit balance|billing|payment required|\\b402\\b|quota.{0,20}exceed|exceed.{0,20}quota|usage limit|plan limit|upgrade your plan|purchase (more )?credit|credits exhausted"),
            not_signed_in: compile("(?i)not (signed|logged) in|authentication required|not authenticated|login required|please (log|sign) in|invalid (api[-_ ]?key|x-api-key)|incorrect api key|\\bunauthorized\\b|\\b401\\b|authenticat(e|ion) (failed|error)|no (valid )?credentials|oauth.{0,20}expir|token.{0,20}expir|expired token|claude auth login|codex login|opencode auth login"),
        }
    })
}

struct FailurePatterns {
    throttled: regex::Regex,
    no_credit: regex::Regex,
    not_signed_in: regex::Regex,
}

fn classify(kind: &Kind, stderr: &str, stdout: &str) -> Option<Failure> {
    let stdout_tail: String = stdout.lines().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    let haystack = format!("{stderr}\n{stdout_tail}");
    let detail = tail(stderr, 200);
    let patterns = failure_patterns();
    if patterns.throttled.is_match(&haystack) {
        return Some(Failure::Throttled(format!("{} is rate limited ({detail}): wait a minute and try again", kind.display_name)));
    }
    if patterns.no_credit.is_match(&haystack) {
        return Some(Failure::NoCredit(format!("{} refused for lack of credit or quota ({detail}): top up or wait for the reset, then try again", kind.display_name)));
    }
    if patterns.not_signed_in.is_match(&haystack) {
        return Some(Failure::NotSignedIn(format!("{} is not signed in: {}", kind.display_name, kind.signin)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(id: &str) -> &'static Kind {
        find_kind(id).expect("a known kind")
    }

    fn direct(program: &str) -> ResolvedCli {
        ResolvedCli { program: PathBuf::from(program), prefix_args: Vec::new(), via_shell: false }
    }

    fn shelled(program: &str) -> ResolvedCli {
        ResolvedCli { program: PathBuf::from("cmd.exe"), prefix_args: vec!["/d".into(), "/c".into(), program.into()], via_shell: true }
    }

    #[test]
    fn the_registry_holds_five_unique_kinds() {
        assert_eq!(KINDS.len(), 5);
        let mut ids = KINDS.iter().map(|kind| kind.id).collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 5);
        let mut commands = KINDS.iter().map(|kind| kind.command).collect::<Vec<_>>();
        commands.sort();
        commands.dedup();
        assert_eq!(commands.len(), 5);
        assert!(find_kind("nope").is_none());
    }

    #[test]
    fn an_unknown_cli_fails_with_its_install_hint() {
        let pretend = Kind {
            id: "definitely-not-a-cli-xyz",
            display_name: "Pretend",
            command: "definitely-not-a-cli-xyz",
            model_flag: None,
            suggested_models: &[],
            install: "install pretend",
            signin: "sign into pretend",
            docs: None,
        };
        let error = resolve(&pretend, None).expect_err("nothing by that name exists");
        assert!(error.contains("install pretend"), "{error}");
    }

    #[test]
    fn a_missing_override_path_says_so() {
        let error = resolve(kind("claude"), Some("C:/definitely/not/a/cli.exe")).expect_err("the file is not there");
        assert!(error.contains("not a file"), "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn lookup_follows_extension_order_inside_one_folder() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("thing.cmd"), "@echo off").unwrap();
        std::fs::write(folder.path().join("thing.exe"), "MZ").unwrap();
        let dirs = vec![folder.path().to_path_buf()];
        let extensions: Vec<String> = [".exe", ".cmd"].iter().map(|extension| extension.to_string()).collect();
        assert_eq!(find_in_dirs_with_extensions("thing", &dirs, &extensions).unwrap(), folder.path().join("thing.exe"));
        let extensions: Vec<String> = [".cmd", ".exe"].iter().map(|extension| extension.to_string()).collect();
        assert_eq!(find_in_dirs_with_extensions("thing", &dirs, &extensions).unwrap(), folder.path().join("thing.cmd"));
        assert_eq!(find_in_dirs_with_extensions("thing.cmd", &dirs, &extensions).unwrap(), folder.path().join("thing.cmd"));
        assert!(find_in_dirs_with_extensions("absent", &dirs, &extensions).is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn lookup_matches_the_exact_name() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("thing"), "#!/bin/sh").unwrap();
        let dirs = vec![folder.path().to_path_buf()];
        assert_eq!(find_in_dirs("thing", &dirs).unwrap(), folder.path().join("thing"));
        assert!(find_in_dirs("absent", &dirs).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn percent_vars_expand_and_unknowns_stay() {
        unsafe { std::env::set_var("YUE2_CLI_TEST_MARKER", "marked") };
        assert_eq!(expand_percent_vars("%YUE2_CLI_TEST_MARKER%\\bin"), "marked\\bin");
        assert_eq!(expand_percent_vars("%YUE2_CLI_TEST_NOPE%\\bin"), "%YUE2_CLI_TEST_NOPE%\\bin");
        assert_eq!(expand_percent_vars("100%"), "100%");
        unsafe { std::env::remove_var("YUE2_CLI_TEST_MARKER") };
    }

    #[test]
    fn npm_shims_resolve_to_node_and_script() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("node.exe"), "MZ").unwrap();
        let modern = folder.path().join("tool.cmd");
        std::fs::write(&modern, "@ECHO off\r\n\"%dp0%\\node.exe\"  \"%dp0%\\node_modules\\tool\\bin.js\"   %*\r\n").unwrap();
        let (node, script) = npm_shim_target(&modern).expect("the modern npm shape");
        assert_eq!(node, folder.path().join("node.exe"));
        assert!(script.to_string_lossy().ends_with("bin.js"), "{}", script.display());
        let old = folder.path().join("old.cmd");
        std::fs::write(&old, "@ECHO off\r\n\"%~dp0\\node.exe\" \"%~dp0\\old.js\" %*\r\n").unwrap();
        assert!(npm_shim_target(&old).is_some());
        let foreign = folder.path().join("foreign.cmd");
        std::fs::write(&foreign, "@echo off\r\npowershell.exe -File \"%~dp0launcher.ps1\" %*\r\n").unwrap();
        assert!(npm_shim_target(&foreign).is_none());
    }

    #[test]
    fn muse_shim_prefers_the_version_file_then_the_newest_binary() {
        let folder = tempfile::tempdir().unwrap();
        let shim = folder.path().join("muse.cmd");
        std::fs::write(&shim, "@echo off").unwrap();
        std::fs::write(folder.path().join("muse-bin-1.4.9-R1.exe"), "MZ").unwrap();
        std::fs::write(folder.path().join("muse-bin-1.4.10-R2.exe"), "MZ").unwrap();
        assert_eq!(muse_shim_binary(&shim).unwrap(), folder.path().join("muse-bin-1.4.10-R2.exe"));
        std::fs::write(folder.path().join(".muse-version"), "1.4.9-R1").unwrap();
        assert_eq!(muse_shim_binary(&shim).unwrap(), folder.path().join("muse-bin-1.4.9-R1.exe"));
    }

    #[test]
    fn resolve_shim_routes_binaries_scripts_and_muse() {
        let folder = tempfile::tempdir().unwrap();
        let binary = folder.path().join("claude.exe");
        std::fs::write(&binary, "MZ").unwrap();
        let resolved = resolve_shim(kind("claude"), &binary);
        assert!(!resolved.via_shell);
        assert!(resolved.prefix_args.is_empty());

        let shim = folder.path().join("muse.cmd");
        std::fs::write(&shim, "@echo off").unwrap();
        std::fs::write(folder.path().join("muse-bin-9.9.9-R1.exe"), "MZ").unwrap();
        let resolved = resolve_shim(kind("muse"), &shim);
        assert!(!resolved.via_shell);
        assert_eq!(resolved.program, folder.path().join("muse-bin-9.9.9-R1.exe"));

        let foreign_folder = tempfile::tempdir().unwrap();
        let foreign = foreign_folder.path().join("foreign.cmd");
        std::fs::write(&foreign, "@echo off\r\npowershell.exe -File x %*\r\n").unwrap();
        let resolved = resolve_shim(kind("muse"), &foreign);
        assert!(resolved.via_shell);
        assert_eq!(resolved.prefix_args.len(), 3);
    }

    fn invocation(id: &str, resolved: &ResolvedCli, model: Option<&str>) -> Invocation {
        let workdir = tempfile::tempdir().unwrap();
        build_invocation(kind(id), resolved, workdir.path(), "system contract", "user request", model).expect("a buildable invocation")
    }

    #[test]
    fn claude_runs_headless_without_tools_or_mcp() {
        let workdir = tempfile::tempdir().unwrap();
        let built = build_invocation(kind("claude"), &direct("claude"), workdir.path(), "system contract", "user request", Some("opus")).unwrap();
        let args: Vec<String> = built.args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(&args[0..9], &["-p", "--output-format", "json", "--input-format", "text", "--tools", "", "--strict-mcp-config", "--mcp-config"]);
        assert!(args.contains(&"--model".to_string()) && args.contains(&"opus".to_string()));
        assert!(args.iter().any(|arg| arg.ends_with("system.txt")), "{args:?}");
        assert_eq!(built.stdin_text.as_deref(), Some("user request"));
        assert_eq!(built.parse, ParseMode::Claude);
        assert!(workdir.path().join("system.txt").is_file());
        assert!(workdir.path().join("mcp.json").is_file());
    }

    #[test]
    fn codex_runs_sandboxed_with_stdin() {
        let workdir = tempfile::tempdir().unwrap();
        let built = build_invocation(kind("codex"), &direct("codex"), workdir.path(), "system contract", "user request", None).unwrap();
        let args: Vec<String> = built.args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(args.starts_with(&["exec".to_string(), "--json".to_string()]));
        for flag in ["-s", "read-only", "--skip-git-repo-check", "--ephemeral", "-C"] {
            assert!(args.contains(&flag.to_string()), "{args:?}");
        }
        assert_eq!(args.last().unwrap(), "-");
        assert!(!args.contains(&"--model".to_string()));
        assert_eq!(built.stdin_text.as_deref(), Some("system contract\n\nuser request"));
        assert_eq!(built.parse, ParseMode::Codex);
    }

    #[test]
    fn opencode_denies_every_tool_two_ways() {
        let workdir = tempfile::tempdir().unwrap();
        let built = build_invocation(kind("opencode"), &direct("opencode"), workdir.path(), "system", "user", Some("anthropic/claude-opus-4-6")).unwrap();
        let args: Vec<String> = built.args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(&args[0..3], &["run", "--format", "json"]);
        assert!(!args.contains(&"--dir".to_string()));
        assert!(args.contains(&"anthropic/claude-opus-4-6".to_string()));
        assert_eq!(args.last().unwrap(), "system\n\nuser");
        assert!(built.stdin_text.is_none());
        assert!(built.env.iter().any(|(name, value)| name == "OPENCODE_CONFIG_CONTENT" && value.contains("\"bash\":\"deny\"")));
        let staged = std::fs::read_to_string(workdir.path().join("opencode.json")).unwrap();
        assert!(staged.contains("\"edit\":\"deny\""), "{staged}");
    }

    #[test]
    fn old_agy_runs_positional_text_without_model() {
        let sandbox = vec!["sandbox".to_string()];
        let built = invocation_agy_with(kind("antigravity"), &sandbox, "system contract\n\nuser request".into(), None).unwrap();
        let args: Vec<String> = built.args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(&args[0..3], &["-p", "system contract\n\nuser request", "--sandbox"]);
        assert!(!args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"--print-timeout".to_string()));
        assert_eq!(built.parse, ParseMode::AgyText);
        assert!(built.stdin_text.is_none());
        let refused = invocation_agy_with(kind("antigravity"), &sandbox, "prompt".into(), Some("gemini-3-pro"));
        assert!(matches!(refused, Err(Failure::Failed(ref message)) if message.contains("no --model flag")), "{refused:?}");
    }

    #[test]
    fn agy_without_a_sandbox_is_refused() {
        let refused = invocation_agy_with(kind("antigravity"), &[], "prompt".into(), None);
        assert!(matches!(refused, Err(Failure::Failed(ref message)) if message.contains("--sandbox")), "{refused:?}");
        let missing = direct("C:/definitely/not/agy.exe");
        let workdir = tempfile::tempdir().unwrap();
        let refused = build_invocation(kind("antigravity"), &missing, workdir.path(), "system", "user", None);
        assert!(matches!(refused, Err(Failure::Failed(ref message)) if message.contains("--sandbox")), "{refused:?}");
    }

    #[test]
    fn muse_runs_from_a_prompt_file_with_no_tools() {
        let workdir = tempfile::tempdir().unwrap();
        let built = build_invocation(kind("muse"), &direct("muse"), workdir.path(), "system contract", "user request", None).unwrap();
        let args: Vec<String> = built.args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(&args[0..3], &["exec", "--json", "--prompt-file"]);
        for flag in ["--disable-approval", "--max-model-steps", "--disable-web-tools", "--disable-shell", "--disable-write", "--no-session-log", "--workspace"] {
            assert!(args.contains(&flag.to_string()), "{args:?}");
        }
        assert!(built.stdin_text.is_none());
        assert_eq!(built.parse, ParseMode::Muse);
        let staged = std::fs::read_to_string(workdir.path().join("prompt.txt")).unwrap();
        assert_eq!(staged, "system contract\n\nuser request");
    }

    #[test]
    fn long_and_shelled_prompts_are_refused_not_truncated() {
        let workdir = tempfile::tempdir().unwrap();
        let long = "word ".repeat(MAX_ARGV_PROMPT_CHARS);
        let refused = build_invocation(kind("opencode"), &direct("opencode"), workdir.path(), &long, "user", None);
        assert!(matches!(refused, Err(Failure::Failed(message)) if message.contains("past what a command line carries")));
        let refused = build_invocation(kind("antigravity"), &shelled("agy.cmd"), workdir.path(), "system", "user", None);
        assert!(matches!(refused, Err(Failure::Failed(message)) if message.contains("cannot cross a shell")));
    }

    #[test]
    fn cmd_quoting_wraps_only_what_needs_it() {
        assert_eq!(quote_for_cmd(&"exec".into()), std::ffi::OsString::from("exec"));
        assert_eq!(quote_for_cmd(&"C:\\no spaces\\x.exe".into()), std::ffi::OsString::from("\"C:\\no spaces\\x.exe\""));
        assert_eq!(quote_for_cmd(&"a&b".into()), std::ffi::OsString::from("\"a&b\""));
        assert_eq!(quote_for_cmd(&"100%".into()), std::ffi::OsString::from("\"100%\""));
    }

    #[cfg(windows)]
    #[test]
    fn a_script_shim_gets_its_quoted_arguments_whole() {
        let folder = tempfile::Builder::new().prefix("cli shim & co ").tempdir().unwrap();
        let shim = folder.path().join("agent.cmd");
        std::fs::write(&shim, "@echo off\r\n>\"%~dp0args.txt\" (echo %1& echo %2& echo %3)\r\n").unwrap();
        let resolved = ResolvedCli { program: PathBuf::from("cmd.exe"), prefix_args: vec!["/d".into(), "/c".into(), shim.into_os_string()], via_shell: true };
        let file = folder.path().join("system prompt.txt");
        let status = command_line(&resolved, &["--system-prompt-file".into(), file.clone().into_os_string(), "100%".into()]).status().unwrap();
        assert!(status.success());
        let seen = std::fs::read_to_string(folder.path().join("args.txt")).unwrap();
        assert_eq!(seen.lines().map(|line| line.trim_matches('"')).collect::<Vec<_>>(), ["--system-prompt-file", file.to_str().unwrap(), "100%"]);
        let probed = run_limited(&resolved, &["--version".into()], PROBE_LIMIT).unwrap();
        assert!(probed.status.success());
        let probed_args = String::from_utf8_lossy(&std::fs::read(folder.path().join("args.txt")).unwrap()).into_owned();
        assert_eq!(probed_args.lines().next(), Some("--version"), "detection reaches the shim too");
    }

    #[test]
    fn claude_parse_accepts_success_and_reports_the_rest() {
        let ok = parse_claude("{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"the draft\",\"total_cost_usd\":0.01,\"num_turns\":1}").unwrap();
        assert_eq!(ok, "the draft");
        let failed = parse_claude("{\"type\":\"result\",\"subtype\":\"error_max_turns\",\"is_error\":true,\"result\":\"partial\"}").expect_err("a cut answer is not a draft");
        assert_eq!(failed.as_deref(), Some("partial"));
        assert!(parse_claude("not json").is_err());
        assert!(parse_claude("{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"  \"}").is_err());
    }

    #[test]
    fn codex_parse_takes_the_last_agent_message_and_honours_failed_turns() {
        let stream = concat!(
            "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n",
            "{\"type\":\"item.completed\",\"item\":{\"id\":\"i0\",\"type\":\"error\",\"message\":\"hooks warning\"}}\n",
            "{\"type\":\"item.completed\",\"item\":{\"id\":\"i1\",\"type\":\"agent_message\",\"text\":\"first\"}}\n",
            "{\"type\":\"item.completed\",\"item\":{\"id\":\"i2\",\"type\":\"command_execution\",\"command\":\"ls\"}}\n",
            "{\"type\":\"item.completed\",\"item\":{\"id\":\"i3\",\"type\":\"agent_message\",\"text\":\"final\"}}\n",
            "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}\n",
        );
        assert_eq!(parse_codex(stream).unwrap(), "final");
        let failed = concat!(
            "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n",
            "{\"type\":\"item.completed\",\"item\":{\"id\":\"i1\",\"type\":\"agent_message\",\"text\":\"partial\"}}\n",
            "{\"type\":\"turn.failed\",\"error\":{\"message\":\"boom\"}}\n",
        );
        assert_eq!(parse_codex(failed).unwrap_err().as_deref(), Some("boom"));
        assert!(parse_codex("{\"type\":\"turn.completed\"}\n").is_err());
    }

    #[test]
    fn opencode_parse_reads_text_parts_and_server_events() {
        let stream = concat!(
            "{\"type\":\"message.updated\",\"properties\":{}}\n",
            "{\"type\":\"text\",\"part\":{\"id\":\"p1\",\"type\":\"text\",\"text\":\"half\"}}\n",
            "{\"type\":\"text\",\"part\":{\"id\":\"p1\",\"type\":\"text\",\"text\":\"half and whole\",\"time\":{\"end\":1}}}\n",
        );
        assert_eq!(parse_opencode(stream).unwrap(), "half and whole");
        let legacy = "{\"type\":\"message.part.updated\",\"part\":{\"id\":\"p1\",\"type\":\"text\",\"text\":\"server shaped\"}}\n";
        assert_eq!(parse_opencode(legacy).unwrap(), "server shaped");
        let failed = "{\"type\":\"error\",\"error\":{\"message\":\"no auth\"}}\n";
        assert_eq!(parse_opencode(failed).unwrap_err().as_deref(), Some("no auth"));
    }

    #[test]
    fn agy_parse_reads_envelopes_and_bare_text() {
        assert_eq!(parse_agy_json("{\"response\":\"the draft\\n\",\"duration_seconds\":1}").unwrap(), "the draft\n");
        assert!(parse_agy_json("{\"response\":\"\"}").is_err());
        assert_eq!(parse_agy_text("the draft\n").unwrap(), "the draft");
        assert!(parse_agy_text("  \n ").is_err());
    }

    #[test]
    fn muse_parse_prefers_the_terminal_event() {
        let stream = concat!(
            "{\"payload_type\":\"run.output.delta\",\"payload\":{\"kind\":\"run_output_delta\",\"text\":\"echo: \"}}\n",
            "{\"payload_type\":\"run.output.delta\",\"payload\":{\"kind\":\"run_output_delta\",\"text\":\"say hi\"}}\n",
            "{\"payload_type\":\"run.terminal.completed\",\"payload\":{\"kind\":\"run_terminal\",\"terminal\":\"completed\",\"text\":\"echo: say hi\",\"reason\":null}}\n",
        );
        assert_eq!(parse_muse(stream).unwrap(), "echo: say hi");
        let cut = "{\"payload_type\":\"run.output.delta\",\"payload\":{\"kind\":\"run_output_delta\",\"text\":\"partial\"}}\n";
        assert_eq!(parse_muse(cut).unwrap(), "partial");
        let failed = "{\"payload_type\":\"run.terminal.failed\",\"payload\":{\"kind\":\"run_terminal\",\"terminal\":\"failed\",\"text\":\"\",\"reason\":\"no model\"}}\n";
        assert_eq!(parse_muse(failed).unwrap_err().as_deref(), Some("no model"));
        assert!(parse_muse("{\"payload_type\":\"task.lifecycle.accepted\",\"payload\":{}}\n").is_err());
    }

    #[test]
    fn a_failed_codex_run_quotes_its_own_stdout_reason() {
        let stream = concat!(
            "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n",
            "{\"type\":\"item.completed\",\"item\":{\"id\":\"i0\",\"type\":\"error\",\"message\":\"Model metadata for `gpt-6.1-sol` not found.\"}}\n",
            "{\"type\":\"turn.started\"}\n",
            "{\"type\":\"error\",\"message\":\"{\\\"type\\\":\\\"error\\\",\\\"status\\\":400,\\\"error\\\":{\\\"type\\\":\\\"invalid_request_error\\\",\\\"message\\\":\\\"The 'gpt-6.1-sol' model is not supported.\\\"}}\"}\n",
            "{\"type\":\"turn.failed\",\"error\":{\"message\":\"{\\\"type\\\":\\\"error\\\",\\\"status\\\":400,\\\"error\\\":{\\\"type\\\":\\\"invalid_request_error\\\",\\\"message\\\":\\\"The 'gpt-6.1-sol' model is not supported.\\\"}}\"}}\n",
        );
        let failure = run_failure(kind("codex"), ParseMode::Codex, Some(1), "", stream);
        assert_eq!(failure.message(), "Codex CLI failed: The 'gpt-6.1-sol' model is not supported.");
    }

    #[test]
    fn a_bare_exit_quotes_whichever_stream_spoke_last() {
        let failure = run_failure(kind("muse"), ParseMode::Muse, Some(1), "muse: explosive decompression\n", "partial\n");
        assert_eq!(failure.message(), "Muse Code exited with 1: muse: explosive decompression");
        let failure = run_failure(kind("muse"), ParseMode::Muse, Some(1), "", "only stdout spoke\n");
        assert_eq!(failure.message(), "Muse Code exited with 1: only stdout spoke");
    }

    #[test]
    fn an_empty_answer_invites_a_retry_not_an_update() {
        let failure = unreadable_answer(kind("antigravity"), "", "  \n ");
        assert_eq!(failure.message(), "Antigravity (agy) answered nothing this time - try again");
        let failure = unreadable_answer(kind("antigravity"), "agy: hiccup on channel 4", "  ");
        assert_eq!(failure.message(), "Antigravity (agy) answered nothing: agy: hiccup on channel 4");
        let failure = unreadable_answer(kind("claude"), "", "definitely not json");
        assert!(failure.message().contains("cannot read"), "{}", failure.message());
    }

    #[cfg(windows)]
    #[test]
    fn the_system_temp_ignores_a_redirected_environment() {
        let profile = tempfile::tempdir().unwrap();
        let temp = system_temp_dir_from(Some(profile.path().as_os_str().to_os_string()));
        assert_eq!(temp, profile.path().join("Temp"));
        assert!(temp.is_dir());
        let fallback = system_temp_dir_from(None);
        assert_eq!(fallback, std::env::temp_dir());
    }

    #[test]
    fn spawned_clis_agree_on_one_temp() {
        let env = sanitized_temp_env(Path::new("C:/Users/someone/AppData/Local/Temp"));
        assert_eq!(env.len(), 3);
        for (name, value) in &env {
            assert!(["TMPDIR", "TMP", "TEMP"].contains(&name.as_str()));
            assert_eq!(value, "C:/Users/someone/AppData/Local/Temp");
        }
    }

    #[test]
    fn long_quotes_break_at_words() {
        assert_eq!(tail("short", 200), "short");
        assert_eq!(tail("  \n ", 200), "no detail");
        assert_eq!(tail("runtime host failed to start inside the workspace", 20), "runtime host failed…");
    }

    #[test]
    fn failures_classify_and_carry_their_signin_hint() {
        let claude = kind("claude");
        let not_signed_in = classify(claude, "Error: Not authenticated. Run claude auth login.", "").expect("classified");
        assert!(matches!(not_signed_in, Failure::NotSignedIn(_)));
        assert!(not_signed_in.message().contains("claude auth login"));
        assert!(not_signed_in.message().ends_with("[cli_not_signed_in]"));

        let no_credit = classify(kind("codex"), "insufficient quota: billing hard limit", "").expect("classified");
        assert!(matches!(no_credit, Failure::NoCredit(_)));
        assert!(no_credit.message().ends_with("[cli_no_credit]"));

        let throttled = classify(kind("opencode"), "HTTP 429: rate limit quota reached, try again later", "").expect("classified");
        assert!(matches!(throttled, Failure::Throttled(_)));

        let stdout_only = classify(kind("antigravity"), "", "line one\nline two\nauthentication required\n").expect("stdout counts");
        assert!(matches!(stdout_only, Failure::NotSignedIn(_)));

        assert!(classify(claude, "panic: broken pipe", "").is_none());
        assert!(matches!(Failure::Failed("plain".into()).message().as_str(), "plain"));
    }

    #[test]
    fn detection_only_reports_known_kinds() {
        for detected in detect() {
            assert!(find_kind(detected.id).is_some());
            assert!(!detected.path.is_empty());
            assert!(!detected.version.is_empty());
        }
        assert_eq!(list().len(), KINDS.len());
    }
}
