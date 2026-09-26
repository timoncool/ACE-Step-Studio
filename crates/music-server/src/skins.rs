//! Winamp skins for the player's Winamp mode: classic Winamp 2 `.wsz` files,
//! kept in the studio's own folder. The user finds them in the Winamp Skin
//! Museum, downloads what they like and adds the file; the window lists what
//! is here and loads a skin from the address this serves it at.

use std::path::PathBuf;

use axum::body::{Body, Bytes};
use axum::extract::Path;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::Json;
use serde_json::{json, Value};

use crate::{api_error, ApiError};

/// The largest skin taken; the museum's run well under a megabyte.
pub const LIMIT: usize = 32 * 1024 * 1024;

/// A .wsz is a zip; anything else is not a skin.
const ZIP: [u8; 4] = [b'P', b'K', 3, 4];

fn folder() -> Result<PathBuf, (StatusCode, Json<ApiError>)> {
    let root = crate::studio_data_root().ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "the studio has no data folder".into()))?;
    let folder = root.join("skins");
    std::fs::create_dir_all(&folder).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("create {}: {error}", folder.display())))?;
    Ok(folder)
}

/// A file name that stays inside the skins folder.
fn clean(name: &str) -> Option<String> {
    let stem = name.trim().trim_end_matches(".wsz").trim_end_matches(".WSZ").trim();
    let stem: String = stem.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let stem = stem.trim().trim_matches('.').to_string();
    (!stem.is_empty()).then(|| format!("{stem}.wsz"))
}

/// The skins in the folder, by name.
pub async fn list() -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let folder = folder()?;
    let mut skins: Vec<Value> = std::fs::read_dir(&folder)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("read {}: {error}", folder.display())))?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.to_ascii_lowercase().ends_with(".wsz").then(|| {
                let title = name[..name.len() - 4].to_string();
                json!({ "name": title, "url": format!("/v1/skins/file/{}", urlencoding(&name)) })
            })
        })
        .collect();
    skins.sort_by(|a, b| a["name"].as_str().unwrap_or_default().to_lowercase().cmp(&b["name"].as_str().unwrap_or_default().to_lowercase()));
    Ok(Json(json!({ "skins": skins, "folder": folder.display().to_string() })))
}

/// Adds a skin: the file's bytes, its name in `X-File-Name`.
pub async fn add(headers: HeaderMap, body: Bytes) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let name = headers
        .get("x-file-name")
        .and_then(|value| value.to_str().ok())
        .map(percent_decode)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "the skin has no file name".into()))?;
    save(&name, &body).map(Json).map_err(|problem| api_error(StatusCode::BAD_REQUEST, problem))
}

/// Keeps a .wsz in the skins folder under its name; what the agent's tool and the window's upload share.
pub fn save(name: &str, bytes: &[u8]) -> Result<Value, String> {
    let name = clean(name).ok_or_else(|| "the skin has no file name".to_string())?;
    if !bytes.starts_with(&ZIP) {
        return Err(format!("{name} is not a Winamp skin: a .wsz file is a zip archive"));
    }
    let path = folder().map_err(|(_, Json(error))| error.error)?.join(&name);
    std::fs::write(&path, bytes).map_err(|error| format!("write {}: {error}", path.display()))?;
    Ok(json!({ "name": name.trim_end_matches(".wsz"), "url": format!("/v1/skins/file/{}", urlencoding(&name)), "path": path.display().to_string() }))
}

pub async fn file(Path(name): Path<String>) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let name = clean(&name).ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "no such skin".into()))?;
    let path = folder()?.join(&name);
    let bytes = std::fs::read(&path).map_err(|_| api_error(StatusCode::NOT_FOUND, format!("no skin {name}")))?;
    Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .body(Body::from(bytes))
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

fn urlencoding(name: &str) -> String {
    name.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skin_name_stays_inside_the_folder() {
        assert_eq!(clean("Base 2.91.wsz").as_deref(), Some("Base 2.91.wsz"));
        assert_eq!(clean("../../evil").as_deref(), Some("_.._evil.wsz"));
        assert_eq!(clean("  .wsz").as_deref(), None);
        assert_eq!(percent_decode(&urlencoding("Зелёный скин.wsz")), "Зелёный скин.wsz");
    }
}
