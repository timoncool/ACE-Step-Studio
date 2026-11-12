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

/// The engine log's last lines, keys masked; the hub client cleans paths, addresses and ids before it leaves.
fn engine_log_tail(state: &AppState) -> String {
    let lines = state.engine_log.lines().unwrap_or_else(|| music_engine::server::startup_log_tail(LOG_LINES));
    let tail = &lines[lines.len().saturating_sub(LOG_LINES)..];
    crate::studio_log::masked(&tail.join("\n"))
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

/// GET /v1/problem/list: the problems reported from this computer, newest first, and whether each is closed.
pub async fn list(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let hub = hub_of(&state).map_err(|(status, body)| (status, Json(body)))?;
    Ok(Json(json!({ "problems": hub.problems() })))
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
        let (hub, data) = hub_answering(200, json!({ "number": 77, "url": "https://github.com/timoncool/ACE-Step-Studio/issues/77", "token": "6f1c2a3b-0d4e-4f5a-8b6c-7d8e9f0a1b2c" })).await;
        let filed = file(&hub, report()).await.expect("filed");
        assert_eq!((filed.number, hub.problems().len()), (77, 1), "the studio remembers the issue");
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
}
