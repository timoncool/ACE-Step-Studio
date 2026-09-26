//! The studio as an MCP server: Streamable HTTP, stateless JSON-RPC at `/mcp`.
//!
//! Every tool is a route of the studio's own API, called inside the process
//! through the same router the page talks to, so an agent does exactly what
//! the page does through the same code: create songs, write lyrics and styles,
//! prepare datasets, train, install LoRA, draw covers, split stems. A file an
//! agent names by its path is sent to the route as the multipart upload the
//! page would send.

use std::collections::HashMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde_json::{json, Value};
use tower::ServiceExt;

/// The studio's API router, set once the service has built it.
static API: OnceLock<Router> = OnceLock::new();

pub fn install(api: Router) {
    let _ = API.set(api);
}

/// The studio's name, as clients show it.
/// The skill an agent reads, served as a resource and a prompt.
const SKILL: &str = include_str!("../../../docs/mcp-skill.md");
const SKILL_URI: &str = "studio://skill";
const GUIDE_URI: &str = "studio://guide/";
/// Answers longer than this are cut: a library of hundreds of songs is more
/// than an agent reads in one call.
const LIMIT: usize = 60_000;

/// What a tool sends to its route.
enum Payload {
    None,
    Json(Value),
    /// Multipart form fields and files, as the page uploads them.
    Form { fields: Vec<(String, String)>, files: Vec<(String, PathBuf, String)> },
    /// A command for the studio's window: what is on screen, the buttons, the
    /// player, the video editor. Answered by the page itself.
    Window { command: &'static str, args: Value, seconds: u64 },
}

struct Call {
    method: Method,
    path: String,
    payload: Payload,
}

struct Tool {
    name: &'static str,
    description: &'static str,
    schema: fn() -> Value,
    call: fn(&Value) -> Result<Call, String>,
}

fn get(path: String) -> Result<Call, String> {
    Ok(Call { method: Method::GET, path, payload: Payload::None })
}

fn post(path: String, body: Value) -> Result<Call, String> {
    Ok(Call { method: Method::POST, path, payload: Payload::Json(body) })
}

fn send(method: Method, path: String, body: Value) -> Result<Call, String> {
    Ok(Call { method, path, payload: Payload::Json(body) })
}

fn window(command: &'static str, args: &Value, seconds: u64) -> Result<Call, String> {
    Ok(Call { method: Method::GET, path: String::new(), payload: Payload::Window { command, args: args.clone(), seconds } })
}

fn composite(kind: &'static str) -> Result<Call, String> {
    Ok(Call { method: Method::GET, path: format!("composite:{kind}"), payload: Payload::None })
}

/// A GET inside the process, as JSON.
async fn fetch(path: &str) -> Value {
    match call_route(Call { method: Method::GET, path: path.into(), payload: Payload::None }).await {
        Ok((_, text)) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
        Err(problem) => json!({ "error": problem }),
    }
}

fn compact_training(training: &Value) -> Value {
    let datasets: Vec<Value> = training["datasets"].as_array().into_iter().flatten().map(|dataset| {
        let items = dataset["items"].as_array().cloned().unwrap_or_default();
        let count = |field: &str, value: &str| items.iter().filter(|item| item[field] == value).count();
        json!({
            "id": dataset["id"], "name": dataset["name"], "trigger": dataset["trigger"], "songs": items.len(),
            "ready": items.iter().filter(|item| item["lyrics_state"] == "done" && item["style_state"] == "done").count(),
            "lyrics": { "wanted": count("lyrics_state", "wanted"), "found": count("lyrics_state", "found"), "done": count("lyrics_state", "done") },
            "style": { "wanted": count("style_state", "wanted"), "done": count("style_state", "done") },
        })
    }).collect();
    let runs: Vec<Value> = training["runs"].as_array().into_iter().flatten().map(compact_run).collect();
    json!({
        "trainer_installed": training["pack_ready"],
        "listening_installed": training["listen"]["ready"],
        "download": training["download"],
        "preparation": compact_preparation(&training["prepare"]),
        "active_run": training["active"],
        "datasets": datasets,
        "runs": runs,
        "recipe_defaults": training["recipe_defaults"],
    })
}

fn compact_run(run: &Value) -> Value {
    let steps = run["steps"].as_array().cloned().unwrap_or_default();
    let last = steps.last().cloned().unwrap_or(Value::Null);
    json!({
        "id": run["id"], "name": run["name"], "dataset": run["dataset_name"], "trigger": run["trigger"], "status": run["status"], "stage": run["stage"],
        "steps_done": steps.len(), "stop": run["recipe"]["stop"], "step_limit": run["recipe"]["steps"], "epochs": run["recipe"]["epochs"],
        "last_step": last, "checkpoints": run["checkpoints"], "installed": run["installed"], "error": run["error"],
    })
}

fn compact_preparation(prepare: &Value) -> Value {
    if prepare.is_null() {
        return Value::Null;
    }
    json!({
        "dataset": prepare["dataset"], "finished": prepare["finished"], "cancelled": prepare["cancelled"],
        "stages": prepare["stages"], "songs_left": prepare["pending"].as_array().map_or(0, Vec::len),
        "failures": prepare["failures"], "notices": prepare["notices"], "run": prepare["run"],
    })
}

/// A song job as an agent follows it: where it is, what it made. The
/// request's lyrics, score and audio codes are left to response_format detailed.
fn compact_job(job: &Value) -> Value {
    if !job.is_object() || job.get("error").is_some_and(|error| error.is_string()) && job.get("status").is_none() {
        return job.clone();
    }
    let mut songs: Vec<Value> = job["songs"].as_array().into_iter().flatten().map(|song| json!({ "id": song["id"], "title": song["song"]["title"] })).collect();
    if songs.is_empty() && job["song"].is_object() {
        songs.push(json!({ "id": job["song"]["id"], "title": job["song"]["song"]["title"] }));
    }
    let settings = &job["generation_settings"];
    json!({
        "id": job["id"], "title": job["title"], "status": job["status"], "phase": job["phase"], "message": job["message"],
        "duration_seconds": job["duration_seconds"], "seed": settings["seed"], "lm_seed": settings["lm_seed"],
        "adapters": settings["adapters"], "songs": songs,
    })
}

/// A library song without what only a re-render reads: its audio codes and
/// the request they were made from, both large.
fn compact_song(song: &Value) -> Value {
    let mut song = song.clone();
    if let Some(fields) = song.as_object_mut() {
        let codes = fields.remove("audio_codes").is_some_and(|codes| !codes.is_null());
        fields.remove("replay_request");
        fields.insert("has_audio_codes".into(), codes.into());
        // the score and the karaoke timings are long; they say they are there
        if let Some(settings) = fields.get_mut("generation_settings").and_then(Value::as_object_mut) {
            let score = settings.remove("abc").is_some_and(|abc| abc.as_str().is_some_and(|abc| !abc.trim().is_empty()));
            settings.remove("lyrics");
            settings.insert("has_score".into(), score.into());
        }
        if let Some(metadata) = fields.get_mut("metadata").and_then(Value::as_object_mut) {
            let karaoke = metadata.remove("lrc").is_some_and(|lrc| lrc.as_str().is_some_and(|lrc| !lrc.is_empty()));
            metadata.insert("has_karaoke".into(), karaoke.into());
        }
    }
    song
}

/// The recogniser settings and the models to choose from, without the files
/// each model is made of.
fn compact_karaoke(status: &Value) -> Value {
    let models: Vec<Value> = status["assets"].as_array().into_iter().flatten()
        .filter(|asset| asset["kind"] == "model" && !asset["vram_gb"].is_null())
        .map(|asset| json!({ "id": asset["id"], "label": asset["label"], "installed": asset["installed"], "vram_gb": asset["vram_gb"], "about": asset["note"] }))
        .collect();
    json!({
        "enabled": status["enabled"], "provider": status["provider"], "whisper_model": status["whisper_model"], "openrouter_model": status["openrouter_model"],
        "runtime": status["runtime"], "ready": status["ready"], "download": status["active_download"], "models": models,
    })
}

/// A name or a description the catalogue gives in several languages, in English.
fn english(value: &Value) -> Value {
    if value.is_object() { value["en"].clone() } else { value.clone() }
}

/// The LoRA an agent picks from: installed ones with their slots and trigger,
/// and the catalogue in one line each.
fn compact_loras(loras: &Value) -> Value {
    let installed: Vec<Value> = loras["installed"].as_array().into_iter().flatten().map(|lora| json!({
        "id": lora["id"], "name": english(&lora["name"]), "kind": lora["kind"], "trigger": lora["trigger"],
        "slots": lora["slots"], "scales": lora["scales"], "range": lora["range"], "error": lora["error"],
    })).collect();
    let catalog: Vec<Value> = loras["catalog"].as_array().into_iter().flatten().map(|lora| json!({
        "id": lora["id"], "name": english(&lora["name"]), "kind": lora["kind"], "installed": lora["installed"],
        "trigger": lora["trigger"], "slots": lora["slots"], "about": english(&lora["description"]),
    })).collect();
    json!({ "installed": installed, "catalog": catalog, "slots": loras["slots"], "download": loras["download"], "installing": loras["installing"] })
}

/// What an agent is answered: the parts it acts on, unless it asked for
/// every field with response_format detailed.
fn shape(name: &str, args: &Value, value: Value) -> Value {
    let detailed = args.get("response_format").and_then(Value::as_str) == Some("detailed");
    match name {
        "training_status" if !detailed => compact_training(&value),
        "song_create" | "song_job_get" | "song_replay" if !detailed => compact_job(&value),
        "song_jobs_list" if !detailed => Value::Array(value.as_array().into_iter().flatten().map(compact_job).collect()),
        "library_song_get" if !detailed => compact_song(&value),
        "lora_list" if !detailed => compact_loras(&value),
        "midi_get" if !detailed => {
            let mut value = value;
            let notes = value.as_object_mut().and_then(|fields| fields.remove("notes")).and_then(|notes| notes.as_array().map(Vec::len)).unwrap_or(0);
            value["notes_count"] = notes.into();
            value
        }
        "karaoke_settings_get" if !detailed => compact_karaoke(&value),
        "training_checkpoint_install" | "lora_install_hf" | "lora_import_files" if value["slots"].as_array().is_some_and(Vec::is_empty) => {
            let mut value = value;
            value["slots"] = json!("not known yet: the engine reads them from the file; lora_list shows them");
            value
        }
        "dataset_get" => {
            let wanted = args.get("dataset_id").and_then(Value::as_str).unwrap_or_default();
            let Some(dataset) = value["datasets"].as_array().into_iter().flatten().find(|dataset| dataset["id"] == wanted).cloned() else {
                return json!({ "error": format!("No dataset {wanted}; training_status lists them.") });
            };
            match args.get("song_id").and_then(Value::as_str) {
                Some(song) => dataset["items"].as_array().into_iter().flatten().find(|item| item["id"] == song).cloned().unwrap_or_else(|| json!({ "error": format!("No song {song} in the dataset.") })),
                None => dataset,
            }
        }
        "library_songs_list" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or_default().to_lowercase();
            let songs = value.as_array().cloned().unwrap_or_default().into_iter().filter(|song| {
                query.is_empty() || song["title"].as_str().unwrap_or_default().to_lowercase().contains(&query) || song["caption"].as_str().unwrap_or_default().to_lowercase().contains(&query)
            });
            if detailed {
                return Value::Array(songs.collect());
            }
            Value::Array(songs.map(|song| {
                let style: String = song["caption"].as_str().unwrap_or_default().trim_start().trim_start_matches("Global Metadata").trim_start_matches(':').trim_start().chars().take(90).collect();
                let mut row = json!({ "id": song["id"], "title": song["title"], "made": song["created_at"], "style": style });
                // a track a tool made names the one it was made from
                if song["metadata"]["derived"].is_object() {
                    row["made_from"] = song["metadata"]["derived"]["from"].clone();
                    row["made_by"] = song["metadata"]["derived"]["tool"].clone();
                }
                row
            }).collect())
        }
        _ => value,
    }
}

async fn status_summary() -> Value {
    let (jobs, activity, training, processing, separation) = tokio::join!(fetch("/v1/music/jobs"), fetch("/v1/activity"), fetch("/v1/training"), fetch("/v1/processing"), fetch("/v1/separation/status"));
    let midi = fetch("/v1/midi").await;
    let running_activity: Vec<Value> = activity["activity"].as_array().into_iter().flatten().filter(|entry| entry["state"] == "running").cloned().collect();
    let active_run = training["runs"].as_array().into_iter().flatten().find(|run| Some(run["id"].as_str().unwrap_or_default()) == training["active"].as_str()).map(compact_run);
    json!({
        "window_open": !open_windows().is_empty(),
        "assistant_requests_waiting": open_questions().len(),
        "song_jobs": Value::Array(jobs.as_array().into_iter().flatten().map(compact_job).collect()),
        "covers_and_karaoke": running_activity,
        "stems": separation["run"],
        "midi": midi["run"],
        "preparation": compact_preparation(&training["prepare"]),
        "training": active_run,
        "processing": processing["run"],
    })
}

/// Whether anything in a status summary is still at work.
fn busy(summary: &Value) -> Vec<&'static str> {
    let unfinished = |run: &Value| run.is_object() && run["done"] != true;
    [
        ("song_jobs", summary["song_jobs"].as_array().is_some_and(|jobs| !jobs.is_empty())),
        ("covers_and_karaoke", summary["covers_and_karaoke"].as_array().is_some_and(|entries| !entries.is_empty())),
        ("stems", unfinished(&summary["stems"])),
        ("processing", unfinished(&summary["processing"])),
        ("midi", unfinished(&summary["midi"])),
        ("preparation", summary["preparation"].is_object() && summary["preparation"]["finished"] != true),
        ("training", !summary["training"].is_null()),
    ]
    .into_iter()
    .filter_map(|(what, working)| working.then_some(what))
    .collect()
}

/// How long one wait holds a call: clients give up on a tool call after about
/// a minute, so a wait answers before that and the agent calls it again.
const WAIT_DEFAULT: u64 = 30;
const WAIT_LONGEST: u64 = 55;

/// Waits for a job, the preparation, training or everything, a slice at a time.
async fn wait_for(args: &Value) -> Result<Value, String> {
    let seconds = args.get("seconds").and_then(Value::as_u64).unwrap_or(WAIT_DEFAULT).clamp(2, WAIT_LONGEST);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    let job = args.get("job_id").and_then(Value::as_str).map(str::to_string);
    let until = args.get("until").and_then(Value::as_str).unwrap_or("idle").to_string();
    loop {
        let (done, now) = match &job {
            Some(job) => {
                let state = fetch(&format!("/v1/music/jobs/{}", segment(job))).await;
                if state.get("status").is_none() {
                    let why = state["error"].as_str().or(state.as_str()).unwrap_or("not found");
                    return Err(format!("No song job {job} ({why}): job_id is what song_create or song_replay returned. Wait for other work with until."));
                }
                let state = compact_job(&state);
                let status = state["status"].as_str().unwrap_or_default().to_string();
                (["completed", "failed", "cancelled", "done", "error"].contains(&status.as_str()), state)
            }
            None => {
                let summary = status_summary().await;
                let working = busy(&summary);
                let done = if until == "idle" { working.is_empty() } else { !working.contains(&until.as_str()) };
                (done, summary)
            }
        };
        if done {
            return Ok(json!({ "done": true, "state": now }));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(json!({ "done": false, "note": "still running; call studio_wait again to keep waiting", "state": now }));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// MCP tool annotations, from what each tool does.
fn annotations(name: &str) -> Value {
    const READS: &[&str] = &["_get", "_status", "_list", "_catalog", "_files", "_logs", "_log", "_runtime", "_local_models", "_hf_files", "_search_hf", "_capabilities", "_system", "_state", "_read_page", "_screenshot"];
    // a verb that changes something outweighs a noun that reads
    const CHANGES: &[&str] = &["install", "import", "remove", "delete", "refresh", "create", "update", "start", "cancel", "select", "download", "apply", "restart"];
    // reads whose names the rules above miss: create_form names the create page
    const READ_NAMES: &[&str] = &["lyrics_find", "cover_prompt_render", "studio_wait", "engine_presets_get", "assistant_requests_wait", "ui_console", "song_defaults", "create_form_get"];
    let changes = CHANGES.iter().any(|verb| name.split('_').any(|word| word == *verb));
    let read_only = READ_NAMES.contains(&name) || !changes && (READS.iter().any(|part| name.ends_with(part) || name.contains(&format!("{part}_"))) || name.starts_with("writing_"));
    let destructive = name.ends_with("_delete") || name.ends_with("_remove") || name == "processing_discard" || name.ends_with("_cancel") || name.contains("_cancel_");
    // what reaches the internet: OpenRouter, Hugging Face, the lyric databases and every download
    let open_world = name.starts_with("openrouter_") || name.contains("_hf") || name == "lyrics_find" || name == "models_download" || name == "lora_install_catalog" || name.ends_with("_install") && name != "training_checkpoint_install";
    let title = name.replace('_', " ");
    json!({ "title": title, "readOnlyHint": read_only, "destructiveHint": destructive, "idempotentHint": read_only || name.ends_with("_set") || name.contains("_select"), "openWorldHint": open_world })
}

/// The window's side of the bridge: the page subscribes to the commands and
/// posts each answer back.
struct Bridge {
    commands: tokio::sync::broadcast::Sender<String>,
    pending: Mutex<HashMap<String, tokio::sync::oneshot::Sender<Result<Value, String>>>>,
    /// The open windows, oldest first; a command goes to the newest only, so
    /// a second window never runs it again.
    windows: Mutex<Vec<u64>>,
    sequence: AtomicU64,
}

fn bridge() -> &'static Bridge {
    static BRIDGE: OnceLock<Bridge> = OnceLock::new();
    BRIDGE.get_or_init(|| Bridge { commands: tokio::sync::broadcast::channel(64).0, pending: Mutex::new(HashMap::new()), windows: Mutex::new(Vec::new()), sequence: AtomicU64::new(0) })
}

fn open_windows() -> std::sync::MutexGuard<'static, Vec<u64>> {
    bridge().windows.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Keeps the window on the list while its stream is open.
struct Listening(u64);

impl Drop for Listening {
    fn drop(&mut self) {
        open_windows().retain(|window| *window != self.0);
    }
}

/// Only the studio's own page and local agents may drive it: a web page in
/// the user's browser, or one rebinding a domain to this computer, sends its
/// own origin and is refused.
fn local_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else { return true };
    let Ok(origin) = origin.to_str() else { return false };
    let Some((scheme, rest)) = origin.split_once("://") else { return false };
    let host = rest.split('/').next().unwrap_or_default();
    let host = if host.starts_with('[') { host.split(']').next().map(|name| format!("{name}]")).unwrap_or_default() } else { host.split(':').next().unwrap_or_default().to_string() };
    scheme == "tauri" || ["localhost", "127.0.0.1", "[::1]", "tauri.localhost"].contains(&host.as_str())
}

fn foreign_origin() -> Response {
    (StatusCode::FORBIDDEN, "This studio answers only its own window and agents on this computer.").into_response()
}

/// The stream of commands the studio's page executes. Its first message
/// names the window, so it knows the commands addressed to it.
pub async fn window_events(headers: HeaderMap) -> Response {
    if !local_origin(&headers) {
        return foreign_origin();
    }
    let window = bridge().sequence.fetch_add(1, Ordering::Relaxed);
    open_windows().push(window);
    let receiver = bridge().commands.subscribe();
    let hello = futures_util::stream::once(async move { Ok::<Event, Infallible>(Event::default().data(json!({ "window": window }).to_string())) });
    let commands = futures_util::stream::unfold((receiver, Listening(window)), |(mut receiver, listening)| async move {
        loop {
            match receiver.recv().await {
                Ok(command) => return Some((Ok(Event::default().data(command)), (receiver, listening))),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(futures_util::StreamExt::chain(hello, commands)).keep_alive(KeepAlive::default()).into_response()
}

#[derive(serde::Deserialize)]
pub struct WindowAnswer {
    id: String,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<String>,
}

/// The page's answer to one command.
pub async fn window_result(headers: HeaderMap, Json(answer): Json<WindowAnswer>) -> StatusCode {
    if !local_origin(&headers) {
        return StatusCode::FORBIDDEN;
    }
    let waiting = bridge().pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&answer.id);
    match waiting {
        Some(sender) => {
            let _ = sender.send(match answer.error {
                Some(error) => Err(error),
                None => Ok(answer.result),
            });
            StatusCode::NO_CONTENT
        }
        None => StatusCode::NOT_FOUND,
    }
}

async fn ask_window(command: &str, args: Value, seconds: u64) -> Result<Value, String> {
    let Some(window) = open_windows().last().copied() else {
        return Err(format!("The studio's window is not open. Open {} and call the tool again; everything else works without it.", music_core::studio().name));
    };
    let id = format!("w{}", bridge().sequence.fetch_add(1, Ordering::Relaxed));
    let (sender, receiver) = tokio::sync::oneshot::channel();
    bridge().pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(id.clone(), sender);
    let _ = bridge().commands.send(json!({ "id": id, "window": window, "command": command, "args": args }).to_string());
    match tokio::time::timeout(Duration::from_secs(seconds), receiver).await {
        Ok(Ok(answer)) => answer,
        _ => {
            bridge().pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&id);
            Err(format!("The window did not answer '{command}' within {seconds} s."))
        }
    }
}

/// The studio's questions for its writing assistant when the user has made
/// the connected agent that assistant: each waits here until the agent
/// answers it with assistant_request_answer.
struct AgentQuestion {
    question: Value,
    answer: tokio::sync::oneshot::Sender<String>,
}

struct Agent {
    questions: Mutex<Vec<AgentQuestion>>,
    /// When an agent last called the server, and what it called.
    last_call: Mutex<Option<(std::time::Instant, String)>>,
    calls: AtomicU64,
}

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| Agent { questions: Mutex::new(Vec::new()), last_call: Mutex::new(None), calls: AtomicU64::new(0) })
}

fn questions() -> std::sync::MutexGuard<'static, Vec<AgentQuestion>> {
    agent().questions.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// An agent that called within this long still counts as connected.
const AGENT_PRESENT: Duration = Duration::from_secs(600);
/// How long a question waits for the agent's answer.
const AGENT_ANSWER: Duration = Duration::from_secs(900);

fn seen(what: &str) {
    *agent().last_call.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((std::time::Instant::now(), what.to_string()));
    agent().calls.fetch_add(1, Ordering::Relaxed);
}

fn agent_present() -> bool {
    agent().last_call.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().is_some_and(|(at, _)| at.elapsed() < AGENT_PRESENT)
}

/// The questions still waiting for an answer; one whose asker gave up is gone.
fn open_questions() -> Vec<Value> {
    let mut waiting = questions();
    waiting.retain(|question| !question.answer.is_closed());
    waiting.iter().map(|question| question.question.clone()).collect()
}

/// Asks the connected agent what the studio would ask its own assistant: the
/// same instructions, the same request, the same answer schema.
pub async fn ask_agent(system: &str, user: &str, schema: Option<Value>, target: &str) -> Result<String, String> {
    if !agent_present() {
        return Err("No agent is connected to the studio. Connect one to its MCP server (Settings, Agent) or choose another assistant.".into());
    }
    let id = format!("q{}", bridge().sequence.fetch_add(1, Ordering::Relaxed));
    let (sender, receiver) = tokio::sync::oneshot::channel();
    questions().push(AgentQuestion { question: json!({ "id": id, "target": target, "instructions": system, "request": user, "answer_schema": schema }), answer: sender });
    match tokio::time::timeout(AGENT_ANSWER, receiver).await {
        Ok(Ok(answer)) => Ok(answer),
        _ => {
            questions().retain(|question| question.question["id"] != id.as_str());
            Err(format!("The agent did not answer within {} minutes.", AGENT_ANSWER.as_secs() / 60))
        }
    }
}

/// Waits until the studio has a question for the agent, a slice at a time.
async fn wait_for_questions(args: &Value) -> Value {
    let seconds = args.get("seconds").and_then(Value::as_u64).unwrap_or(WAIT_DEFAULT).clamp(1, WAIT_LONGEST);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        let waiting = open_questions();
        if !waiting.is_empty() {
            return json!({ "requests": waiting });
        }
        if tokio::time::Instant::now() >= deadline {
            return json!({ "requests": [], "note": "no request yet; call assistant_requests_wait again to keep listening" });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn answer_question(args: &Value) -> Result<String, String> {
    let id = text(args, "request_id")?;
    let answer = match args.get("answer") {
        Some(Value::String(text)) => text.clone(),
        Some(value) if !value.is_null() => value.to_string(),
        _ => return Err("'answer' is required".into()),
    };
    let found = {
        let mut waiting = questions();
        waiting.iter().position(|question| question.question["id"] == id.as_str()).map(|at| waiting.remove(at))
    };
    let Some(question) = found else {
        return Err(format!("No request {id} is waiting; assistant_requests_wait lists the open ones."));
    };
    question.answer.send(answer).map_err(|_| format!("Request {id} was given up by the studio before the answer came."))?;
    Ok("Answered; the studio goes on with it.".into())
}

/// Whether an agent and the window are connected, for the settings page.
pub async fn status() -> Json<Value> {
    let last = agent().last_call.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    Json(json!({
        "window_open": !open_windows().is_empty(),
        "agent_connected": agent_present(),
        "agent_last_call": last.as_ref().map(|(_, what)| what.clone()),
        "agent_seconds_ago": last.as_ref().map(|(at, _)| at.elapsed().as_secs()),
        "agent_calls": agent().calls.load(Ordering::Relaxed),
        "requests_waiting": open_questions().len(),
    }))
}

/// A required text argument, or a message saying which is missing.
fn text(args: &Value, name: &str) -> Result<String, String> {
    args.get(name).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).ok_or_else(|| format!("'{name}' is required"))
}

/// A path segment, escaped.
pub(crate) fn segment(value: &str) -> String {
    value.bytes().map(|byte| if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") }).collect()
}

/// The arguments without the ones that went into the path.
fn body_without(args: &Value, taken: &[&str]) -> Value {
    let mut body = args.as_object().cloned().unwrap_or_default();
    for name in taken {
        body.remove(*name);
    }
    Value::Object(body)
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

fn id_only(name: &str, what: &str) -> Value {
    object(json!({ name: { "type": "string", "description": what } }), &[name])
}

fn nothing() -> Value {
    json!({ "type": "object", "additionalProperties": false })
}

/// The files under a folder that `keep` takes, with their path inside it: the
/// folders a song sits in name its artist when its tags do not. A folder
/// linked from inside itself is walked once.
fn folder_files(folder: &Path, keep: fn(&Path) -> bool, what: &str) -> Result<Vec<(String, PathBuf, String)>, String> {
    let root = folder.parent().unwrap_or(folder);
    let mut found = Vec::new();
    let mut walked = std::collections::HashSet::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if !walked.insert(std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone())) {
            continue;
        }
        let entries = std::fs::read_dir(&dir).map_err(|error| format!("read {}: {error}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if keep(&path) {
                let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                found.push(("files".to_string(), path, relative));
            }
        }
    }
    found.sort_by(|a, b| a.2.cmp(&b.2));
    if found.is_empty() {
        return Err(format!("no {what} in {}", folder.display()));
    }
    Ok(found)
}

/// Audio, lyrics and cue files: what a dropped folder of songs brings.
fn song_file(path: &Path) -> bool {
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_lowercase();
    ["wav", "mp3", "flac", "ogg", "m4a", "txt", "lrc", "cue"].contains(&extension.as_str())
}

/// A dataset folder of the studio family: its dataset.json and WAV files.
fn dataset_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "dataset.json") || path.extension().is_some_and(|value| value.eq_ignore_ascii_case("wav"))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "audio".into())
}

fn tools() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        vec![
            // ---------------------------------------------------------------- the studio
            Tool {
                name: "studio_status",
                description: "What the studio is doing now, in one short summary: song jobs, covers and karaoke, the stem split, the MIDI transcription, audio processing, the dataset preparation and its stages, the training run with its step and loss, and whether the studio's window is open. Call it first, and use studio_wait to wait.",
                schema: nothing,
                call: |_| composite("status"),
            },
            Tool {
                name: "studio_wait",
                description: "Wait for work to finish instead of polling: a song job (job_id), or until one kind of work is over - song_jobs, covers_and_karaoke, stems, midi, processing, preparation, training - or everything (until: idle, the default). Returns when it is done or after seconds (30 by default, at most 55, under the minute clients allow a call) with how far it got; call it again to keep waiting.",
                schema: || object(json!({ "job_id": { "type": "string" }, "until": { "type": "string", "enum": ["idle", "song_jobs", "covers_and_karaoke", "stems", "midi", "processing", "preparation", "training"] }, "seconds": { "type": "integer" } }), &[]),
                call: |_| composite("wait"),
            },
            Tool {
                name: "studio_system",
                description: "The graphics card, its free video memory, RAM, CPU load and the engine's memory.",
                schema: nothing,
                call: |_| get("/v1/system/resources".into()),
            },
            Tool {
                name: "models_status",
                description: "The installed model set, what is downloaded and what is missing, and download progress.",
                schema: nothing,
                call: |_| get("/setup/status".into()),
            },
            Tool {
                name: "models_catalog",
                description: "Every model set and file the studio can download, with sizes and the video memory each needs.",
                schema: nothing,
                call: |_| get("/setup/catalog".into()),
            },
            Tool {
                name: "models_download",
                description: "Download a model set (profile_id from models_catalog) or single components (ids). Resumes what is partly there.",
                schema: || object(json!({ "profile_id": { "type": "string" }, "ids": { "type": "array", "items": { "type": "string" } } }), &[]),
                call: |args| post("/setup/download".into(), args.clone()),
            },
            Tool {
                name: "models_adopt",
                description: "Take model files already on this computer instead of downloading them: every file of the catalogue found in the folder (by name, else by exact size) is linked into the studio's models. models_status shows the result.",
                schema: || id_only("path", "folder with the model files"),
                call: |args| post("/setup/adopt".into(), json!({ "path": text(args, "path")? })),
            },
            Tool {
                name: "song_defaults",
                description: "What the engine does with a song_create field left out: its default steps, guidance, duration and sampling, its version, and the weights it serves.",
                schema: nothing,
                call: |_| get("/v1/local-models/music".into()),
            },
            Tool {
                name: "models_select",
                description: "Use an installed model set (profile_id) or a custom mix of components (component_ids) for generation.",
                schema: || object(json!({ "profile_id": { "type": "string" }, "component_ids": { "type": "array", "items": { "type": "string" } } }), &[]),
                call: |args| post("/setup/select".into(), args.clone()),
            },
            Tool {
                name: "engine_options_get",
                description: "The engine's launch options: backend (auto, cuda, vulkan, cpu), keep models loaded, and the rest.",
                schema: nothing,
                call: |_| get("/engine/options".into()),
            },
            Tool {
                name: "engine_options_set",
                description: "Change the engine's launch options (the whole object as engine_options_get returns it, with your changes); engine_restart applies them.",
                schema: || json!({ "type": "object" }),
                call: |args| send(Method::PUT, "/engine/options".into(), args.clone()),
            },
            Tool {
                name: "engine_restart",
                description: "Restart the music engine, for example after changing its options or models.",
                schema: nothing,
                call: |_| post("/engine/restart".into(), json!({})),
            },
            Tool {
                name: "engine_presets_get",
                description: "The engine's ready-made presets (speed against quality and video memory) and which one is in use.",
                schema: nothing,
                call: |_| get("/engine/presets".into()),
            },
            Tool {
                name: "engine_preset_apply",
                description: "Use an engine preset by id from engine_presets_get; the engine restarts with it.",
                schema: || id_only("id", "preset id"),
                call: |args| post("/engine/preset".into(), args.clone()),
            },
            // ---------------------------------------------------------------- the window: what the user sees
            Tool {
                name: "ui_screenshot",
                description: "A picture of the studio's window as the user sees it now. Use it to check what a change looks like, and before clicking anything.",
                schema: || object(json!({ "max_width": { "type": "integer", "description": "pixels, 1600 by default" } }), &[]),
                call: |args| window("screenshot", args, 30),
            },
            Tool {
                name: "ui_read_page",
                description: "Every visible control of the window, one per line: its ref (e12), kind, label and value. Refs are what ui_click, ui_type and ui_select take; read the page again after it changes.",
                schema: nothing,
                call: |args| window("read_page", args, 15),
            },
            Tool {
                name: "ui_click",
                description: "Click a control of the window like the user would: by ref from ui_read_page, or by its visible label in text.",
                schema: || object(json!({ "ref": { "type": "string" }, "text": { "type": "string" } }), &[]),
                call: |args| window("click", args, 15),
            },
            Tool {
                name: "ui_type",
                description: "Type into a field of the window (replaces its text); submit presses Enter after.",
                schema: || object(json!({ "ref": { "type": "string" }, "text": { "type": "string", "description": "the field's label, when there is no ref" }, "value": { "type": "string" }, "submit": { "type": "boolean" } }), &["value"]),
                call: |args| window("type", args, 15),
            },
            Tool {
                name: "ui_select",
                description: "Choose an option of a list in the window.",
                schema: || object(json!({ "ref": { "type": "string" }, "text": { "type": "string" }, "value": { "type": "string" } }), &["value"]),
                call: |args| window("select", args, 15),
            },
            Tool {
                name: "ui_press_key",
                description: "Press a key in the window: Enter, Escape (closes a dialog), Tab, ArrowDown...",
                schema: || id_only("key", "the key"),
                call: |args| window("press_key", args, 15),
            },
            Tool {
                name: "ui_scroll",
                description: "Scroll the page (direction down or up, amount in pixels), or bring a control into view by ref or text.",
                schema: || object(json!({ "direction": { "type": "string", "enum": ["down", "up"] }, "amount": { "type": "integer" }, "ref": { "type": "string" }, "text": { "type": "string" } }), &[]),
                call: |args| window("scroll", args, 15),
            },
            Tool {
                name: "ui_navigate",
                description: "Open a page of the studio: create, library, search, playlist, adapters (LoRA and training), tools, news.",
                schema: || object(json!({ "view": { "type": "string", "enum": ["create", "library", "search", "playlist", "adapters", "tools", "news"] } }), &["view"]),
                call: |args| window("navigate", args, 15),
            },
            Tool {
                name: "ui_open_settings",
                description: "Open the settings window, at a section when given.",
                schema: || object(json!({ "section": { "type": "string" } }), &[]),
                call: |args| window("open_settings", args, 15),
            },
            // ---------------------------------------------------------------- the agent as the studio's assistant
            Tool {
                name: "assistant_requests_wait",
                description: "When the user has chosen you as the studio's writing assistant (assistant engine 'Agent (MCP)'), what the studio would ask its own assistant waits here: the create page's write buttons, and the lyric layout and the styles of a dataset preparation. Returns the waiting requests - id, target, the instructions the studio's assistant would get, the request, and the JSON schema the answer must match - as soon as there is one, or after seconds (30 by default, at most 55). Answer each with assistant_request_answer; keep calling while the user works.",
                schema: || object(json!({ "seconds": { "type": "integer" } }), &[]),
                call: |_| composite("questions"),
            },
            Tool {
                name: "assistant_request_answer",
                description: "Answer a request from assistant_requests_wait: answer is the JSON object its answer_schema describes (or plain text for a request without a schema), written by its instructions. The studio goes on with it as with its own assistant's answer.",
                schema: || object(json!({ "request_id": { "type": "string" }, "answer": { "anyOf": [{ "type": "object" }, { "type": "string" }] } }), &["request_id", "answer"]),
                call: |_| composite("answer"),
            },
            Tool {
                name: "ui_notify",
                description: "Show the user a short message in the studio's window, for a few seconds: what you did, what you need from them. tone: info (default), success or error.",
                schema: || object(json!({ "text": { "type": "string" }, "tone": { "type": "string", "enum": ["info", "success", "error"] } }), &["text"]),
                call: |args| window("notify", args, 15),
            },
            Tool {
                name: "ui_console",
                description: "The errors and warnings the studio's window logged lately, newest last: what went wrong on the page when a button did nothing.",
                schema: nothing,
                call: |args| window("console", args, 15),
            },
            // ---------------------------------------------------------------- the create page's form
            Tool {
                name: "create_form_get",
                description: "The create page's form as the user sees it now: every field, the request it would send, whether the model is ready, and the form's error if it refused. The create page must be open (ui_navigate create).",
                schema: nothing,
                call: |args| window("create_get", args, 15),
            },
            Tool {
                name: "create_form_set",
                description: "Fill the create page's form in the window, as if typed - the user sees every field change; fields not given stay. fields: mode (simple|studio), title, caption, lyrics, instrumental, vocal_language, bpm, keyscale, timesignature, duration, task_type, source_song_id, reference_song_id, tracks, audio_cover_strength, cover_noise_strength, repainting_start, repainting_end, think, inference_steps, guidance_scale, shift, solver, scheduler, guidance, lm_temperature, lm_cfg_scale, lm_top_p, lm_top_k, lm_seed, lm_batch_size, synth_batch_size, seed, randomize_seed, audio_codes, adapters, cover_prompt, output_format, mp3_bitrate, peak_clip. Use it when the user wants to see and adjust the song before it is made; song_create makes one directly.",
                schema: || object(json!({ "fields": { "type": "object", "description": "field -> value" } }), &["fields"]),
                call: |args| window("create_set", args, 15),
            },
            Tool {
                name: "create_form_submit",
                description: "Press Create on the create page: the song is made from the form as it stands. studio_wait until idle waits for it; create_form_get shows why the form refused, if it did.",
                schema: nothing,
                call: |args| window("create_submit", args, 15),
            },
            // ---------------------------------------------------------------- the player
            Tool {
                name: "player_state",
                description: "What the studio's player plays: the song, playing or paused, position, length, volume, shuffle, repeat.",
                schema: nothing,
                call: |args| window("player_state", args, 15),
            },
            Tool {
                name: "player_play",
                description: "Play a library song (song_id) in the studio's player, or resume what is loaded. With stem (drums, bass, other, vocals, guitar, piano) it plays that separated stem of the song alone; stems_split makes them, stems_get lists them.",
                schema: || object(json!({ "song_id": { "type": "string" }, "stem": { "type": "string", "enum": ["drums", "bass", "other", "vocals", "guitar", "piano"] } }), &[]),
                call: |args| window("player_play", args, 15),
            },
            Tool {
                name: "player_pause",
                description: "Pause the studio's player.",
                schema: nothing,
                call: |args| window("player_pause", args, 15),
            },
            Tool {
                name: "player_seek",
                description: "Move the player to a position in seconds.",
                schema: || object(json!({ "seconds": { "type": "number" } }), &["seconds"]),
                call: |args| window("player_seek", args, 15),
            },
            Tool {
                name: "player_next",
                description: "Play the next song of the queue.",
                schema: nothing,
                call: |args| window("player_next", args, 15),
            },
            Tool {
                name: "player_previous",
                description: "Play the previous song of the queue.",
                schema: nothing,
                call: |args| window("player_previous", args, 15),
            },
            Tool {
                name: "player_set",
                description: "Set the player's volume (0 to 1), shuffle, and repeat (none, all, one).",
                schema: || object(json!({ "volume": { "type": "number" }, "shuffle": { "type": "boolean" }, "repeat": { "type": "string", "enum": ["none", "all", "one"] } }), &[]),
                call: |args| window("player_set", args, 15),
            },
            // ---------------------------------------------------------------- the video editor
            Tool {
                name: "video_open",
                description: "Open the video editor for a library song: an audio-reactive clip with a visualiser, background, cover, text layers and karaoke lyrics, rendered to MP4. The studio's window must be visible (not minimised, not a hidden tab): a hidden window holds the preview and the render.",
                schema: || id_only("song_id", "library song id"),
                call: |args| window("video_open", args, 15),
            },
            Tool {
                name: "video_get",
                description: "Everything the open video editor is set to: preset and the list of presets, aspect ratio, colours, positions, effects and their intensities, text layers, lyrics overlay, background, cover, playback, and the render's progress and saved file.",
                schema: nothing,
                call: |args| window("video_get", args, 15),
            },
            Tool {
                name: "video_set",
                description: "Change the open video editor; any part, the rest stays. config: {preset, aspectRatio 16:9|9:16|1:1, primaryColor, secondaryColor, bgDim 0-1, particleCount, visualizerX/Y 0-100, visualizerScale 0.3-2, lyricsX/Y}. effects: {shake, glitch, vhs, cctv, scanlines, chromatic, bloom, filmGrain, pixelate, strobe, vignette, hueShift, letterbox: true/false}; intensities: the same names, 0-1. text_layers: [{text, x, y, size, color, font}], replaces all. lyrics: {enabled, style lines|scroll|karaoke, position, font_size, lines, show_sections, color, background_color, background_opacity, highlight_color, offset_seconds}. background: {image_path | video_path | image_url | video_url | type: random}. album_art_path, or album_art: null for the song cover.",
                schema: || object(json!({
                    "config": { "type": "object" },
                    "effects": { "type": "object" },
                    "intensities": { "type": "object" },
                    "text_layers": { "type": "array", "items": { "type": "object" } },
                    "lyrics": { "type": "object" },
                    "background": { "type": "object" },
                    "album_art_path": { "type": "string" },
                    "album_art": { "anyOf": [{ "type": "string" }, { "type": "null" }] }
                }), &[]),
                call: |args| {
                    let mut args = args.clone();
                    // files on this computer reach the page as data URLs
                    let data_url = |path: &str| -> Result<String, String> {
                        use base64::Engine;
                        let path = PathBuf::from(path);
                        let bytes = std::fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
                        let kind = match path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_lowercase().as_str() {
                            "jpg" | "jpeg" => "image/jpeg",
                            "png" => "image/png",
                            "webp" => "image/webp",
                            "gif" => "image/gif",
                            "mp4" => "video/mp4",
                            "webm" => "video/webm",
                            "mov" => "video/quicktime",
                            other => return Err(format!("{other} files cannot be a background or a cover")),
                        };
                        Ok(format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
                    };
                    if let Some(background) = args.get_mut("background").and_then(Value::as_object_mut) {
                        if let Some(path) = background.remove("image_path").and_then(|value| value.as_str().map(str::to_string)) {
                            background.insert("image".into(), data_url(&path)?.into());
                        }
                        if let Some(path) = background.remove("video_path").and_then(|value| value.as_str().map(str::to_string)) {
                            background.insert("video".into(), data_url(&path)?.into());
                        }
                        if let Some(url) = background.remove("image_url") {
                            background.insert("image".into(), url);
                        }
                        if let Some(url) = background.remove("video_url") {
                            background.insert("video".into(), url);
                        }
                    }
                    if let Some(path) = args.get("album_art_path").and_then(Value::as_str).map(str::to_string) {
                        args["album_art"] = data_url(&path)?.into();
                        args.as_object_mut().map(|fields| fields.remove("album_art_path"));
                    }
                    window("video_set", &args, 30)
                },
            },
            Tool {
                name: "video_render",
                description: "Render the open video editor's clip to MP4. It runs in the window and returns at once; video_get shows the progress; export.saved names the file on this computer when it is done, export.error says why it failed.",
                schema: || object(json!({ "name": { "type": "string", "description": "file name, the song title by default" } }), &[]),
                call: |args| window("video_render", args, 15),
            },
            Tool {
                name: "video_play",
                description: "Play the clip's preview in the video editor.",
                schema: nothing,
                call: |args| window("video_play", args, 15),
            },
            Tool {
                name: "video_pause",
                description: "Pause the clip's preview.",
                schema: nothing,
                call: |args| window("video_pause", args, 15),
            },
            Tool {
                name: "video_seek",
                description: "Move the clip's preview to a position in seconds, to look at a frame with ui_screenshot.",
                schema: || object(json!({ "seconds": { "type": "number" } }), &["seconds"]),
                call: |args| window("video_seek", args, 15),
            },
            Tool {
                name: "video_close",
                description: "Close the video editor.",
                schema: nothing,
                call: |args| window("video_close", args, 15),
            },
            // ---------------------------------------------------------------- songs
            Tool {
                name: "song_create",
                description: "Make a song with ACE-Step. caption: the sound in English - comma-separated tags, a few plain sentences as ACE-Step's own examples are written, or both: genre, mood, instruments, vocals, timbre, production - never tempo, key or length (writing_guide topic caption). lyrics: sections tagged [Verse 1], [Chorus], [Bridge]... each tag alone on its line, a blank line between sections; exactly [Instrumental] for an instrumental; empty lets the engine's language model write them. Metadata goes in its own fields; what is left out the language model fills in when think is on. task_type other than text2music works on source_song_id (a library song): cover, cover-nofsq (remix), repaint (repainting_start/end seconds), lego/extract/complete (track, base model). adapters: installed LoRA ids with strengths (see lora_list), and the LoRA's trigger word in the caption. Returns a job; studio_wait with its job_id, the songs then are in the library.",
                schema: || object(json!({
                    "caption": { "type": "string" },
                    "lyrics": { "type": "string" },
                    "title": { "type": "string" },
                    "vocal_language": { "type": "string", "description": "en, ru, zh, ja, ko, es, fr, de, it, pt ... or unknown" },
                    "duration": { "type": "number", "description": "seconds, up to 600; left out, the language model decides" },
                    "bpm": { "type": "integer" },
                    "keyscale": { "type": "string", "description": "\"A minor\", \"F# major\"" },
                    "timesignature": { "type": "string", "enum": ["2", "3", "4", "6"] },
                    "task_type": { "type": "string", "enum": ["text2music", "cover", "cover-nofsq", "repaint", "lego", "extract", "complete"] },
                    "source_song_id": { "type": "string" },
                    "reference_song_id": { "type": "string", "description": "a library song whose timbre the render follows" },
                    "track": { "type": "string", "description": "lego/extract: one stem; complete: stems joined by \" | \"" },
                    "audio_cover_strength": { "type": "number" },
                    "cover_noise_strength": { "type": "number" },
                    "repainting_start": { "type": "number" },
                    "repainting_end": { "type": "number" },
                    "think": { "type": "boolean", "description": "let the language model plan the song first (default true)" },
                    "use_cot_caption": { "type": "boolean" },
                    "inference_steps": { "type": "integer" },
                    "guidance_scale": { "type": "number", "description": "CFG, for SFT/base models; turbo ignores it" },
                    "shift": { "type": "number" },
                    "solver": { "type": "string" },
                    "scheduler": { "type": "string" },
                    "guidance": { "type": "string", "enum": ["apg", "adg", "cfg_pp", "dynamic_cfg", "rescaled_cfg", "cfg_zero_star", "smc_cfg", "cfg_mp"] },
                    "cfg_interval_start": { "type": "number" },
                    "cfg_interval_end": { "type": "number" },
                    "seed": { "type": "integer" },
                    "lm_seed": { "type": "integer" },
                    "lm_temperature": { "type": "number" },
                    "lm_cfg_scale": { "type": "number" },
                    "lm_top_p": { "type": "number" },
                    "lm_top_k": { "type": "integer" },
                    "lm_negative_prompt": { "type": "string" },
                    "lm_batch_size": { "type": "integer", "description": "songs planned from the request (1 by default)" },
                    "synth_batch_size": { "type": "integer", "description": "takes rendered of each plan (1 by default); songs x takes up to 9" },
                    "audio_codes": { "type": "string" },
                    "synth_model": { "type": "string", "description": "a DiT file for this song only, from song_defaults" },
                    "lm_model": { "type": "string" },
                    "adapters": { "type": "array", "items": { "type": "object", "properties": { "id": { "type": "string" }, "scales": { "type": "object", "description": "slot -> strength" } }, "required": ["id"] } },
                    "adapter_group_scales": { "type": "object", "description": "self_attn, cross_attn, mlp, cond_embed, time_embed, proj_in -> strength of every LoRA there" },
                    "output_format": { "type": "string", "enum": ["mp3", "wav16", "wav24", "wav32", "flac"] },
                    "mp3_bitrate": { "type": "integer" },
                    "peak_clip": { "type": "integer" },
                    "fade_in": { "type": "number" },
                    "fade_out": { "type": "number" },
                    "cover_prompt": { "type": "string", "description": "what the cover should show; drawn only when an image model is set up" }
                }), &["caption"]),
                call: |args| post("/v1/music/jobs".into(), args.clone()),
            },
            Tool {
                name: "song_plan",
                description: "ACE-Step's own language model writes on the card, nothing leaves the machine. mode inspire: request.caption is a one-line idea, the answer is a whole request - caption, lyrics, bpm, keyscale, timesignature, duration, vocal_language. mode format: request carries caption and lyrics, the answer completes the metadata and tidies them. Returns {plan}; song_create takes it as it is.",
                schema: || object(json!({
                    "mode": { "type": "string", "enum": ["inspire", "format"] },
                    "request": { "type": "object", "description": "caption, lyrics, vocal_language, bpm, keyscale, timesignature, duration, lm_model" }
                }), &["mode", "request"]),
                call: |args| post("/v1/ace/plan".into(), args.clone()),
            },
            Tool {
                name: "song_understand",
                description: "The engine listens to a library song: returns {request} with its caption, lyrics, metadata and the audio codes that let a new render follow its structure (as audio_codes in song_create).",
                schema: || id_only("song_id", "the library song to listen to"),
                call: |args| post("/v1/ace/understand".into(), args.clone()),
            },
            Tool {
                name: "song_job_get",
                description: "A song job in short: status, phase, title, seeds, LoRA and, when completed, the library songs it made. response_format detailed gives the whole request.",
                schema: || object(json!({ "job_id": { "type": "string", "description": "from song_create or song_replay" }, "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every field" } }), &["job_id"]),
                call: |args| get(format!("/v1/music/jobs/{}", segment(&text(args, "job_id")?))),
            },
            Tool {
                name: "song_jobs_list",
                description: "Song jobs queued or running now.",
                schema: || object(json!({ "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every field" } }), &[]),
                call: |_| get("/v1/music/jobs".into()),
            },
            Tool {
                name: "song_job_cancel",
                description: "Stop a queued or running song job.",
                schema: || id_only("job_id", "the job to stop"),
                call: |args| post(format!("/v1/music/jobs/{}", segment(&text(args, "job_id")?)), json!({})),
            },
            Tool {
                name: "song_replay",
                description: "Render a library song again from its saved audio codes - the same plan, without the language model - with other steps, a new seed, another format, or any other request field in changes (guidance_scale, solver, scheduler, synth_model...).",
                schema: || object(json!({ "song_id": { "type": "string" }, "steps": { "type": "integer" }, "seed": { "type": "integer" }, "output_format": { "type": "string" }, "changes": { "type": "object", "description": "request field -> value" } }), &["song_id"]),
                call: |args| post("/v1/music/replay".into(), args.clone()),
            },
            // ---------------------------------------------------------------- how to write for the model
            Tool {
                name: "writing_guide",
                description: "How ACE-Step wants to be written for - the studio's song-writer contract its assistant follows. Read it before writing a caption or lyrics yourself. topic: song, caption, lyrics, transcript, sections; none lists them.",
                schema: || object(json!({ "topic": { "type": "string", "enum": ["song", "caption", "lyrics", "transcript", "sections"] } }), &[]),
                call: |args| get(format!("/v1/writing/guide?topic={}", segment(args.get("topic").and_then(Value::as_str).unwrap_or_default()))),
            },
            Tool {
                name: "writing_examples",
                description: "Official ACE-Step example requests (caption and lyrics) closest to a brief such as 'russian folk rock with accordion': write in their shape and density.",
                schema: || id_only("brief", "the idea, genre or mood"),
                call: |args| get(format!("/v1/writing/examples?brief={}", segment(&text(args, "brief")?))),
            },
            // ---------------------------------------------------------------- the writing assistant
            Tool {
                name: "assistant_sections",
                description: "Lay lyrics out in tagged sections (verse, chorus, bridge...) with their words untouched - the form's tag button. Tags already in the text are replaced. Returns the tagged lyrics.",
                schema: || object(json!({ "lyrics": { "type": "string" } }), &["lyrics"]),
                call: |args| post("/v1/assistant/sections".into(), args.clone()),
            },
            Tool {
                name: "assistant_write",
                description: "The studio's writing assistant, following its ACE-Step song-writer contract. target: all (caption, lyrics, bpm, keyscale, timesignature, duration_seconds, title and cover prompt from an idea in description), lyrics (rewrite the lyrics to fit the caption), prompt (write the caption and metadata for the lyrics), transcript (lay out recognised words as a lyric sheet). Returns a draft whose fields go into song_create as they are (duration_seconds is its duration).",
                schema: || object(json!({
                    "target": { "type": "string", "enum": ["all", "lyrics", "prompt", "transcript"] },
                    "description": { "type": "string", "description": "The idea, or the text to work on" },
                    "instruction": { "type": "string" },
                    "lyrics": { "type": "string" },
                    "caption": { "type": "string" },
                    "vocal_language": { "type": "string" },
                    "duration_seconds": { "type": "number" },
                    "instrumental": { "type": "boolean" }
                }), &["target"]),
                call: |args| post("/v1/assistant/write".into(), args.clone()),
            },
            Tool {
                name: "assistant_status",
                description: "Which writing assistant is set up (local model or OpenRouter) and whether it is running.",
                schema: nothing,
                call: |_| get("/v1/assistant/status".into()),
            },
            // ---------------------------------------------------------------- the library
            Tool {
                name: "library_songs_list",
                description: "The songs in the library, newest first: id, title, when made, made_from and made_by for a track a tool made from another (stems, processing, replay), and the start of the caption. query filters by title or caption; library_song_get gives one song whole. response_format detailed gives every field of every song.",
                schema: || object(json!({ "query": { "type": "string" }, "response_format": { "type": "string", "enum": ["concise", "detailed"] } }), &[]),
                call: |_| get("/v1/library/songs".into()),
            },
            Tool {
                name: "library_song_get",
                description: "One library song with its caption, lyrics, settings and metadata (derived: what it was made from). Its audio codes and replay request only with response_format detailed.",
                schema: || object(json!({ "song_id": { "type": "string", "description": "library song id" }, "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every field" } }), &["song_id"]),
                call: |args| get(format!("/v1/library/songs/{}", segment(&text(args, "song_id")?))),
            },
            Tool {
                name: "library_song_update",
                description: "Change a library song: send the song as library_song_get returns it with your changes (title, caption, lyrics, metadata).",
                schema: || object(json!({ "song_id": { "type": "string" }, "song": { "type": "object" } }), &["song_id", "song"]),
                call: |args| send(Method::PUT, format!("/v1/library/songs/{}", segment(&text(args, "song_id")?)), args.get("song").cloned().unwrap_or_default()),
            },
            Tool {
                name: "library_song_delete",
                description: "Remove a song from the library.",
                schema: || id_only("song_id", "library song id"),
                call: |args| send(Method::DELETE, format!("/v1/library/songs/{}", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "library_import_audio",
                description: "Add an audio file from this computer to the library.",
                schema: || object(json!({ "path": { "type": "string" }, "title": { "type": "string" } }), &["path"]),
                call: |args| {
                    let path = PathBuf::from(text(args, "path")?);
                    let title = args.get("title").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default());
                    let name = file_name(&path);
                    Ok(Call { method: Method::POST, path: "/v1/library/import".into(), payload: Payload::Form { fields: vec![("title".into(), title)], files: vec![("audio".into(), path, name)] } })
                },
            },
            Tool {
                name: "playlist_list",
                description: "The library's playlists.",
                schema: nothing,
                call: |_| get("/v1/library/playlists".into()),
            },
            Tool {
                name: "playlist_create",
                description: "Make a playlist of library songs.",
                schema: || object(json!({ "name": { "type": "string" }, "description": { "type": "string" }, "song_ids": { "type": "array", "items": { "type": "string" } } }), &["name"]),
                call: |args| post("/v1/library/playlists".into(), args.clone()),
            },
            Tool {
                name: "playlist_update",
                description: "Rename a playlist or set its songs.",
                schema: || object(json!({ "playlist_id": { "type": "string" }, "name": { "type": "string" }, "description": { "type": "string" }, "song_ids": { "type": "array", "items": { "type": "string" } } }), &["playlist_id", "name"]),
                call: |args| send(Method::PUT, format!("/v1/library/playlists/{}", segment(&text(args, "playlist_id")?)), body_without(args, &["playlist_id"])),
            },
            Tool {
                name: "playlist_delete",
                description: "Remove a playlist; its songs stay in the library.",
                schema: || id_only("playlist_id", "playlist id"),
                call: |args| send(Method::DELETE, format!("/v1/library/playlists/{}", segment(&text(args, "playlist_id")?)), json!({})),
            },
            // ---------------------------------------------------------------- covers, karaoke, stems, processing
            Tool {
                name: "cover_draw",
                description: "Draw a library song's cover now, from its cover prompt or the cover template.",
                schema: || id_only("song_id", "library song id"),
                call: |args| post(format!("/v1/library/songs/{}/cover/auto", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "cover_templates_get",
                description: "The cover prompt templates, with {title}, {style} and {excerpt} placeholders.",
                schema: nothing,
                call: |_| get("/v1/cover-templates".into()),
            },
            Tool {
                name: "cover_templates_set",
                description: "Save the cover templates (the object as cover_templates_get returns it, with your changes).",
                schema: || json!({ "type": "object" }),
                call: |args| send(Method::PUT, "/v1/cover-templates".into(), args.clone()),
            },
            Tool {
                name: "karaoke_make",
                description: "Time every word of a library song's lyrics against its vocals (enhanced LRC). language helps the recogniser.",
                schema: || object(json!({ "song_id": { "type": "string" }, "language": { "type": "string" } }), &["song_id"]),
                call: |args| post(format!("/v1/library/songs/{}/karaoke", segment(&text(args, "song_id")?)), body_without(args, &["song_id"])),
            },
            Tool {
                name: "stems_split",
                description: "Split a library song into six stems (drums, bass, other, vocals, guitar, piano) with HT-Demucs. Each stem becomes a track of the library made from the song (made_from, made_by stems), replacing the stems of an earlier split; stems_get names them. Wait with studio_wait until stems.",
                schema: || id_only("song_id", "library song id"),
                call: |args| post(format!("/v1/library/songs/{}/stems", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "stems_get",
                description: "The stems of a library song and the progress of a split.",
                schema: || id_only("song_id", "library song id"),
                call: |args| get(format!("/v1/library/songs/{}/stems", segment(&text(args, "song_id")?))),
            },
            // ---------------------------------------------------------------- audio to MIDI
            Tool {
                name: "midi_status",
                description: "Audio to MIDI (MuScriptor, weights CC BY-NC 4.0 - non-commercial): whether the transcriber is installed, the model sizes (small 103M, medium 307M, large 1.4B) with what each still has to download, the download at work, and the transcription at work or the last one with its progress and file.",
                schema: nothing,
                call: |_| get("/v1/midi".into()),
            },
            Tool {
                name: "midi_transcribe",
                description: "Turn a library track - a song, a stem, a processed take - or any audio file on this computer into multi-instrument MIDI (34 instrument groups and drums). Give song_id or path; size small, medium (default) or large. What the transcriber needs is downloaded first when it is missing. Wait with studio_wait until midi; a track's MIDI is then kept beside it (midi_get, library_song_files), a file's in the studio's midi folder (midi_status run.file).",
                schema: || object(json!({ "song_id": { "type": "string" }, "path": { "type": "string", "description": "an audio file on this computer, instead of song_id" }, "size": { "type": "string", "enum": ["small", "medium", "large"] } }), &[]),
                call: |args| post("/v1/midi/transcribe".into(), args.clone()),
            },
            Tool {
                name: "midi_get",
                description: "A library track's MIDI: the file on this computer, the model size, when it was made, its instruments and how many notes. response_format detailed adds every note (pitch, start and end in seconds, instrument).",
                schema: || object(json!({ "song_id": { "type": "string" }, "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every note" } }), &["song_id"]),
                call: |args| get(format!("/v1/library/songs/{}/midi", segment(&text(args, "song_id")?))),
            },
            Tool {
                name: "midi_delete",
                description: "Remove a library track's MIDI.",
                schema: || id_only("song_id", "library song id"),
                call: |args| send(Method::DELETE, format!("/v1/library/songs/{}/midi", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "midi_install",
                description: "Download the transcriber and a model size ahead of the first transcription (it also happens by itself on first use). midi_status shows the progress.",
                schema: || object(json!({ "size": { "type": "string", "enum": ["small", "medium", "large"] } }), &[]),
                call: |args| post("/v1/midi/install".into(), args.clone()),
            },
            Tool {
                name: "midi_remove",
                description: "Delete a model size's weights; the transcriber and the MIDI files stay.",
                schema: || object(json!({ "size": { "type": "string", "enum": ["small", "medium", "large"] } }), &["size"]),
                call: |args| post("/v1/midi/remove".into(), args.clone()),
            },
            Tool {
                name: "midi_cancel",
                description: "Stop the transcription at work, or the download it waits for.",
                schema: nothing,
                call: |_| post("/v1/midi/cancel".into(), json!({})),
            },
            Tool {
                name: "processing_start",
                description: "Run audio processing on a library song: any of denoise, lifter (Spectral Lifter), naturalize (vocal naturaliser), vst (plugin chain), master (to a reference). Each is an object of its settings; processing_get shows the result, then processing_keep or processing_discard.",
                schema: || object(json!({ "song_id": { "type": "string" }, "denoise": { "type": "object" }, "lifter": { "type": "object" }, "naturalize": { "type": "object" }, "vst": { "type": "array" }, "master": { "type": "object" } }), &["song_id"]),
                call: |args| post(format!("/v1/library/songs/{}/process", segment(&text(args, "song_id")?)), body_without(args, &["song_id"])),
            },
            Tool {
                name: "processing_get",
                description: "The processing at work or its finished result, waiting to be kept or discarded.",
                schema: nothing,
                call: |_| get("/v1/processing".into()),
            },
            Tool {
                name: "processing_keep",
                description: "Keep the processed audio as a new track of the library, made from the one processed: it names that track and the processing with its settings (metadata.derived), and wears its cover.",
                schema: || object(json!({ "label": { "type": "string", "description": "what the processing is called on the new track; its stages when left out" } }), &[]),
                call: |args| post("/v1/processing/keep".into(), args.clone()),
            },
            Tool {
                name: "processing_discard",
                description: "Throw the processed audio away.",
                schema: nothing,
                call: |_| post("/v1/processing/discard".into(), json!({})),
            },
            // ---------------------------------------------------------------- LoRA
            Tool {
                name: "lora_list",
                description: "Installed LoRA (id, name, trigger word, slots, default strengths) and the catalogue of ready ones in one line each, with download progress. Pass an installed id with a strength per slot in song_create adapters. response_format detailed gives every field in every language.",
                schema: || object(json!({ "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every field" } }), &[]),
                call: |_| get("/v1/adapters".into()),
            },
            Tool {
                name: "lora_install_catalog",
                description: "Download LoRA from the studio's catalogue by id.",
                schema: || object(json!({ "ids": { "type": "array", "items": { "type": "string" } } }), &["ids"]),
                call: |args| post("/v1/adapters/install".into(), args.clone()),
            },
            Tool {
                name: "lora_search_hf",
                description: "Search Hugging Face for LoRA for this model.",
                schema: || object(json!({ "q": { "type": "string" } }), &[]),
                call: |args| get(format!("/v1/adapters/hub?q={}", segment(args.get("q").and_then(Value::as_str).unwrap_or_default()))),
            },
            Tool {
                name: "lora_hf_files",
                description: "The adapter files in one Hugging Face repository.",
                schema: || id_only("repo", "owner/name"),
                call: |args| get(format!("/v1/adapters/hub/files?repo={}", segment(&text(args, "repo")?))),
            },
            Tool {
                name: "lora_install_hf",
                description: "Download adapter files from a Hugging Face repository into the studio.",
                schema: || object(json!({ "repo": { "type": "string" }, "paths": { "type": "array", "items": { "type": "string" } } }), &["repo", "paths"]),
                call: |args| post("/v1/adapters/hub/install".into(), args.clone()),
            },
            Tool {
                name: "lora_update",
                description: "Rename an installed LoRA, set its trigger word or its default strengths per slot.",
                schema: || object(json!({ "lora_id": { "type": "string" }, "name": { "type": "string" }, "trigger": { "type": "string" }, "scales": { "type": "object" } }), &["lora_id"]),
                call: |args| send(Method::PATCH, format!("/v1/adapters/{}", segment(&text(args, "lora_id")?)), body_without(args, &["lora_id"])),
            },
            Tool {
                name: "lora_delete",
                description: "Remove an installed LoRA.",
                schema: || id_only("lora_id", "installed LoRA id"),
                call: |args| send(Method::DELETE, format!("/v1/adapters/{}", segment(&text(args, "lora_id")?)), json!({})),
            },
            // ---------------------------------------------------------------- everything else the page does
            Tool {
                name: "models_cancel_download",
                description: "Stop the model download at work.",
                schema: nothing,
                call: |_| post("/setup/cancel".into(), json!({})),
            },
            Tool {
                name: "models_remove",
                description: "Delete downloaded model files by component id (see models_catalog).",
                schema: || object(json!({ "ids": { "type": "array", "items": { "type": "string" } } }), &["ids"]),
                call: |args| post("/setup/remove".into(), args.clone()),
            },
            Tool {
                name: "engine_logs",
                description: "The music engine's recent log, for when a song fails or the engine will not start.",
                schema: nothing,
                call: |_| get("/v1/engine/logs".into()),
            },
            Tool {
                name: "studio_capabilities",
                description: "What the studio can do with the installed models and providers, and which provider runs each capability.",
                schema: nothing,
                call: |_| get("/v1/capabilities".into()),
            },
            Tool {
                name: "settings_get",
                description: "The studio's settings: which provider (local or OpenRouter) runs music, lyrics timing, the assistant and covers, and their models.",
                schema: nothing,
                call: |_| get("/v1/configuration".into()),
            },
            Tool {
                name: "settings_set",
                description: "Save the studio's settings: the object as settings_get returns it, with your changes.",
                schema: || json!({ "type": "object" }),
                call: |args| send(Method::PUT, "/v1/configuration".into(), args.clone()),
            },
            Tool {
                name: "studio_open_data_folder",
                description: "Open the studio's data folder (models, songs, settings) in the file explorer for the user.",
                schema: nothing,
                call: |_| post("/v1/open-data-directory".into(), json!({})),
            },
            Tool {
                name: "assistant_set",
                description: "Choose the writing assistant: provider none, local (an OpenAI-compatible server at local_base_url with local_model), managed (a model the studio runs: managed_model, or a GGUF at managed_path) or openrouter (openrouter_model); reasoning_effort off, minimal, low, medium or high.",
                schema: || object(json!({ "provider": { "type": "string", "enum": ["none", "local", "managed", "openrouter"] }, "local_base_url": { "type": "string" }, "local_model": { "type": "string" }, "managed_model": { "type": "string" }, "managed_path": { "type": "string" }, "openrouter_model": { "type": "string" }, "reasoning_effort": { "type": "string" } }), &[]),
                call: |args| send(Method::PUT, "/v1/assistant/status".into(), args.clone()),
            },
            Tool {
                name: "assistant_local_models",
                description: "The models an OpenAI-compatible local server (LM Studio, Ollama...) at base offers.",
                schema: || id_only("base", "the server's base URL, e.g. http://127.0.0.1:1234/v1"),
                call: |args| get(format!("/v1/assistant/local-models?base={}", segment(&text(args, "base")?))),
            },
            Tool {
                name: "assistant_runtime",
                description: "The assistant models the studio can run itself: what is downloaded, what is running, download progress.",
                schema: nothing,
                call: |_| get("/v1/assistant/runtime".into()),
            },
            Tool {
                name: "assistant_model_install",
                description: "Download a managed assistant model or its runtime (asset_id and model_id from assistant_runtime).",
                schema: || object(json!({ "asset_id": { "type": "string" }, "model_id": { "type": "string" } }), &["asset_id"]),
                call: |args| post("/v1/assistant/runtime/install".into(), args.clone()),
            },
            Tool {
                name: "assistant_model_remove",
                description: "Delete a downloaded assistant model or runtime.",
                schema: || object(json!({ "asset_id": { "type": "string" }, "model_id": { "type": "string" } }), &["asset_id"]),
                call: |args| post("/v1/assistant/runtime/remove".into(), args.clone()),
            },
            Tool {
                name: "assistant_cancel_download",
                description: "Stop the assistant model download at work.",
                schema: nothing,
                call: |_| post("/v1/assistant/runtime/cancel".into(), json!({})),
            },
            Tool {
                name: "assistant_start",
                description: "Load a managed assistant model on the card now (it also starts by itself on first use).",
                schema: || object(json!({ "model_id": { "type": "string" }, "model_path": { "type": "string" } }), &["model_id"]),
                call: |args| post("/v1/assistant/runtime/start".into(), args.clone()),
            },
            Tool {
                name: "assistant_stop",
                description: "Unload the managed assistant model and free its video memory.",
                schema: nothing,
                call: |_| post("/v1/assistant/runtime/stop".into(), json!({})),
            },
            Tool {
                name: "karaoke_settings_get",
                description: "Lyrics timing and recognition: which recogniser (parakeet, whisper, openrouter), its model, on card or processor, what is downloaded.",
                schema: || object(json!({ "response_format": { "type": "string", "enum": ["concise", "detailed"], "description": "detailed gives every file of every model" } }), &[]),
                call: |_| get("/v1/karaoke/status".into()),
            },
            Tool {
                name: "karaoke_settings_set",
                description: "Choose the recogniser used for karaoke and for dataset lyrics: enabled, provider (parakeet, whisper, openrouter), whisper_model, openrouter_model, runtime (cpu, cuda).",
                schema: || object(json!({ "enabled": { "type": "boolean" }, "provider": { "type": "string" }, "whisper_model": { "type": "string" }, "openrouter_model": { "type": "string" }, "runtime": { "type": "string" } }), &[]),
                call: |args| send(Method::PUT, "/v1/karaoke/status".into(), args.clone()),
            },
            Tool {
                name: "recogniser_install",
                description: "Download a speech recogniser or its runtime (asset_id and model_id from karaoke_settings_get).",
                schema: || object(json!({ "asset_id": { "type": "string" }, "model_id": { "type": "string" } }), &["asset_id"]),
                call: |args| post("/v1/karaoke/install".into(), args.clone()),
            },
            Tool {
                name: "recogniser_remove",
                description: "Delete a downloaded speech recogniser or runtime.",
                schema: || object(json!({ "asset_id": { "type": "string" }, "model_id": { "type": "string" } }), &["asset_id"]),
                call: |args| post("/v1/karaoke/remove".into(), args.clone()),
            },
            Tool {
                name: "recogniser_cancel_download",
                description: "Stop the recogniser download at work.",
                schema: nothing,
                call: |_| post("/v1/karaoke/cancel".into(), json!({})),
            },
            Tool {
                name: "karaoke_delete",
                description: "Remove a library song's karaoke timings.",
                schema: || id_only("song_id", "library song id"),
                call: |args| send(Method::DELETE, format!("/v1/library/songs/{}/karaoke", segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "separator_status",
                description: "The stem separator: whether its model and runtime are installed, and download progress.",
                schema: nothing,
                call: |_| get("/v1/separation/status".into()),
            },
            Tool {
                name: "separator_runtime",
                description: "The ONNX runtimes the separator can use (card or processor) and what is downloaded.",
                schema: nothing,
                call: |_| get("/v1/separation/runtime".into()),
            },
            Tool {
                name: "separator_runtime_install",
                description: "Download a runtime for the separator (asset_id from separator_runtime).",
                schema: || id_only("asset_id", "runtime asset id"),
                call: |args| post("/v1/separation/runtime/install".into(), args.clone()),
            },
            Tool {
                name: "separator_cancel_download",
                description: "Stop the separator runtime download at work.",
                schema: nothing,
                call: |_| post("/v1/separation/runtime/cancel".into(), json!({})),
            },
            Tool {
                name: "separator_install",
                description: "Download the HT-Demucs stem model.",
                schema: nothing,
                call: |_| post("/v1/separation/install".into(), json!({})),
            },
            Tool {
                name: "separator_remove",
                description: "Delete the HT-Demucs stem model.",
                schema: nothing,
                call: |_| post("/v1/separation/remove".into(), json!({})),
            },
            Tool {
                name: "separator_settings_get",
                description: "Separator settings: runtime (card or processor) and overlap.",
                schema: nothing,
                call: |_| get("/v1/separation/settings".into()),
            },
            Tool {
                name: "separator_settings_set",
                description: "Save separator settings: the object as separator_settings_get returns it, with your changes.",
                schema: || json!({ "type": "object" }),
                call: |args| send(Method::PUT, "/v1/separation/settings".into(), args.clone()),
            },
            Tool {
                name: "library_song_files",
                description: "Where a library song's files are on this computer: its audio, its cover, each stem and its MIDI; read or open them directly.",
                schema: || id_only("song_id", "library song id"),
                call: |args| get(format!("/v1/library/songs/{}/files", segment(&text(args, "song_id")?))),
            },
            Tool {
                name: "cover_set_from_file",
                description: "Make an image file on this computer (PNG, JPEG, WebP) a library song's cover.",
                schema: || object(json!({ "song_id": { "type": "string" }, "path": { "type": "string" } }), &["song_id", "path"]),
                call: |args| {
                    use base64::Engine;
                    let path = PathBuf::from(text(args, "path")?);
                    let bytes = std::fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
                    let media_type = match path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_lowercase().as_str() {
                        "jpg" | "jpeg" => "image/jpeg",
                        "webp" => "image/webp",
                        _ => "image/png",
                    };
                    send(Method::PUT, format!("/v1/library/songs/{}/cover", segment(&text(args, "song_id")?)), json!({ "image_base64": base64::engine::general_purpose::STANDARD.encode(bytes), "media_type": media_type }))
                },
            },
            Tool {
                name: "library_version_select",
                description: "Make one of a song's audio versions (the original, a processed one) the one it plays.",
                schema: || object(json!({ "song_id": { "type": "string" }, "version": { "type": "string" } }), &["song_id", "version"]),
                call: |args| send(Method::PUT, format!("/v1/library/songs/{}/version", segment(&text(args, "song_id")?)), json!({ "version": text(args, "version")? })),
            },
            Tool {
                name: "library_version_delete",
                description: "Remove one audio version of a song.",
                schema: || object(json!({ "song_id": { "type": "string" }, "version": { "type": "string" } }), &["song_id", "version"]),
                call: |args| send(Method::DELETE, format!("/v1/library/songs/{}/versions/{}", segment(&text(args, "song_id")?), segment(&text(args, "version")?)), json!({})),
            },
            Tool {
                name: "cover_prompt_render",
                description: "Fill a cover template for a song, to see the prompt a cover would be drawn from.",
                schema: || object(json!({ "template": { "type": "string" }, "song_id": { "type": "string" }, "title": { "type": "string" } }), &["template"]),
                call: |args| post("/v1/cover-templates/render".into(), args.clone()),
            },
            Tool {
                name: "openrouter_status",
                description: "Whether an OpenRouter key is set, and where it comes from.",
                schema: nothing,
                call: |_| get("/v1/openrouter/settings".into()),
            },
            Tool {
                name: "openrouter_set_key",
                description: "Store the user's OpenRouter API key (empty to remove it).",
                schema: || object(json!({ "api_key": { "type": "string" } }), &["api_key"]),
                call: |args| send(Method::PUT, "/v1/openrouter/settings".into(), args.clone()),
            },
            Tool {
                name: "openrouter_catalog",
                description: "OpenRouter's live model catalogue, by capability: music, transcription, text, images.",
                schema: nothing,
                call: |_| get("/v1/openrouter/catalog".into()),
            },
            Tool {
                name: "openrouter_catalog_refresh",
                description: "Fetch OpenRouter's model catalogue again.",
                schema: nothing,
                call: |_| post("/v1/openrouter/catalog/refresh".into(), json!({})),
            },
            Tool {
                name: "openrouter_log",
                description: "What was asked of OpenRouter and what came back, newest last.",
                schema: nothing,
                call: |_| get("/v1/openrouter/logs".into()),
            },
            Tool {
                name: "openrouter_complete",
                description: "Ask an OpenRouter text model a prompt.",
                schema: || object(json!({ "model_id": { "type": "string" }, "prompt": { "type": "string" } }), &["model_id", "prompt"]),
                call: |args| post("/v1/openrouter/completions".into(), args.clone()),
            },
            Tool {
                name: "openrouter_cover",
                description: "Draw an image with an OpenRouter image model from a prompt.",
                schema: || object(json!({ "model_id": { "type": "string" }, "prompt": { "type": "string" } }), &["model_id", "prompt"]),
                call: |args| post("/v1/openrouter/covers".into(), args.clone()),
            },
            Tool {
                name: "openrouter_transcribe",
                description: "Transcribe an audio file on this computer with an OpenRouter speech model.",
                schema: || object(json!({ "model_id": { "type": "string" }, "path": { "type": "string" }, "language": { "type": "string" } }), &["model_id", "path"]),
                call: |args| {
                    use base64::Engine;
                    let path = PathBuf::from(text(args, "path")?);
                    let bytes = std::fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
                    let format = path.extension().and_then(|value| value.to_str()).unwrap_or("wav").to_lowercase();
                    post("/v1/openrouter/transcriptions".into(), json!({ "model_id": text(args, "model_id")?, "audio_base64": base64::engine::general_purpose::STANDARD.encode(bytes), "audio_format": format, "language": args.get("language") }))
                },
            },
            Tool {
                name: "processing_set_reference",
                description: "Use an audio file on this computer as the reference processing_start masters to.",
                schema: || id_only("path", "reference audio file"),
                call: |args| {
                    let path = PathBuf::from(text(args, "path")?);
                    let name = file_name(&path);
                    Ok(Call { method: Method::POST, path: "/v1/processing/reference".into(), payload: Payload::Form { fields: Vec::new(), files: vec![("audio".into(), path, name)] } })
                },
            },
            Tool {
                name: "vst_list",
                description: "The VST3 plugins the studio found, for processing_start's vst chain.",
                schema: nothing,
                call: |_| get("/v1/processing/vst".into()),
            },
            Tool {
                name: "vst_scan",
                description: "Look for VST3 plugins again.",
                schema: nothing,
                call: |_| post("/v1/processing/vst/scan".into(), json!({})),
            },
            Tool {
                name: "vst_open_editor",
                description: "Open a VST3 plugin's own window for the user to set it; state_id keeps its settings for the chain.",
                schema: || object(json!({ "path": { "type": "string" }, "state_id": { "type": "string" } }), &["path"]),
                call: |args| post("/v1/processing/vst/editor".into(), args.clone()),
            },
            Tool {
                name: "lora_import_files",
                description: "Install LoRA files from this computer (.safetensors, with adapter_config.json or lora.json beside them when there are).",
                schema: || object(json!({ "name": { "type": "string" }, "paths": { "type": "array", "items": { "type": "string" } } }), &["name", "paths"]),
                call: |args| {
                    let paths: Vec<PathBuf> = args.get("paths").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).collect();
                    if paths.is_empty() {
                        return Err("'paths' is required".into());
                    }
                    let files = paths.into_iter().map(|path| { let name = file_name(&path); ("files".to_string(), path, name) }).collect();
                    Ok(Call { method: Method::POST, path: "/v1/adapters/import".into(), payload: Payload::Form { fields: vec![("name".into(), text(args, "name")?)], files } })
                },
            },
            Tool {
                name: "lora_cancel_download",
                description: "Stop the LoRA download at work.",
                schema: nothing,
                call: |_| post("/v1/adapters/cancel".into(), json!({})),
            },
            Tool {
                name: "dataset_import_folder",
                description: "Take a dataset folder another studio of the family wrote (its dataset.json and WAV files).",
                schema: || id_only("path", "the dataset folder"),
                call: |args| {
                    let files = folder_files(&PathBuf::from(text(args, "path")?), dataset_file, "dataset.json or WAV files")?;
                    Ok(Call { method: Method::POST, path: "/v1/training/datasets/import".into(), payload: Payload::Form { fields: Vec::new(), files } })
                },
            },
            Tool {
                name: "dataset_reveal",
                description: "Open a dataset's folder in the file explorer for the user.",
                schema: || id_only("dataset_id", "dataset id"),
                call: |args| post(format!("/v1/training/datasets/{}/reveal", segment(&text(args, "dataset_id")?)), json!({})),
            },
            Tool {
                name: "dataset_song_files",
                description: "Where a dataset song's audio and its separated vocals are on this computer.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_id": { "type": "string" } }), &["dataset_id", "song_id"]),
                call: |args| get(format!("/v1/training/datasets/{}/items/{}/files", segment(&text(args, "dataset_id")?), segment(&text(args, "song_id")?))),
            },
            Tool {
                name: "training_pack_cancel",
                description: "Stop the trainer download at work.",
                schema: nothing,
                call: |_| post("/v1/training/pack/cancel".into(), json!({})),
            },
            Tool {
                name: "dataset_prepare_train_after",
                description: "For the preparation at work: start this training run once every song is ready (train: {name, recipe}), or not (train: null).",
                schema: || object(json!({ "train": { "anyOf": [{ "type": "object" }, { "type": "null" }] } }), &[]),
                call: |args| post("/v1/training/prepare/train-after".into(), args.clone()),
            },
            // ---------------------------------------------------------------- training
            Tool {
                name: "training_status",
                description: "Training at a glance: every dataset with its song count and how many songs are ready, every run with its status, stage, last step, loss and checkpoints, the preparation at work, whether the trainer and the listening pack are installed, and the recipe defaults to start a run from. response_format detailed gives everything, recipe fields included.",
                schema: || object(json!({ "response_format": { "type": "string", "enum": ["concise", "detailed"] } }), &[]),
                call: |_| get("/v1/training".into()),
            },
            Tool {
                name: "dataset_get",
                description: "One dataset with every song: id, title, artist, lyrics, style (its caption), lyrics_state (wanted, found, done), style_state (wanted, done), lyrics_source and length. Give song_id for one song only.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_id": { "type": "string" } }), &["dataset_id"]),
                call: |_| get("/v1/training".into()),
            },
            Tool {
                name: "training_pack_install",
                description: "Download the trainer and its weights (needed once before training).",
                schema: nothing,
                call: |_| post("/v1/training/pack/install".into(), json!({})),
            },
            Tool {
                name: "training_listen_pack_install",
                description: "Download MOSS-Music and the tempo and key models, which caption songs by ear during preparation.",
                schema: nothing,
                call: |_| post("/v1/training/listen/install".into(), json!({})),
            },
            Tool {
                name: "dataset_create",
                description: "Make an empty dataset. The trigger word is made from the name when left out.",
                schema: || object(json!({ "name": { "type": "string" }, "trigger": { "type": "string" } }), &["name"]),
                call: |args| post("/v1/training/datasets".into(), args.clone()),
            },
            Tool {
                name: "dataset_add_folder",
                description: "Add every song under a folder on this computer to a dataset, as a dropped folder: audio, .txt/.lrc lyrics beside it, albums cut by their .cue. Titles and artists come from tags, file names and folders.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "path": { "type": "string", "description": "folder or single audio file" } }), &["dataset_id", "path"]),
                call: |args| {
                    let path = PathBuf::from(text(args, "path")?);
                    let files = if path.is_dir() { folder_files(&path, song_file, "audio")? } else { vec![("files".to_string(), path.clone(), file_name(&path))] };
                    Ok(Call { method: Method::POST, path: format!("/v1/training/datasets/{}/files", segment(&text(args, "dataset_id")?)), payload: Payload::Form { fields: Vec::new(), files } })
                },
            },
            Tool {
                name: "dataset_add_library_songs",
                description: "Add library songs to a dataset.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_ids": { "type": "array", "items": { "type": "string" } } }), &["dataset_id", "song_ids"]),
                call: |args| post(format!("/v1/training/datasets/{}/songs", segment(&text(args, "dataset_id")?)), body_without(args, &["dataset_id"])),
            },
            Tool {
                name: "dataset_update",
                description: "Rename a dataset or change its trigger word.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "name": { "type": "string" }, "trigger": { "type": "string" } }), &["dataset_id"]),
                call: |args| send(Method::PATCH, format!("/v1/training/datasets/{}", segment(&text(args, "dataset_id")?)), body_without(args, &["dataset_id"])),
            },
            Tool {
                name: "dataset_delete",
                description: "Remove a dataset and its audio.",
                schema: || id_only("dataset_id", "dataset id"),
                call: |args| send(Method::DELETE, format!("/v1/training/datasets/{}", segment(&text(args, "dataset_id")?)), json!({})),
            },
            Tool {
                name: "dataset_song_update",
                description: "Write a dataset song's title, artist, style (its caption, three parts under their headings), lyrics (sections tagged [verse], [chorus]...) or instrumental flag. What you write is final: preparation leaves it alone.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_id": { "type": "string" }, "title": { "type": "string" }, "artist": { "type": "string" }, "style": { "type": "string" }, "lyrics": { "type": "string" }, "instrumental": { "type": "boolean" } }), &["dataset_id", "song_id"]),
                call: |args| send(Method::PATCH, format!("/v1/training/datasets/{}/items/{}", segment(&text(args, "dataset_id")?), segment(&text(args, "song_id")?)), body_without(args, &["dataset_id", "song_id"])),
            },
            Tool {
                name: "dataset_song_describe",
                description: "Have the studio's writing assistant caption a dataset song from its title and lyrics, without listening - for when the listening pack is not installed. Writing the caption yourself with dataset_song_update is better.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_id": { "type": "string" } }), &["dataset_id", "song_id"]),
                call: |args| post(format!("/v1/training/datasets/{}/items/{}/describe", segment(&text(args, "dataset_id")?), segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "dataset_song_delete",
                description: "Remove a song from a dataset.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "song_id": { "type": "string" } }), &["dataset_id", "song_id"]),
                call: |args| send(Method::DELETE, format!("/v1/training/datasets/{}/items/{}", segment(&text(args, "dataset_id")?), segment(&text(args, "song_id")?)), json!({})),
            },
            Tool {
                name: "lyrics_find",
                description: "Look a song's published lyrics up in LRCLIB, QQ Music and Kugou by artist, title and length; returns the plain and timed lyrics word for word, or that it is an instrumental, or nothing.",
                schema: || object(json!({ "artist": { "type": "string" }, "title": { "type": "string" }, "seconds": { "type": "number" } }), &["title"]),
                call: |args| post("/v1/lyrics/find".into(), args.clone()),
            },
            Tool {
                name: "dataset_prepare",
                description: "Fill a dataset in by itself: lyrics from the lyric databases, Whisper only for songs they miss, sections laid out by the assistant, captions written by MOSS-Music from what it hears with the tempo and key measured. lyrics and style: missing (only what is not done yet), all (again, for the chosen songs) or none. items: song ids, all songs when left out. train: start this run once every song is ready. Watch training_status.",
                schema: || object(json!({
                    "dataset_id": { "type": "string" },
                    "items": { "type": "array", "items": { "type": "string" } },
                    "lyrics": { "type": "string", "enum": ["none", "missing", "all"] },
                    "style": { "type": "string", "enum": ["none", "missing", "all"] },
                    "language": { "type": "string", "description": "sung language code when known, e.g. ru" },
                    "train": { "type": "object", "description": "not with writer agent: start the training once you have written the songs", "properties": { "name": { "type": "string" }, "recipe": { "type": "object" } } },
                    "writer": { "type": "string", "enum": ["studio", "agent"], "description": "agent: the studio finds, recognises and captions, and leaves the lyric layout to you - songs stay lyrics_state 'found' (lyrics as found, lyrics_source says from where); lay them out with dataset_song_update, following writing_guide topics sections and transcript" }
                }), &["dataset_id"]),
                call: |args| post(format!("/v1/training/datasets/{}/prepare", segment(&text(args, "dataset_id")?)), body_without(args, &["dataset_id"])),
            },
            Tool {
                name: "dataset_prepare_cancel",
                description: "Stop the preparation at work; every song keeps what it has.",
                schema: nothing,
                call: |_| post("/v1/training/prepare/cancel".into(), json!({})),
            },
            Tool {
                name: "training_start",
                description: "Train a LoRA on a dataset. recipe: the recipe from training_status (recipe_defaults) with your changes, e.g. stop 'steps' with steps, or stop 'epochs' with epochs. Watch training_status; one run at a time, and it holds the card.",
                schema: || object(json!({ "dataset_id": { "type": "string" }, "name": { "type": "string" }, "recipe": { "type": "object" } }), &["dataset_id", "recipe"]),
                call: |args| post("/v1/training/runs".into(), args.clone()),
            },
            Tool {
                name: "training_continue",
                description: "Train a finished or stopped run further, from its latest checkpoint up to steps in all (training_status shows resume_step, or resume_refused with the reason). Same recipe and songs; the loss chart and the checkpoints go on from there. Stops by steps only.",
                schema: || object(json!({ "run_id": { "type": "string" }, "steps": { "type": "integer", "description": "the total steps to reach, above resume_step" } }), &["run_id", "steps"]),
                call: |args| post(format!("/v1/training/runs/{}/continue", segment(&text(args, "run_id")?)), json!({ "steps": args.get("steps").cloned().unwrap_or(Value::Null) })),
            },
            Tool {
                name: "training_cancel",
                description: "Stop a training run; its saved checkpoints stay.",
                schema: || id_only("run_id", "training run id"),
                call: |args| post(format!("/v1/training/runs/{}/cancel", segment(&text(args, "run_id")?)), json!({})),
            },
            Tool {
                name: "training_checkpoint_install",
                description: "Put a run's checkpoint (its step) into the LoRA library, ready to use in song_create.",
                schema: || object(json!({ "run_id": { "type": "string" }, "step": { "type": "integer" }, "name": { "type": "string" } }), &["run_id", "step"]),
                call: |args| {
                    let step = args.get("step").and_then(Value::as_u64).ok_or("'step' is required")?;
                    post(format!("/v1/training/runs/{}/checkpoints/{step}/install", segment(&text(args, "run_id")?)), body_without(args, &["run_id", "step"]))
                },
            },
            Tool {
                name: "training_run_delete",
                description: "Remove a finished training run and its checkpoints.",
                schema: || id_only("run_id", "training run id"),
                call: |args| send(Method::DELETE, format!("/v1/training/runs/{}", segment(&text(args, "run_id")?)), json!({})),
            },
        ]
    })
}

/// Builds the route's request, calls it inside the process and returns what
/// it answered, cut to a size an agent reads.
async fn call_route(call: Call) -> Result<(StatusCode, String), String> {
    let api = API.get().ok_or("the studio is still starting")?.clone();
    let builder = Request::builder().method(call.method).uri(&call.path);
    let request = match call.payload {
        Payload::None | Payload::Window { .. } => builder.body(Body::empty()),
        Payload::Json(body) => builder.header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())),
        Payload::Form { fields, files } => {
            let boundary = format!("studio-mcp-{}", uuid::Uuid::now_v7().simple());
            let mut parts: Vec<Part> = Vec::new();
            for (name, value) in fields {
                parts.push(Part::Text(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")));
            }
            for (name, path, file) in files {
                if let Err(error) = tokio::fs::metadata(&path).await {
                    return Err(format!("read {}: {error}", path.display()));
                }
                parts.push(Part::Text(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n", file.replace('"', "'"))));
                parts.push(Part::File(path));
                parts.push(Part::Text("\r\n".into()));
            }
            parts.push(Part::Text(format!("--{boundary}--\r\n")));
            builder.header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}")).body(Body::from_stream(streamed(parts)))
        }
    }
    .map_err(|error| error.to_string())?;
    let response = api.oneshot(request).await.map_err(|error| error.to_string())?;
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024).await.map_err(|error| error.to_string())?;
    let mut text = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => serde_json::to_string(&value).unwrap_or_default(),
        Err(_) => String::from_utf8_lossy(&bytes).into_owned(),
    };
    if text.trim().is_empty() {
        text = if status.is_success() { "Done.".into() } else { status.to_string() };
    }
    Ok((status, text))
}

/// A piece of a multipart body: text, or a file read as it is sent.
enum Part {
    Text(String),
    File(PathBuf),
}

/// The parts as a stream, a file a megabyte at a time: a folder of albums is
/// sent without ever being held in memory whole.
fn streamed(parts: Vec<Part>) -> impl futures_util::Stream<Item = std::io::Result<axum::body::Bytes>> {
    futures_util::stream::unfold((parts.into_iter(), None::<tokio::fs::File>), |(mut parts, mut open)| async move {
        loop {
            if let Some(file) = open.as_mut() {
                let mut chunk = vec![0u8; 1 << 20];
                match tokio::io::AsyncReadExt::read(file, &mut chunk).await {
                    Ok(0) => open = None,
                    Ok(read) => {
                        chunk.truncate(read);
                        return Some((Ok(chunk.into()), (parts, open)));
                    }
                    Err(error) => return Some((Err(error), (parts, None))),
                }
                continue;
            }
            match parts.next()? {
                Part::Text(text) => return Some((Ok(text.into()), (parts, None))),
                Part::File(path) => match tokio::fs::File::open(&path).await {
                    Ok(file) => open = Some(file),
                    Err(error) => return Some((Err(error), (parts, None))),
                },
            }
        }
    })
}

/// Cuts an answer to what an agent reads in one go, and says how to get the rest.
fn cut(mut text: String) -> String {
    if text.len() > LIMIT {
        let mut end = LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n... (cut: ask for one item, e.g. library_song_get or dataset_get with song_id)");
    }
    text
}

/// The protocol revisions the studio speaks: the stateless one, where every
/// request carries its version, and the handshake ones older clients open
/// with `initialize`.
const MODERN: &[&str] = &["2026-07-28"];
const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
/// How long a client may keep the tool list, the prompts and the guides:
/// they change only with the studio, but an update should show within minutes.
const LIST_TTL_MS: u64 = 300_000;
const HEADER_MISMATCH: i64 = -32020;
const UNSUPPORTED_VERSION: i64 = -32022;

fn server_info() -> Value {
    json!({ "name": env!("CARGO_PKG_NAME"), "title": music_core::studio().name, "version": crate::studio_version() })
}

fn supported_versions() -> Vec<&'static str> {
    MODERN.iter().chain(LEGACY).copied().collect()
}

/// A complete result, signed with the server's identity.
fn rpc(id: Value, mut result: Value) -> Response {
    if let Some(fields) = result.as_object_mut() {
        fields.entry("resultType").or_insert_with(|| "complete".into());
        let meta = fields.entry("_meta").or_insert_with(|| json!({}));
        meta["io.modelcontextprotocol/serverInfo"] = server_info();
    }
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

fn rpc_error(id: Value, code: i64, message: String) -> Response {
    rpc_failure(StatusCode::OK, id, code, message, None)
}

fn rpc_failure(status: StatusCode, id: Value, code: i64, message: String, data: Option<Value>) -> Response {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    (status, Json(json!({ "jsonrpc": "2.0", "id": id, "error": error }))).into_response()
}

/// A list or a read a client may cache: the same for everyone, fresh for five minutes.
fn cacheable(mut result: Value) -> Value {
    result["ttlMs"] = LIST_TTL_MS.into();
    result["cacheScope"] = "public".into();
    result
}

/// A header value, with the Base64 sentinel form decoded.
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    use base64::Engine;
    let raw = headers.get(name)?.to_str().ok()?;
    match raw.strip_prefix("=?base64?").and_then(|inner| inner.strip_suffix("?=")) {
        Some(encoded) => base64::engine::general_purpose::STANDARD.decode(encoded).ok().and_then(|bytes| String::from_utf8(bytes).ok()),
        None => Some(raw.to_string()),
    }
}

/// Why a stateless request is refused, if it is: a version the studio does
/// not speak, or headers that do not say what its body says.
fn refused(headers: &HeaderMap, method: &str, params: &Value, version: &str) -> Option<(i64, String, Option<Value>)> {
    if !MODERN.contains(&version) {
        return Some((UNSUPPORTED_VERSION, "Unsupported protocol version".into(), Some(json!({ "supported": supported_versions(), "requested": version }))));
    }
    let mismatch = |what: String| Some((HEADER_MISMATCH, format!("Header mismatch: {what}"), None));
    match header_value(headers, "mcp-protocol-version") {
        Some(value) if value == version => {}
        Some(value) => return mismatch(format!("MCP-Protocol-Version header value '{value}' does not match body value '{version}'")),
        None => return mismatch("the MCP-Protocol-Version header is missing".into()),
    }
    match header_value(headers, "mcp-method") {
        Some(value) if value == method => {}
        Some(value) => return mismatch(format!("Mcp-Method header value '{value}' does not match body value '{method}'")),
        None => return mismatch("the Mcp-Method header is missing".into()),
    }
    let named = match method {
        "tools/call" | "prompts/get" => params.get("name"),
        "resources/read" => params.get("uri"),
        _ => return None,
    }
    .and_then(Value::as_str)
    .unwrap_or_default();
    match header_value(headers, "mcp-name") {
        Some(value) if value == named => None,
        Some(value) => mismatch(format!("Mcp-Name header value '{value}' does not match body value '{named}'")),
        None => mismatch("the Mcp-Name header is missing".into()),
    }
}

/// A tool's answer: the text an agent reads, and the same data structured
/// when it is JSON and whole.
fn tool_result(id: Value, text: String, structured: Option<Value>, error: bool) -> Response {
    let mut result = json!({ "content": [{ "type": "text", "text": text }], "isError": error });
    if let Some(structured) = structured.filter(|value| value.is_object() || value.is_array()) {
        result["structuredContent"] = structured;
    }
    rpc(id, result)
}

fn tool_json(id: Value, value: Value) -> Response {
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    if text.len() > LIMIT {
        tool_result(id, cut(text), None, false)
    } else {
        tool_result(id, text, Some(value), false)
    }
}

/// The name a tool shows the user: its words, the first capitalised.
fn tool_title(name: &str) -> String {
    let words = name.replace('_', " ");
    let mut letters = words.chars();
    letters.next().map(|first| first.to_uppercase().chain(letters).collect()).unwrap_or_default()
}

pub async fn handle(headers: HeaderMap, body: axum::body::Bytes) -> Response {
    if !local_origin(&headers) {
        return foreign_origin();
    }
    let Ok(message) = serde_json::from_slice::<Value>(&body) else {
        return rpc_failure(StatusCode::BAD_REQUEST, Value::Null, -32700, "Parse error".into(), None);
    };
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return rpc_failure(StatusCode::BAD_REQUEST, Value::Null, -32600, "Invalid request: one JSON-RPC request or notification per POST".into(), None);
    };
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    // a notification is only acknowledged
    if message.get("id").is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    // A request of the stateless revision says its version in _meta and is
    // checked against its headers; one without it is of the handshake era.
    if let Some(version) = params.get("_meta").and_then(|meta| meta.get(META_VERSION)).and_then(Value::as_str) {
        if let Some((code, text, data)) = refused(&headers, method, &params, version) {
            return rpc_failure(StatusCode::BAD_REQUEST, id, code, text, data);
        }
    }
    if method != "tools/call" {
        seen(method);
    }
    match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let protocol = LEGACY.iter().find(|version| **version == asked).copied().unwrap_or(LEGACY[0]);
            rpc(id, json!({
                "protocolVersion": protocol,
                "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
                "serverInfo": server_info(),
                "instructions": instructions(),
            }))
        }
        "server/discover" => rpc(id, cacheable(json!({
            "supportedVersions": supported_versions(),
            "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
            "instructions": instructions(),
        }))),
        "ping" => rpc(id, json!({})),
        "resources/list" => {
            let mut list = vec![json!({ "uri": SKILL_URI, "name": "studio skill", "title": "How to drive the studio", "description": "How to drive the studio: every tool, what the model reads, step-by-step recipes.", "mimeType": "text/markdown" })];
            for (topic, about) in crate::assistant::GUIDE_TOPICS {
                list.push(json!({ "uri": format!("{GUIDE_URI}{topic}"), "name": format!("writing guide: {topic}"), "title": format!("Writing guide: {topic}"), "description": about, "mimeType": "text/plain" }));
            }
            rpc(id, cacheable(json!({ "resources": list })))
        }
        "resources/templates/list" => rpc(id, cacheable(json!({ "resourceTemplates": [] }))),
        "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or_default();
            let text = if uri == SKILL_URI { Some(SKILL.to_string()) } else { uri.strip_prefix(GUIDE_URI).and_then(crate::assistant::writing_guide) };
            match text {
                Some(text) => rpc(id, cacheable(json!({ "contents": [{ "uri": uri, "mimeType": if uri == SKILL_URI { "text/markdown" } else { "text/plain" }, "text": text }] }))),
                None => rpc_error(id, -32002, format!("Resource not found: {uri}")),
            }
        }
        "prompts/list" => rpc(id, cacheable(json!({ "prompts": [
            { "name": "studio", "title": "Studio skill", "description": "Load the studio's skill: the tools, what the model reads, and recipes." },
            { "name": "write_song", "title": "Write a song", "description": "Write a song for the model and make it.", "arguments": [{ "name": "idea", "description": "what the song is about, its genre and mood", "required": true }] },
        ] }))),
        "prompts/get" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let idea = params.get("arguments").and_then(|arguments| arguments.get("idea")).and_then(Value::as_str).unwrap_or_default();
            let text = match name {
                "studio" => Some(SKILL.to_string()),
                "write_song" => crate::assistant::writing_guide("song").map(|guide| format!("{guide}\n\nWrite a song about: {idea}\nRead the closest reference captions with writing_examples, write the caption and lyrics by the rules above, then song_create and studio_wait.")),
                _ => None,
            };
            match text {
                Some(text) => rpc(id, json!({ "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] })),
                None => rpc_error(id, -32602, format!("Unknown prompt: {name}")),
            }
        }
        "tools/list" => {
            let list: Vec<Value> = tools().iter().map(|tool| json!({ "name": tool.name, "title": tool_title(tool.name), "description": tool.description, "inputSchema": (tool.schema)(), "annotations": annotations(tool.name) })).collect();
            rpc(id, cacheable(json!({ "tools": list })))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            seen(name);
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let answer = |text: String, error: bool| tool_result(id.clone(), text, None, error);
            let Some(tool) = tools().iter().find(|tool| tool.name == name) else {
                return rpc_error(id, -32602, format!("Unknown tool: {name}; tools/list names them all."));
            };
            // a tool may read a file or walk a folder before its call: off the runtime
            let prepare = tool.call;
            let prepared = {
                let args = args.clone();
                tokio::task::spawn_blocking(move || prepare(&args)).await.unwrap_or_else(|error| Err(format!("{name} failed: {error}")))
            };
            match prepared {
                Err(problem) => answer(problem, true),
                Ok(Call { payload: Payload::Window { command, args, seconds }, .. }) => match ask_window(command, args, seconds).await {
                    Ok(result) => {
                        // a screenshot comes back as an image the agent sees
                        let mut content = Vec::new();
                        if let Some(image) = result.get("image").and_then(Value::as_str) {
                            content.push(json!({ "type": "image", "data": image, "mimeType": "image/png" }));
                        }
                        let text = result.get("text").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| if result.is_null() || result.get("image").is_some() { "Done.".into() } else { serde_json::to_string_pretty(&result).unwrap_or_default() });
                        content.push(json!({ "type": "text", "text": text }));
                        let mut answer = json!({ "content": content, "isError": false });
                        if result.get("image").is_none() && result.get("text").is_none() && (result.is_object() || result.is_array()) {
                            answer["structuredContent"] = result;
                        }
                        rpc(id.clone(), answer)
                    }
                    Err(problem) => answer(problem, true),
                },
                Ok(call) if call.path == "composite:status" => tool_json(id, status_summary().await),
                Ok(call) if call.path == "composite:wait" => match wait_for(&args).await {
                    Ok(state) => tool_json(id, state),
                    Err(problem) => answer(problem, true),
                },
                Ok(call) if call.path == "composite:questions" => tool_json(id, wait_for_questions(&args).await),
                Ok(call) if call.path == "composite:answer" => match answer_question(&args) {
                    Ok(text) => answer(text, false),
                    Err(problem) => answer(problem, true),
                },
                Ok(call) => match call_route(call).await {
                    Ok((status, text)) if status.is_success() => match serde_json::from_str::<Value>(&text) {
                        Ok(value) => tool_json(id, shape(name, &args, value)),
                        Err(_) => answer(text, false),
                    },
                    Ok((_, text)) => answer(text, true),
                    Err(problem) => answer(problem, true),
                },
            }
        }
        _ => rpc_failure(StatusCode::NOT_FOUND, id, -32601, format!("Method not found: {method}"), None),
    }
}

/// What an agent is told when it connects.
fn instructions() -> String {
    format!("You drive {} on this computer. Every tool runs the same code as a button of the studio, and the user sees what you do in its window. Start with studio_status. Long work (songs, stems, karaoke, dataset preparation, training) is a job: start it, then studio_wait instead of polling. The graphics card runs one heavy job at a time; while a LoRA trains no song is made. Look ids up instead of guessing them: library_songs_list, training_status, dataset_get, lora_list, models_status. Before writing for the model yourself read writing_guide and writing_examples. When the user has made you the studio's writing assistant, answer its requests: assistant_requests_wait, then assistant_request_answer. The whole guide is the resource studio://skill (prompt 'studio').", music_core::studio().name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_unique_name_and_an_object_schema() {
        let mut names: Vec<&str> = tools().iter().map(|tool| tool.name).collect();
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count, "two tools share a name");
        for tool in tools() {
            let schema = (tool.schema)();
            assert_eq!(schema["type"], "object", "{} has no object schema", tool.name);
            assert!(!tool.description.is_empty(), "{} has no description", tool.name);
        }
    }

    #[test]
    fn a_tool_turns_its_arguments_into_its_route() {
        let find = |name: &str| tools().iter().find(|tool| tool.name == name).expect("the tool");
        let call = (find("dataset_song_update").call)(&json!({ "dataset_id": "d 1", "song_id": "s", "lyrics": "[verse]" })).unwrap();
        assert_eq!(call.method, Method::PATCH);
        assert_eq!(call.path, "/v1/training/datasets/d%201/items/s");
        match call.payload {
            Payload::Json(body) => assert_eq!(body, json!({ "lyrics": "[verse]" })),
            _ => panic!("a JSON body"),
        }
        assert!((find("library_song_get").call)(&json!({})).is_err(), "a missing id is refused");
    }

    #[test]
    fn a_folder_becomes_its_songs_with_their_paths() {
        let root = tempfile::tempdir().unwrap();
        let album = root.path().join("Artist").join("2020 - Album");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("01. Song.flac"), b"x").unwrap();
        std::fs::write(album.join("01. Song.lrc"), b"x").unwrap();
        std::fs::write(album.join("cover.jpg"), b"x").unwrap();
        let files = folder_files(&root.path().join("Artist"), song_file, "audio").unwrap();
        let names: Vec<&str> = files.iter().map(|(_, _, name)| name.as_str()).collect();
        assert_eq!(names, ["Artist/2020 - Album/01. Song.flac", "Artist/2020 - Album/01. Song.lrc"]);
    }

    #[test]
    fn an_agent_is_answered_what_it_acts_on() {
        let job = json!({
            "id": "j", "title": "t", "status": "completed", "phase": "done", "message": "m", "duration_seconds": 60,
            "lyrics": "[Verse 1]", "generation_settings": { "seed": 1, "lm_seed": 2, "lyrics": "[Verse 1]", "adapters": [{ "name": "a" }] },
            "songs": [{ "id": "s", "audio_url": "/x", "song": { "title": "Song", "audio_codes": "1,2,3" } }],
        });
        let short = compact_job(&job);
        assert_eq!(short["songs"], json!([{ "id": "s", "title": "Song" }]));
        assert_eq!(short["seed"], 1);
        assert!(!short.to_string().contains("[Verse 1]") && !short.to_string().contains("1,2,3"), "no lyrics, no codes");

        let song = compact_song(&json!({ "id": "s", "lyrics": "[Verse 1]", "audio_codes": "1,2,3", "replay_request": { "x": 1 } }));
        assert_eq!(song["has_audio_codes"], true);
        assert!(song.get("audio_codes").is_none() && song.get("replay_request").is_none());

        let loras = compact_loras(&json!({
            "installed": [{ "id": "l", "name": "Mine", "trigger": "t", "slots": ["ar"], "scales": {}, "bytes": 5 }],
            "catalog": [{ "id": "c", "name": { "en": "Pop", "ru": "Поп" }, "description": { "en": "Hooks", "ru": "Хуки" }, "installed": false, "slots": ["ar"] }],
        }));
        assert_eq!(loras["catalog"][0]["name"], "Pop");
        assert_eq!(loras["catalog"][0]["about"], "Hooks");
        assert!(loras["installed"][0].get("bytes").is_none());
    }

    #[test]
    fn a_tool_that_changes_something_is_not_read_only() {
        for name in ["lora_install_catalog", "lora_import_files", "separator_runtime_install", "openrouter_catalog_refresh", "dataset_create"] {
            assert_eq!(annotations(name)["readOnlyHint"], false, "{name}");
        }
        for name in ["lora_list", "training_status", "dataset_song_files", "ui_screenshot", "writing_guide", "create_form_get", "song_defaults"] {
            assert_eq!(annotations(name)["readOnlyHint"], true, "{name}");
        }
        for name in ["models_download", "lora_install_catalog", "training_pack_install", "openrouter_complete"] {
            assert_eq!(annotations(name)["openWorldHint"], true, "{name}");
        }
        assert_eq!(annotations("training_checkpoint_install")["openWorldHint"], false);
    }

    #[test]
    fn idle_waits_for_every_kind_of_work() {
        let idle = json!({ "song_jobs": [], "covers_and_karaoke": [], "stems": null, "processing": { "done": true }, "preparation": null, "training": null });
        assert!(busy(&idle).is_empty());
        let mut splitting = idle.clone();
        splitting["stems"] = json!({ "song_id": "s", "progress": 0.1, "done": false });
        assert_eq!(busy(&splitting), ["stems"]);
        let mut processing = idle.clone();
        processing["processing"] = json!({ "done": false });
        assert_eq!(busy(&processing), ["processing"]);
    }

    #[test]
    fn a_dataset_folder_brings_its_json_and_wavs() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("set");
        std::fs::create_dir_all(folder.join("audio")).unwrap();
        std::fs::write(folder.join("dataset.json"), b"{}").unwrap();
        std::fs::write(folder.join("audio").join("a.WAV"), b"x").unwrap();
        std::fs::write(folder.join("audio").join("a.mp3"), b"x").unwrap();
        let files = folder_files(&folder, dataset_file, "dataset.json or WAV files").unwrap();
        let names: Vec<&str> = files.iter().map(|(_, _, name)| name.as_str()).collect();
        assert_eq!(names, ["set/audio/a.WAV", "set/dataset.json"]);
        assert!(folder_files(&folder.join("audio"), |_| false, "x").is_err(), "an empty pick is refused");
    }

    #[test]
    fn only_local_pages_and_agents_may_drive_the_studio() {
        let from = |origin: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::ORIGIN, origin.parse().unwrap());
            local_origin(&headers)
        };
        assert!(local_origin(&HeaderMap::new()), "an agent sends no origin");
        assert!(from("http://127.0.0.1:3791") && from("http://localhost") && from("http://tauri.localhost") && from("tauri://localhost") && from("http://[::1]:8791"));
        assert!(!from("https://example.com") && !from("http://127.0.0.1.evil.com") && !from("null"));
    }

    #[tokio::test]
    async fn the_agent_answers_what_the_studio_asks_its_assistant() {
        seen("studio_status");
        let asking = tokio::spawn(async { ask_agent("rules", "idea", Some(json!({ "type": "object" })), "all").await });
        let waiting = wait_for_questions(&json!({ "seconds": 5 })).await;
        let request = &waiting["requests"][0];
        assert_eq!(request["instructions"], "rules");
        assert_eq!(request["target"], "all");
        let id = request["id"].as_str().unwrap().to_string();
        answer_question(&json!({ "request_id": id, "answer": { "lyrics": "[Verse 1]" } })).unwrap();
        assert_eq!(asking.await.unwrap().unwrap(), r#"{"lyrics":"[Verse 1]"}"#);
        assert!(answer_question(&json!({ "request_id": id, "answer": "x" })).is_err(), "answered once only");
    }

    #[tokio::test]
    async fn a_multipart_body_streams_its_files() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("song.flac");
        std::fs::write(&path, vec![7u8; (1 << 20) + 5]).unwrap();
        let parts = vec![Part::Text("head".into()), Part::File(path), Part::Text("tail".into())];
        let chunks: Vec<axum::body::Bytes> = futures_util::StreamExt::collect::<Vec<_>>(streamed(parts)).await.into_iter().map(Result::unwrap).collect();
        let whole: Vec<u8> = chunks.concat();
        assert_eq!(whole.len(), 4 + (1 << 20) + 5 + 4);
        assert!(whole.starts_with(b"head") && whole.ends_with(b"tail"));
    }

    #[tokio::test]
    async fn a_stateless_request_is_checked_against_its_headers() {
        let call = |headers: &[(&str, &str)], body: Value| {
            let mut map = HeaderMap::new();
            for (name, value) in headers {
                map.insert(axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(), value.parse().unwrap());
            }
            async move {
                let response = handle(map, axum::body::Bytes::from(body.to_string())).await;
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
                (status, serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null))
            }
        };
        let meta = json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {} });
        let modern = [("mcp-protocol-version", "2026-07-28"), ("mcp-method", "server/discover")];
        let (status, found) = call(&modern, json!({ "jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": { "_meta": meta } })).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(found["result"]["supportedVersions"][0], "2026-07-28");
        assert_eq!(found["result"]["resultType"], "complete");
        assert_eq!(found["result"]["cacheScope"], "public");
        assert!(found["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"].is_string());

        let (status, found) = call(&[("mcp-protocol-version", "2026-07-28"), ("mcp-method", "tools/call"), ("mcp-name", "studio_system")], json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "studio_status", "_meta": meta } })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "a name that differs from the body is refused");
        assert_eq!(found["error"]["code"], HEADER_MISMATCH);

        let (status, found) = call(&[("mcp-protocol-version", "2026-07-28")], json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": { "_meta": meta } })).await;
        assert_eq!((status, found["error"]["code"].clone()), (StatusCode::BAD_REQUEST, json!(HEADER_MISMATCH)), "a missing Mcp-Method is refused");

        let old = json!({ "io.modelcontextprotocol/protocolVersion": "1900-01-01" });
        let (status, found) = call(&[("mcp-protocol-version", "1900-01-01"), ("mcp-method", "tools/list")], json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": { "_meta": old } })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(found["error"]["code"], UNSUPPORTED_VERSION);
        assert_eq!(found["error"]["data"]["requested"], "1900-01-01");

        let encoded = format!("=?base64?{}?=", { use base64::Engine; base64::engine::general_purpose::STANDARD.encode("studio://skill") });
        let (status, found) = call(&[("mcp-protocol-version", "2026-07-28"), ("mcp-method", "resources/read"), ("mcp-name", encoded.as_str())], json!({ "jsonrpc": "2.0", "id": 5, "method": "resources/read", "params": { "uri": "studio://skill", "_meta": meta } })).await;
        assert_eq!(status, StatusCode::OK, "a Base64 name is decoded before it is compared");
        assert!(found["result"]["contents"][0]["text"].as_str().unwrap().contains("MCP"));

        let (status, found) = call(&[("mcp-protocol-version", "2026-07-28"), ("mcp-method", "nope/nope")], json!({ "jsonrpc": "2.0", "id": 6, "method": "nope/nope", "params": { "_meta": meta } })).await;
        assert_eq!((status, found["error"]["code"].clone()), (StatusCode::NOT_FOUND, json!(-32601)));

        let (status, _) = call(&[], json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn the_server_introduces_itself_and_lists_its_tools() {
        let reply = |body: Value| async move {
            let response = handle(HeaderMap::new(), axum::body::Bytes::from(body.to_string())).await;
            let bytes = axum::body::to_bytes(response.into_body(), 1 << 22).await.unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        };
        let hello = reply(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).await;
        assert_eq!(hello["result"]["capabilities"], json!({ "tools": {}, "resources": {}, "prompts": {} }));
        let resources = reply(json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/read", "params": { "uri": "studio://guide/lyrics" } })).await;
        assert!(resources["result"]["contents"][0]["text"].as_str().unwrap().contains("[Verse]"));
        let skill = reply(json!({ "jsonrpc": "2.0", "id": 5, "method": "prompts/get", "params": { "name": "studio" } })).await;
        assert!(skill["result"]["messages"][0]["content"]["text"].as_str().unwrap().contains("MCP"));
        let list = reply(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).await;
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), tools().len());
        let unknown = reply(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "nope" } })).await;
        assert_eq!(unknown["error"]["code"], -32602);
    }
}
