//! Diagnostics (Speech Studio's `app/diagnostics.rs`, as MiniMax Music3 Studio carries it): the version, the system,
//! the graphics card against the CUDA builds, the music models and the optional parts, what the engine's DLLs are
//! missing, and the latest log lines - as text to copy, and as a bug-report pack (`<root>/reports/*.zip`: the
//! diagnostics, the journal and the logs). Keys never get in: anything in a log that looks like one is masked.

use std::io::Write;
use std::path::{Path, PathBuf};

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::studio_log::masked;
use crate::{api_error, ApiError, AppState};

/// How many of the studio log's latest lines the diagnostics carry.
const LOG_LINES: usize = 400;

/// The studio's log files in `<root>/logs`, the current one last.
fn log_files(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join("logs")) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "log")).collect();
    files.sort_by_key(|path| (!path.to_string_lossy().ends_with(".old.log"), path.clone()));
    files
}

/// The latest lines of the current log, oldest first.
fn recent_log(root: &Path) -> Vec<String> {
    let Some(current) = log_files(root).into_iter().rfind(|path| !path.to_string_lossy().ends_with(".old.log")) else { return Vec::new() };
    let Ok(bytes) = std::fs::read(&current) else { return Vec::new() };
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(LOG_LINES)..].iter().map(|line| masked(line)).collect()
}

/// A value of a part's status by its path (`set.ready`), null where the status has none.
fn field(status: &Value, path: &str) -> Value {
    path.split('.').try_fold(status, |value, key| value.get(key)).cloned().unwrap_or(Value::Null)
}

/// What the engine and the backends it loads miss, by the Windows loader's rules: the engine folder, then where the
/// studio keeps the engine's cuBLAS. Only this card's CUDA build is checked; the other is never loaded here.
fn dll_missing(state: &AppState) -> Value {
    if !cfg!(windows) {
        return json!([]);
    }
    let engine = crate::engine_bundle_root();
    let executable = music_engine::server::executable_name();
    let mut missing = Vec::new();
    if !engine.join(&executable).is_file() {
        missing.push(format!("{}: no {executable} in the engine folder", engine.display()));
        return json!(missing);
    }
    let search = [engine.clone(), state.engine_runtime.library_dir()];
    let mut entries = vec![engine.join(&executable)];
    if let Some(build) = crate::cuda_build::current() {
        entries.push(engine.join(build.folder()).join("ggml-cuda.dll"));
    }
    if let Ok(files) = std::fs::read_dir(&engine) {
        for file in files.flatten().map(|entry| entry.path()) {
            let name = file.file_name().map(|name| name.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if name.starts_with("ggml-") && name.ends_with(".dll") {
                entries.push(file);
            }
        }
    }
    for entry in entries.into_iter().filter(|entry| entry.is_file()) {
        let name = entry.strip_prefix(&engine).unwrap_or(&entry).display().to_string();
        match crate::engine_runtime::unresolved_in(&search, &entry) {
            Ok(absent) => missing.extend(absent.into_iter().map(|library| format!("{name}: {library}"))),
            Err(error) => missing.push(format!("{name}: the import table was not read: {error:#}")),
        }
    }
    missing.extend(state.engine_runtime.vc_runtime_missing().into_iter().map(|library| format!("{executable}: {library}")));
    json!(missing)
}

/// The diagnostics as data.
async fn report(state: &AppState) -> Value {
    let root = crate::studio_data_root().unwrap_or_else(|| PathBuf::from("."));
    let target = crate::effective_install_target(state).await;
    let manager = state.model_manager.status(target).await;
    let models: Vec<Value> = manager.components.iter().map(|component| json!({ "id": component.id, "installed": component.installed, "bytes": component.bytes })).collect();
    let separation = crate::separation_status(State(state.clone())).await.0;
    let assistant = crate::assistant_runtime_status(State(state.clone())).await.0;
    let karaoke = crate::karaoke_status(State(state.clone())).await.0;
    let midi = crate::midi_status(State(state.clone())).await.0;
    let training = crate::read_training(State(state.clone())).await.0;
    let resources = crate::resources::snapshot();
    let state_for_dll = state.clone();
    let dll = tokio::task::spawn_blocking(move || dll_missing(&state_for_dll)).await.unwrap_or_else(|error| json!([format!("the check failed: {error}")]));
    // nvidia-smi is a process of its own: it waits off the async workers
    let (gpu, cards) = tokio::task::spawn_blocking(|| (crate::cuda_build::describe(), crate::cuda_build::nvidia_cards())).await.unwrap_or_else(|error| {
        studio_log!("[ERROR] the card query of the diagnostics stopped: {error}");
        (None, Vec::new())
    });
    json!({
        "version": crate::studio_version(),
        "os": format!("{} ({})", sysinfo::System::long_os_version().or_else(sysinfo::System::name).unwrap_or_else(|| std::env::consts::OS.to_string()), std::env::consts::ARCH),
        "gpu": gpu,
        "cards": cards.into_iter().map(|(index, name, vram_gb)| json!({ "index": index, "name": name, "vram_gb": vram_gb })).collect::<Vec<_>>(),
        "memory": { "ram_used_mb": resources.ram_used_mb, "ram_total_mb": resources.ram_total_mb, "vram": resources.gpus.iter().map(|gpu| json!({ "name": gpu.name, "used_mb": gpu.vram_used_mb, "total_mb": gpu.vram_total_mb })).collect::<Vec<_>>() },
        "engine_device": serde_json::to_value(*state.active_device.read().await).unwrap_or(Value::Null),
        "engine_options": serde_json::to_value(state.engine_options.read().await.clone()).unwrap_or(Value::Null),
        "models_dir": manager.model_root,
        "free_bytes": fs2::available_space(&root).ok(),
        "models_ready": manager.ready,
        "models": models,
        "optional": {
            "stems": { "ready": field(&separation, "ready"), "engine": field(&separation, "engine"), "card": field(&separation, "card"), "card_missing_bytes": field(&separation, "card_missing_bytes") },
            "assistant": { "ready": field(&assistant, "set.ready"), "provider": field(&assistant, "provider"), "model": field(&assistant, "chosen_model") },
            "karaoke": { "ready": field(&karaoke, "ready"), "enabled": field(&karaoke, "enabled"), "provider": field(&karaoke, "provider"), "runtime": field(&karaoke, "runtime") },
            "midi": { "ready": field(&midi, "tool_installed") },
            "training": { "ready": field(&training, "pack_ready"), "card_trains": field(&training, "card_trains") },
        },
        "dll_missing": dll,
        "data_dir": root.to_string_lossy(),
    })
}

/// The diagnostics as text to paste into an issue.
fn text(report: &Value, log: &[String]) -> String {
    let gb = |bytes: &Value| bytes.as_u64().map(|bytes| format!("{:.1} GB", bytes as f64 / 1e9)).unwrap_or_else(|| "?".into());
    let shown = |value: &Value| match value {
        Value::String(text) => text.clone(),
        Value::Null => "?".into(),
        other => other.to_string(),
    };
    let mut out = String::new();
    out.push_str(&format!("{} {}\n", music_core::studio().name, report["version"].as_str().unwrap_or_default()));
    out.push_str(&format!("OS: {}\n", report["os"].as_str().unwrap_or_default()));
    match report["gpu"].as_str() {
        Some(gpu) => out.push_str(&format!("GPU: {gpu}\n")),
        None => out.push_str("GPU: no NVIDIA card answered nvidia-smi\n"),
    }
    let memory = &report["memory"];
    for vram in memory["vram"].as_array().into_iter().flatten() {
        out.push_str(&format!("VRAM: {} of {} MB used ({})\n", shown(&vram["used_mb"]), shown(&vram["total_mb"]), shown(&vram["name"])));
    }
    out.push_str(&format!("RAM: {} of {} MB used\n", shown(&memory["ram_used_mb"]), shown(&memory["ram_total_mb"])));
    out.push_str(&format!("Engine device: {} · options: {}\n", shown(&report["engine_device"]), report["engine_options"]));
    out.push_str(&format!("Models: {} · free {} · ready: {}\n", report["models_dir"].as_str().unwrap_or_default(), gb(&report["free_bytes"]), report["models_ready"]));
    for model in report["models"].as_array().into_iter().flatten() {
        out.push_str(&format!("  {} {}\n", if model["installed"] == true { "[OK]" } else { "[--]" }, model["id"].as_str().unwrap_or_default()));
    }
    out.push_str("Optional parts:\n");
    for (name, part) in report["optional"].as_object().into_iter().flatten() {
        let details: Vec<String> = part.as_object().into_iter().flatten().filter(|(key, _)| *key != "ready").map(|(key, value)| format!("{key} {}", shown(value))).collect();
        out.push_str(&format!("  {} {name}{}\n", if part["ready"] == true { "[OK]" } else { "[--]" }, if details.is_empty() { String::new() } else { format!(" ({})", details.join(", ")) }));
    }
    let missing = report["dll_missing"].as_array().cloned().unwrap_or_default();
    out.push_str(&format!("DLL check: {}\n", if missing.is_empty() { "nothing missing".to_string() } else { missing.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ") }));
    out.push_str(&format!("\nLatest log ({} lines):\n", log.len()));
    for line in log {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// GET /v1/diagnostics: the report and its text.
pub async fn get(State(state): State<AppState>) -> Json<Value> {
    let report = report(&state).await;
    let root = crate::studio_data_root().unwrap_or_else(|| PathBuf::from("."));
    let log = tokio::task::spawn_blocking(move || recent_log(&root)).await.unwrap_or_default();
    Json(json!({ "text": text(&report, &log), "report": report }))
}

/// The bug-report pack: the diagnostics, the journal, the studio's logs and the engine's, every one masked.
fn pack(root: &Path, report: &Value, journal: &[crate::library::JournalEntry], engine_log: &[String]) -> Result<PathBuf, String> {
    let log = recent_log(root);
    let dir = root.join("reports");
    std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let path = dir.join(format!("ace-step-studio-report-{}.zip", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    let file = std::fs::File::create(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut put = |name: &str, body: &[u8]| -> Result<(), String> {
        zip.start_file(name, options).map_err(|error| error.to_string())?;
        zip.write_all(body).map_err(|error| error.to_string())
    };
    put("diagnostics.txt", text(report, &log).as_bytes())?;
    put("diagnostics.json", serde_json::to_string_pretty(report).map_err(|error| error.to_string())?.as_bytes())?;
    let journal: String = journal.iter().rev().filter_map(|entry| serde_json::to_string(entry).ok()).map(|line| masked(&line) + "\n").collect();
    put("journal.jsonl", journal.as_bytes())?;
    for file in log_files(root) {
        if let (Some(name), Ok(bytes)) = (file.file_name().and_then(|name| name.to_str()), std::fs::read(&file)) {
            put(name, masked(&String::from_utf8_lossy(&bytes)).as_bytes())?;
        }
    }
    if let Ok(bytes) = std::fs::read(music_engine::server::startup_log_path()) {
        put("engine-startup.log", masked(&String::from_utf8_lossy(&bytes)).as_bytes())?;
    }
    put("engine.log", masked(&engine_log.join("\n")).as_bytes())?;
    zip.finish().map_err(|error| error.to_string())?;
    Ok(path)
}

/// POST /v1/diagnostics/report: packs the bug report into the data folder and answers its path.
pub async fn make_report(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let root = crate::studio_data_root().ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the studio has no data directory".into()))?;
    let report = report(&state).await;
    let journal = state.library.journal(crate::library::JOURNAL_KEPT).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let engine_log = state.engine_log.lines().unwrap_or_else(|| music_engine::server::startup_log_tail(400));
    let path = tokio::task::spawn_blocking(move || pack(&root, &report, &journal, &engine_log))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("the report was not packed: {error}")))?;
    Ok(Json(json!({ "path": path.to_string_lossy() })))
}

/// Shows `path` in the system file manager, the file selected where the system can.
fn reveal(path: &Path, select: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let mut explorer = std::process::Command::new("explorer.exe");
        if select { explorer.arg(format!("/select,{}", path.display())) } else { explorer.arg(path) };
        music_engine::process_group::detach_from_group(&mut explorer).spawn().map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        let mut open = std::process::Command::new("open");
        if select { open.arg("-R") };
        open.arg(path).spawn().map(|_| ())
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let folder = if select { path.parent().unwrap_or(path) } else { path };
        music_engine::process_group::detach_from_group(&mut dub_core::proc::host_command("xdg-open")).arg(folder).spawn().map(|_| ())
    }
}

#[derive(serde::Deserialize)]
pub struct Revealed {
    path: String,
}

/// POST /v1/diagnostics/report/reveal {path}: shows a report of the data folder in the file manager.
pub async fn reveal_report(Json(asked): Json<Revealed>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let root = crate::studio_data_root().ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the studio has no data directory".into()))?;
    let asked = PathBuf::from(asked.path);
    let inside = asked.parent().is_some_and(|parent| parent == root.join("reports")) && asked.extension().is_some_and(|extension| extension == "zip") && asked.is_file();
    if !inside {
        return Err(api_error(StatusCode::NOT_FOUND, format!("{} is not a report of this studio", asked.display())));
    }
    reveal(&asked, true).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// POST /v1/diagnostics/logs/reveal: the studio's log folder in the file manager.
pub async fn reveal_logs() -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let root = crate::studio_data_root().ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the studio has no data directory".into()))?;
    let dir = root.join("logs");
    std::fs::create_dir_all(&dir).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{}: {error}", dir.display())))?;
    reveal(&dir, false).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_lines_come_from_the_current_log() {
        let root = std::env::temp_dir().join(format!("ace-diagnostics-{}", uuid::Uuid::now_v7()));
        let logs = root.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("ace-step-studio.old.log"), "older\n").unwrap();
        let many: String = (0..LOG_LINES + 5).map(|index| format!("line {index}\n")).collect();
        std::fs::write(logs.join("ace-step-studio.log"), many + "key sk-or-v1-abcdef1234567890\n").unwrap();
        let log = recent_log(&root);
        assert_eq!(log.len(), LOG_LINES);
        assert_eq!(log.last().map(String::as_str), Some("key [key]"));
        assert_eq!(log_files(&root).iter().map(|path| path.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(), vec!["ace-step-studio.old.log", "ace-step-studio.log"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_text_names_the_card_every_model_and_part() {
        let report = json!({
            "version": "3.7.1", "os": "Windows 11", "gpu": "NVIDIA GeForce RTX 4090, compute 8.9, driver 581.29, cuda13 build",
            "memory": { "ram_used_mb": 1, "ram_total_mb": 2, "vram": [] }, "engine_device": "Cuda", "engine_options": {},
            "models_dir": "D:/m", "free_bytes": 5_000_000_000_u64, "models_ready": true,
            "models": [{ "id": "dit-q4", "installed": true }, { "id": "lm-q8", "installed": false }],
            "optional": { "stems": { "ready": false, "card_missing_bytes": 379_723_801 } },
            "dll_missing": ["ace-server.exe: cublas64_13.dll"],
        });
        let text = text(&report, &["last line".into()]);
        for expected in ["RTX 4090", "cuda13 build", "[OK] dit-q4", "[--] lm-q8", "[--] stems (card_missing_bytes 379723801)", "cublas64_13.dll", "last line"] {
            assert!(text.contains(expected), "{expected} in\n{text}");
        }
    }

    #[test]
    fn a_status_field_is_read_by_its_path() {
        let status = json!({ "set": { "ready": true }, "provider": "local" });
        assert_eq!(field(&status, "set.ready"), Value::Bool(true));
        assert_eq!(field(&status, "provider"), json!("local"));
        assert_eq!(field(&status, "set.missing"), Value::Null);
    }
}
