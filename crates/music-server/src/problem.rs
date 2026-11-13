//! "Report a problem" without a GitHub account: the person's words from the window, with the facts the studio knows,
//! filed through Studio Hub, whose bot opens the issue (`studio_hub_client::problems`).

use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use studio_hub_client::problems::{Filed, ProblemKind, ProblemReport, ReportError};
use studio_hub_client::Hub;

use crate::AppState;

/// Lines of the engine log a report carries.
const LOG_LINES: usize = 200;

/// The window's form: `kind` is bug, idea or question; the engine log goes along unless `attach_log` is false.
#[derive(Debug, Deserialize)]
pub struct ProblemRequest {
    #[serde(default = "bug")]
    kind: String,
    what: String,
    #[serde(default)]
    steps: String,
    #[serde(default)]
    expected: String,
    #[serde(default = "yes")]
    attach_log: bool,
}

fn bug() -> String {
    "bug".into()
}

fn yes() -> bool {
    true
}

/// The refusal the window shows, by kind, with the status of the answer.
pub fn refusal(error: &ReportError) -> (StatusCode, Value) {
    match error {
        ReportError::Field(field) => (StatusCode::BAD_REQUEST, json!({ "code": "field", "field": field })),
        ReportError::Rate { retry_after_s } => (StatusCode::TOO_MANY_REQUESTS, json!({ "code": "rate", "retry_after_s": retry_after_s })),
        ReportError::NotConfigured => (StatusCode::SERVICE_UNAVAILABLE, json!({ "code": "not_configured" })),
        ReportError::Busy => (StatusCode::SERVICE_UNAVAILABLE, json!({ "code": "busy" })),
        ReportError::Unreachable(why) => (StatusCode::BAD_GATEWAY, json!({ "code": "unreachable", "detail": why })),
    }
}

/// The report of a request: the set chosen now and the tail of the engine log go with the person's words.
async fn report_of(state: &AppState, request: ProblemRequest) -> Result<ProblemReport, (StatusCode, Value)> {
    let kind = match request.kind.as_str() {
        "bug" => ProblemKind::Bug,
        "idea" => ProblemKind::Idea,
        "question" => ProblemKind::Question,
        other => return Err((StatusCode::BAD_REQUEST, json!({ "code": "field", "field": format!("kind: {other} is not bug, idea or question") }))),
    };
    let set = state.selected_profile_id.read().await.clone();
    let models = match set {
        Some(set) => Some(set),
        None => state.selected_component_ids.read().await.as_ref().map(|ids| ids.join(", ")),
    };
    let log = if request.attach_log { Some(engine_log_tail(state)) } else { None };
    let given = |text: String| Some(text.trim().to_string()).filter(|text| !text.is_empty());
    Ok(ProblemReport { kind, what: request.what, steps: given(request.steps), expected: given(request.expected), models, log })
}

/// The engine log's last lines without the text of any song, keys masked; the hub client cleans paths, addresses and
/// ids before it leaves.
fn engine_log_tail(state: &AppState) -> String {
    let lines = state.engine_log.lines().unwrap_or_else(|| music_engine::server::startup_log_tail(LOG_LINES));
    let lines = without_request_text(&lines);
    let tail = &lines[lines.len().saturating_sub(LOG_LINES)..];
    crate::studio_log::masked(&tail.join("\n"))
}

/// What stands where the engine printed what a person asked for.
const LEFT_OUT: &str = "[request text left out]";

/// The tags the engine starts its own lines with: a line under one of them ends the text the engine printed after
/// a header such as `[fill Batch0] seed=N:`. A tag the engine gains later is read as text until the next known one,
/// which leaves out more, never less.
const ENGINE_TAGS: &[&str] = &[
    "Ace-LM", "Ace-Synth", "Ace-Understand", "Adapter", "Audio-Resample", "Audio", "BPE", "Build-Context", "Build-Schedule",
    "CondEnc", "CondEncoder", "Context-Silence", "Context", "Debug", "Dedup", "DiT-Generate", "DiT", "Encode-Src", "Encode-Text",
    "Encode-Timbre", "Encode", "FSM", "FSQ-Roundtrip", "FSQ-Tok", "FSQ", "GGUF", "GraphArena", "Init-Noise", "LM-Adapter",
    "LM-Batch", "LM-Config", "LM-Debug", "LM-Forward", "LM-Generate", "LM-KV", "LM-Load", "LM-Phase1", "LM-Phase2", "LM-Rep",
    "LM", "Latent-Splice", "Latent", "Load", "MP3-Codec", "MP3", "Outpaint", "Pipeline-Synth", "Quantize", "Qwen3", "Region",
    "Registry", "Request", "Resolve-Params", "Resolve-T", "STORK2", "STORK4", "Safetensors", "Server", "Store", "Synth-Load",
    "Synth-Run", "TextEncoder", "Tok", "Understand-Debug", "Understand-Decode", "Understand-Load", "Understand-Prefill",
    "Understand-Prompt", "Understand-Result", "Understand-Tok", "Understand-VAE", "Understand", "VAE-Decode", "VAE-Enc", "VAE",
    "WAV-Splice", "WAV", "WeightCtx", "studio", "ERROR", "OK",
];

/// The tag of an engine line - `[DiT] Step 1/8`, `[VAE-Decode Batch0] ...` - with its batch dropped, when the line is one.
fn engine_tag(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('[')?;
    let (tag, after) = rest.split_once(']')?;
    if !after.starts_with(' ') {
        return None;
    }
    let tag = tag.rsplit_once(" Batch").filter(|(_, index)| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())).map_or(tag, |(name, _)| name);
    Some(tag)
}

/// The engine's log with the text of each request - the style, caption and lyrics it was asked for or wrote, and the
/// chain of thought it printed - replaced by one line [`LEFT_OUT`]; loading, steps and errors stay. The engine prints
/// that text after a header that ends in a colon (`[fill Batch0] seed=N:`, `[inspire Batch0] seed=N:`,
/// `[LM-Phase2] N=1, CoT[0]:`) up to its next own line, and a caption it heard on one line (`[Understand-Result]
/// caption: ...`). Untagged lines before the first engine line may belong to text whose header is earlier and go too,
/// except those that read as a failure: an engine that fails at its start prints them before any line of its own.
pub fn without_request_text(lines: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::with_capacity(lines.len());
    let mut in_text = true;
    let mut after_header = false;
    let mut leave_out = |kept: &mut Vec<String>| {
        if kept.last().map(String::as_str) != Some(LEFT_OUT) {
            kept.push(LEFT_OUT.to_string());
        }
    };
    for line in lines {
        let trimmed = line.trim_end();
        let header = trimmed.ends_with(':') && engine_tag(trimmed).is_some() && (trimmed.contains("] seed=") || trimmed.contains("CoT["));
        if header {
            kept.push(line.clone());
            leave_out(&mut kept);
            in_text = true;
            after_header = true;
            continue;
        }
        let own = engine_tag(trimmed).is_some_and(|tag| ENGINE_TAGS.contains(&tag)) || trimmed.starts_with("load_backend:") || trimmed.starts_with("ggml_");
        if in_text && !own {
            if !after_header && reads_as_failure(trimmed) {
                kept.push(line.clone());
            } else if !trimmed.is_empty() {
                leave_out(&mut kept);
            }
            continue;
        }
        in_text = false;
        if trimmed.starts_with("[Understand-Result] caption:") {
            leave_out(&mut kept);
            continue;
        }
        kept.push(line.clone());
    }
    kept
}

/// An untagged line that reads as a failure of the engine or its libraries: CUDA, ggml and the loader print these
/// before the engine's first line of its own (`CUDA error: out of memory`, `  current device: 0, in function ...`,
/// `...ggml-cuda.cu:111: CUDA error`, `...ggml.c:1234: GGML_ASSERT(...) failed`).
fn reads_as_failure(line: &str) -> bool {
    let lower = line.to_lowercase();
    if ["error", "fatal", "failed", "assert", "exception", "cannot", "not found", "abort"].iter().any(|word| lower.contains(word)) {
        return true;
    }
    if line.starts_with(char::is_whitespace) && lower.trim_start().starts_with("current device") {
        return true;
    }
    // a source file and its line: `...\ggml-cuda.cu:111:`
    let parts: Vec<&str> = line.split(':').collect();
    parts.windows(3).any(|window| {
        let (file, number) = (window[0].trim_end(), window[1]);
        let source = [".c", ".cc", ".cpp", ".cu", ".h", ".hpp"].iter().any(|extension| file.ends_with(extension));
        source && (file.contains('/') || file.contains('\\') || !file.contains(' ')) && !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn hub_of(state: &AppState) -> Result<&Hub, (StatusCode, Value)> {
    state.hub.as_ref().ok_or_else(|| refusal(&ReportError::Unreachable("Studio Hub is off for this run: its state file cannot be read".into())))
}

/// POST /v1/problem/preview: exactly the request that would leave now, for the window's "what will be sent".
pub async fn preview(State(state): State<AppState>, Json(request): Json<ProblemRequest>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let shown = async {
        let report = report_of(&state, request).await?;
        hub_of(&state)?.problem_body(&report).map_err(|error| refusal(&error))
    };
    shown.await.map(Json).map_err(|(status, body)| (status, Json(body)))
}

/// Files a report through `hub`, the refusal by kind when it does not go.
pub async fn file(hub: &Hub, report: ProblemReport) -> Result<Filed, (StatusCode, Value)> {
    hub.report_problem(report).await.map_err(|error| refusal(&error))
}

/// POST /v1/problem/report: the issue's number and address, or why it was not made.
pub async fn report(State(state): State<AppState>, Json(request): Json<ProblemRequest>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let filed = async {
        let report = report_of(&state, request).await?;
        file(hub_of(&state)?, report).await
    };
    match filed.await {
        Ok(filed) => {
            studio_log!("a problem was reported from the window: issue #{}", filed.number);
            Ok(Json(json!({ "number": filed.number, "url": filed.url })))
        }
        Err((status, body)) => {
            studio_log!("[ERROR] a problem report did not go: {body}");
            Err((status, Json(body)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hub at a local address that answers every report with `status` and `body`.
    async fn hub_answering(status: u16, body: Value) -> (Hub, std::path::PathBuf) {
        let app = axum::Router::new().route(
            "/v1/issue",
            axum::routing::post(move || {
                let body = body.clone();
                async move { (StatusCode::from_u16(status).unwrap(), Json(body)) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.ok() });
        hub_at(vec![format!("http://{address}")])
    }

    fn hub_at(urls: Vec<String>) -> (Hub, std::path::PathBuf) {
        let data = std::env::temp_dir().join(format!("problem-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&data).unwrap();
        let hub = Hub::new(studio_hub_client::HubConfig {
            app: crate::PROGRAM_ID.into(),
            version: "test".into(),
            data_dir: data.clone(),
            http: reqwest::Client::new(),
            os_label: "windows 11".into(),
            gpu: studio_hub_client::Gpu { vendor: "nvidia".into(), vram_gb: "24".into(), backend: "cuda".into(), compute: Some("8.9".into()) },
            ui_lang: "en".into(),
            urls,
        })
        .unwrap();
        (hub, data)
    }

    fn report() -> ProblemReport {
        ProblemReport { kind: ProblemKind::Bug, what: "The song stops at decoding every time".into(), steps: None, expected: None, models: Some("quality-q8".into()), log: Some("line one\nline two".into()) }
    }

    /// Every way a report does not go reaches the window as its own kind, and one that goes gives the issue.
    #[tokio::test]
    async fn a_report_comes_back_as_the_issue_or_as_its_refusal_by_kind() {
        let cases = [
            (400, json!({ "error": "what: too short" }), "field"),
            (429, json!({ "error": "rate", "retry_after_s": 1200 }), "rate"),
            (503, json!({ "error": "not_configured" }), "not_configured"),
            (503, json!({ "error": "busy" }), "busy"),
            (503, json!({ "error": "github", "status": 502 }), "unreachable"),
        ];
        for (status, body, code) in cases {
            let (hub, data) = hub_answering(status, body).await;
            let (http, refused) = file(&hub, report()).await.expect_err(code);
            assert_eq!(refused["code"], code, "{status}");
            if code == "rate" {
                assert_eq!((http, refused["retry_after_s"].as_u64()), (StatusCode::TOO_MANY_REQUESTS, Some(1200)));
            }
            std::fs::remove_dir_all(&data).ok();
        }
        let (hub, data) = hub_at(vec!["http://127.0.0.1:9".into()]);
        assert_eq!(file(&hub, report()).await.expect_err("no hub").1["code"], "unreachable");
        std::fs::remove_dir_all(&data).ok();
        let (hub, data) = hub_answering(200, json!({ "number": 77, "url": "https://github.com/timoncool/ACE-Step-Studio/issues/77" })).await;
        let filed = file(&hub, report()).await.expect("filed");
        assert_eq!((filed.number, filed.url.as_str()), (77, "https://github.com/timoncool/ACE-Step-Studio/issues/77"));
        std::fs::remove_dir_all(&data).ok();
    }

    /// What the window shows before sending is the request itself: the person's words, the set and the log.
    #[test]
    fn the_preview_is_the_request_that_leaves() {
        let (hub, data) = hub_at(Vec::new());
        let body = hub.problem_body(&report()).unwrap();
        assert_eq!(body["what"], "The song stops at decoding every time");
        assert_eq!(body["models"], "quality-q8");
        assert!(body["log"].as_str().is_some_and(|log| log.contains("line two")));
        assert_eq!(refusal(&hub.problem_body(&ProblemReport { what: "short".into(), ..report() }).unwrap_err()).1["code"], "field");
        std::fs::remove_dir_all(&data).ok();
    }


    /// A real stretch of the engine's log - a song asked for with a caption and written lyrics, then an engine that
    /// could not bind its port - leaves without one line of the song's text, its loading, steps and errors in place,
    /// in the log the preview shows as in the one sent.
    #[test]
    fn the_log_that_leaves_carries_no_text_of_a_song() {
        let lines: Vec<String> = include_str!("testdata/engine-log-with-song-text.log").lines().map(String::from).collect();
        let song = ["caption:", "Coffee in the morning sun", "Electric sunshine fills the air", "[Verse 1]", "[Chorus]", "(Ooooh-ooh-ooh-ooh)", "keyscale:", "bpm:88", "</think>", "# Lyric"];
        let source = lines.join("\n");
        assert!(song.iter().all(|text| source.contains(text)), "the stretch holds the song's text");
        let kept = without_request_text(&lines);
        let log = kept.join("\n");
        for text in song {
            assert!(!log.contains(text), "{text} left the studio");
        }
        assert_eq!(kept.iter().filter(|line| line.as_str() == LEFT_OUT).count(), 2, "one mark for the written song, one for the chain of thought");
        for line in ["[fill Batch0] seed=", "[LM-Phase2] N=1, CoT[0]:", "[LM-Phase2] max_tokens:", "[DiT] Step 1/8", "[DiT] Step 8/8", "[VAE-Decode Batch0] Decode:", "[Server] FATAL: cannot bind 127.0.0.1:8089", "load_backend: loaded CUDA backend"] {
            assert!(log.contains(line), "{line} stays");
        }
        assert_eq!(without_request_text(&lines[lines.iter().position(|line| line.starts_with("Coffee")).unwrap()..]).first().map(String::as_str), Some(LEFT_OUT), "a tail that starts inside the text");
        assert_eq!(without_request_text(&["[Understand-Result] caption: a song about my dog".to_string(), "[Understand-Result] bpm: 120".to_string()]), [LEFT_OUT, "[Understand-Result] bpm: 120"]);

        let (hub, data) = hub_at(Vec::new());
        let body = hub.problem_body(&ProblemReport { log: Some(log.clone()), ..report() }).unwrap();
        let shown = body["log"].as_str().unwrap();
        assert!(shown.contains("[DiT] Step 8/8") && !shown.contains("Coffee in the morning sun"));
        std::fs::remove_dir_all(&data).ok();
    }


    /// An engine that fails at its start prints the failure before any line of its own: those lines leave whole, while
    /// a log that starts inside a song's text still starts with the mark.
    #[test]
    fn a_failure_at_the_start_leaves_whole() {
        let failure: Vec<String> = [
            "CUDA error: out of memory",
            "  current device: 0, in function ggml_cuda_init at D:\\a\\ggml\\src\\ggml-cuda\\ggml-cuda.cu:254",
            "D:\\a\\ggml\\src\\ggml-cuda\\ggml-cuda.cu:111: CUDA error",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(without_request_text(&failure), failure);
        let assert = ["/src/ggml/src/ggml.c:1934: GGML_ASSERT(ggml_can_mul_mat(a, b)) failed".to_string(), "error: unknown argument: --keep".to_string()];
        assert_eq!(without_request_text(&assert), assert);

        let song = ["Coffee in the morning sun".to_string(), "Meet me, Mr. Brown, at 5:30".to_string(), "[Chorus]".to_string(), "[DiT] Step 1/8 t=1.000".to_string()];
        assert_eq!(without_request_text(&song), [LEFT_OUT, "[DiT] Step 1/8 t=1.000"]);
        let after_header = ["[fill Batch0] seed=7:".to_string(), "caption: an error of my ways".to_string(), "[LM-Phase2] max_tokens: 250".to_string()];
        assert_eq!(without_request_text(&after_header), ["[fill Batch0] seed=7:", LEFT_OUT, "[LM-Phase2] max_tokens: 250"], "after a header the text goes whatever it says");
    }
}
