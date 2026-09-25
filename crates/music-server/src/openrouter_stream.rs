use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};

/// Extracts and decodes the documented `choices[].delta.audio.data` SSE payloads.
/// The caller supplies the complete byte stream after transport has finished.
pub fn decode_audio_sse(body: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(body).context("OpenRouter music stream is not UTF-8 SSE")?;
    let mut encoded = String::new();
    let mut saw_done = false;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else { continue; };
        let data = data.trim_start();
        if data == "[DONE]" { saw_done = true; break; }
        let event: serde_json::Value = serde_json::from_str(data).context("invalid JSON event in OpenRouter music stream")?;
        if let Some(chunk) = event.pointer("/choices/0/delta/audio/data").and_then(serde_json::Value::as_str) {
            encoded.push_str(chunk);
        }
    }
    if !saw_done { bail!("OpenRouter music stream ended before [DONE]"); }
    if encoded.is_empty() { bail!("OpenRouter music stream contained no delta.audio.data"); }
    STANDARD.decode(encoded).context("OpenRouter delta.audio.data is not valid base64")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn joins_audio_data_across_sse_events() {
        let body = b"data: {\"choices\":[{\"delta\":{\"audio\":{\"data\":\"SU\"}}}]}\n\ndata: {\"choices\":[{\"delta\":{\"audio\":{\"data\":\"Qz\"}}}]}\n\ndata: [DONE]\n";
        assert_eq!(decode_audio_sse(body).unwrap(), b"ID3");
    }
    #[test]
    fn rejects_incomplete_streams() { assert!(decode_audio_sse(b"data: {}\n").is_err()); }
}
