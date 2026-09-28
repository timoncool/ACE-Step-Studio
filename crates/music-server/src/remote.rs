//! The studio from another computer: off unless the user turns it on.
//!
//! Turned on, the service listens on every address instead of loopback only,
//! and hands a browser the same interface the window shows. The studio's own
//! window and agents on this computer come in over loopback and are let
//! through as before; anything from the network needs the access key - once
//! in the address (`?key=`, which is then kept as a cookie), as a cookie, or as
//! a bearer token. The engines stay on loopback either way.

use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};

use axum::{
    extract::{ConnectInfo, Request},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The setting as it is kept.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkAccess {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub key: String,
}

fn setting() -> &'static Mutex<NetworkAccess> {
    static SETTING: OnceLock<Mutex<NetworkAccess>> = OnceLock::new();
    SETTING.get_or_init(|| Mutex::new(NetworkAccess::default()))
}

/// Whether the running service listens on the network: what the setting was
/// when it started, since the address is bound once.
fn listening() -> &'static OnceLock<bool> {
    static LISTENING: OnceLock<bool> = OnceLock::new();
    &LISTENING
}

pub fn current() -> NetworkAccess {
    setting().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

/// Loads the kept setting at the service's start and says which address to
/// bind: every address when it is on and has a key, loopback otherwise.
pub fn start(saved: Option<NetworkAccess>) -> [u8; 4] {
    let saved = saved.unwrap_or_default();
    let open = saved.enabled && !saved.key.is_empty();
    *setting().lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = saved;
    let _ = listening().set(open);
    if open { [0, 0, 0, 0] } else { [127, 0, 0, 1] }
}

/// 148 random bits: the random parts of two version 7 ids.
fn new_key() -> String {
    format!("{}{}", uuid::Uuid::now_v7().simple(), uuid::Uuid::now_v7().simple())
}

/// The addresses of this computer another one on the network reaches it at.
fn local_addresses() -> Vec<String> {
    // the address the system would send from, without sending anything
    let mut found = Vec::new();
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("192.168.0.1:9").is_ok() {
            if let Ok(address) = socket.local_addr() {
                if !address.ip().is_loopback() && !address.ip().is_unspecified() {
                    found.push(address.ip().to_string());
                }
            }
        }
    }
    found
}

/// The setting for the page. The key is shown only to this computer.
pub async fn status(ConnectInfo(peer): ConnectInfo<SocketAddr>) -> Json<Value> {
    let access = current();
    let local = peer.ip().is_loopback();
    Json(json!({
        "enabled": access.enabled,
        "key": if local { Some(access.key) } else { None },
        "listening": listening().get().copied().unwrap_or(false),
        "addresses": if local { local_addresses() } else { Vec::new() },
        "port": crate::listen_port(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct AccessChange {
    #[serde(default)]
    enabled: Option<bool>,
    /// A new key replaces the old one; every browser signed in with it is out.
    #[serde(default)]
    new_key: bool,
}

/// Turns access on or off, or makes a new key. Only from this computer.
pub async fn change(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(change): Json<AccessChange>,
) -> Result<Json<Value>, (StatusCode, Json<crate::ApiError>)> {
    if !peer.ip().is_loopback() {
        return Err(crate::api_error(StatusCode::FORBIDDEN, "access from the network is changed on the studio's own computer".into()));
    }
    {
        let mut access = setting().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(enabled) = change.enabled {
            access.enabled = enabled;
        }
        if change.new_key || (access.enabled && access.key.is_empty()) {
            access.key = new_key();
        }
    }
    crate::persist_studio_settings(&state).await.map_err(|error| crate::api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")))?;
    Ok(status(ConnectInfo(peer)).await)
}

/// The key a request carries: `?key=`, the cookie, or a bearer token.
fn carried_key(request: &Request) -> Option<String> {
    let from_query = request.uri().query().and_then(|query| query.split('&').find_map(|pair| pair.strip_prefix("key=")).map(str::to_string));
    let from_cookie = request
        .headers()
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| cookies.split(';').find_map(|pair| pair.trim().strip_prefix("studio_key=")).map(str::to_string));
    let from_bearer = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string);
    from_query.or(from_cookie).or(from_bearer)
}

/// Compares keys in time that does not depend on where they differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// Lets this computer through, and the network only with the key.
pub async fn guard(ConnectInfo(peer): ConnectInfo<SocketAddr>, request: Request, next: Next) -> Response {
    if peer.ip().is_loopback() {
        return next.run(request).await;
    }
    let access = current();
    if !access.enabled || access.key.is_empty() {
        return (StatusCode::FORBIDDEN, "Access from the network is off. Turn it on in the studio: Settings - Appearance - Access from the network.").into_response();
    }
    match carried_key(&request) {
        Some(key) if same(&key, &access.key) => {
            // a key in the address becomes a cookie, and the address loses it
            let in_query = request.uri().query().is_some_and(|query| query.split('&').any(|pair| pair.starts_with("key=")));
            if in_query {
                let path = request.uri().path().to_string();
                let rest: Vec<&str> = request.uri().query().unwrap_or_default().split('&').filter(|pair| !pair.starts_with("key=")).collect();
                let location = if rest.is_empty() { path } else { format!("{path}?{}", rest.join("&")) };
                return Response::builder()
                    .status(StatusCode::SEE_OTHER)
                    .header(header::LOCATION, location)
                    .header(header::SET_COOKIE, format!("studio_key={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000", access.key))
                    .body(axum::body::Body::empty())
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
            next.run(request).await
        }
        _ => {
            let path = request.uri().path();
            if path.starts_with("/v1") || path.starts_with("/mcp") || path.starts_with("/setup") || path.starts_with("/engine") || path == "/health" {
                (StatusCode::UNAUTHORIZED, "the access key is missing or wrong").into_response()
            } else {
                (StatusCode::UNAUTHORIZED, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], SIGN_IN).into_response()
            }
        }
    }
}

const SIGN_IN: &str = r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Studio</title>
<style>body{font-family:system-ui,sans-serif;background:#111;color:#eee;display:grid;place-items:center;height:100vh;margin:0}form{display:grid;gap:12px;width:min(360px,90vw)}input,button{font-size:16px;padding:10px 12px;border-radius:10px;border:1px solid #444;background:#1c1c1c;color:#eee}button{background:#db2777;border:0;font-weight:600}p{color:#aaa;font-size:14px;margin:0}</style></head>
<body><form method="get"><p>Access key - shown in the studio under Settings, Appearance, Access from the network.</p><input name="key" autocomplete="off" autofocus placeholder="Access key"><button>Open the studio</button></form></body></html>"#;

/// The window's own files, handed over by the desktop shell: path to bytes
/// and media type. Absent when the service runs on its own.
pub type AssetSource = Box<dyn Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync>;

fn assets() -> &'static OnceLock<AssetSource> {
    static ASSETS: OnceLock<AssetSource> = OnceLock::new();
    &ASSETS
}

pub fn set_asset_source(source: AssetSource) {
    let _ = assets().set(source);
}

/// The interface for a browser: `/` is the page, anything else a file of it.
pub async fn interface(request: Request) -> Response {
    let Some(source) = assets().get() else {
        return (StatusCode::NOT_FOUND, "this service has no interface of its own; open the studio window").into_response();
    };
    let path = request.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match source(path).or_else(|| (!path.contains('.')).then(|| source("index.html")).flatten()) {
        Some((bytes, mime)) => ([(header::CONTENT_TYPE, mime)], bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_compare_only_equal() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
    }
}
