//! multipart/mixed bodies as the engine returns them.

use anyhow::{bail, Context, Result};

/// The parts of a multipart/mixed body, in wire order.
pub fn parse(content_type: &str, body: &[u8]) -> Result<Vec<Part>> {
    let boundary = boundary_from_content_type(content_type)?;
    parse_parts(&boundary, body)
}

/// The file extension of an audio part.
pub fn audio_extension(content_type: &str) -> Result<&'static str> {
    match content_type.split(';').next().unwrap_or("").trim() {
        "audio/mpeg" => Ok("mp3"),
        "audio/wav" | "audio/x-wav" => Ok("wav"),
        other => bail!("unsupported engine audio content type: {other}"),
    }
}

/// One part of a multipart body: its Content-Type and its bytes.
pub struct Part { pub content_type: String, pub body: Vec<u8> }

pub fn boundary_from_content_type(content_type: &str) -> Result<String> {
    let mut fields = content_type.split(';');
    if !fields.next().is_some_and(|value| value.trim().eq_ignore_ascii_case("multipart/mixed")) {
        bail!("the engine result is not multipart/mixed");
    }
    let boundary = fields
        .find_map(|field| field.trim().strip_prefix("boundary=").or_else(|| field.trim().strip_prefix("Boundary=")))
        .map(|value| value.trim().trim_matches('"').to_owned())
        .filter(|value| !value.is_empty())
        .context("multipart result has no boundary")?;
    if boundary.bytes().any(|byte| byte == b'\r' || byte == b'\n') {
        bail!("multipart boundary contains a line break");
    }
    Ok(boundary)
}

pub fn parse_parts(boundary: &str, body: &[u8]) -> Result<Vec<Part>> {
    let marker = [b"--".as_slice(), boundary.as_bytes()].concat();
    if !body.starts_with(&marker) { bail!("multipart result does not start with its boundary"); }
    let mut cursor = 0;
    let mut parts = Vec::new();
    loop {
        if !body[cursor..].starts_with(&marker) { bail!("multipart boundary is malformed"); }
        cursor += marker.len();
        if body[cursor..].starts_with(b"--\r\n") {
            cursor += 4;
            if cursor != body.len() { bail!("data follows the closing multipart boundary"); }
            break;
        }
        if !body[cursor..].starts_with(b"\r\n") { bail!("multipart boundary has no CRLF terminator"); }
        cursor += 2;
        let headers_end = find_bytes(&body[cursor..], b"\r\n\r\n").context("multipart part has no header terminator")? + cursor;
        let content_type = parse_headers(&body[cursor..headers_end])?;
        let content_start = headers_end + 4;
        let next_boundary = find_bytes(&body[content_start..], &[b"\r\n".as_slice(), marker.as_slice()].concat()).context("multipart part has no following boundary")? + content_start;
        parts.push(Part { content_type, body: body[content_start..next_boundary].to_vec() });
        cursor = next_boundary + 2;
    }
    Ok(parts)
}

fn parse_headers(raw: &[u8]) -> Result<String> {
    let headers = std::str::from_utf8(raw).context("multipart headers are not UTF-8")?;
    let mut content_type = None;
    for line in headers.split("\r\n") {
        let (name, value) = line.split_once(':').context("malformed multipart header")?;
        if name.trim().eq_ignore_ascii_case("content-type") {
            if content_type.replace(value.trim().to_owned()).is_some() { bail!("multipart part has duplicate Content-Type header"); }
        }
    }
    content_type.context("multipart part has no Content-Type header")
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_come_back_in_wire_order_with_their_bytes() {
        let body = b"--b\r\nContent-Type: audio/mpeg\r\n\r\nID3\x00\xff\r\n--b\r\nContent-Type: application/octet-stream\r\n\r\n\x01\x02\r\n--b--\r\n";
        let parts = parse("multipart/mixed; boundary=b", body).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].content_type, "audio/mpeg");
        assert_eq!(parts[0].body, b"ID3\x00\xff");
        assert_eq!(parts[1].body, b"\x01\x02");
    }

    #[test]
    fn a_body_without_its_boundary_is_refused() {
        assert!(parse("multipart/mixed; boundary=b", b"nothing").is_err());
        assert!(parse("application/json", b"{}").is_err());
    }
}
