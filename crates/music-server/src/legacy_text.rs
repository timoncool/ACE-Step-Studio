//! Text written in the system's legacy code page rather than in Unicode: cue
//! sheets and lyric files saved by old rippers, and ID3 tags that declare
//! ISO-8859-1 while holding GBK, Shift-JIS or Windows-1251 bytes. Windows
//! players read both in the ANSI code page of the machine, and so does this.

use encoding_rs::Encoding;
use std::sync::OnceLock;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetACP() -> u32;
}

/// The machine's ANSI code page as an encoding; Windows-1252 off Windows.
pub fn system_encoding() -> &'static Encoding {
    static ENCODING: OnceLock<&'static Encoding> = OnceLock::new();
    #[cfg(windows)]
    let code_page = unsafe { GetACP() };
    #[cfg(not(windows))]
    let code_page = 1252;
    ENCODING.get_or_init(|| for_code_page(code_page))
}

fn for_code_page(code_page: u32) -> &'static Encoding {
    match code_page {
        874 => encoding_rs::WINDOWS_874,
        932 => encoding_rs::SHIFT_JIS,
        936 => encoding_rs::GBK,
        949 => encoding_rs::EUC_KR,
        950 => encoding_rs::BIG5,
        1250 => encoding_rs::WINDOWS_1250,
        1251 => encoding_rs::WINDOWS_1251,
        1253 => encoding_rs::WINDOWS_1253,
        1254 => encoding_rs::WINDOWS_1254,
        1255 => encoding_rs::WINDOWS_1255,
        1256 => encoding_rs::WINDOWS_1256,
        1257 => encoding_rs::WINDOWS_1257,
        1258 => encoding_rs::WINDOWS_1258,
        _ => encoding_rs::WINDOWS_1252,
    }
}

/// A text file's contents: UTF-8 when it is (a BOM is dropped), else the
/// system code page.
pub fn decode(bytes: &[u8]) -> String {
    decode_with(bytes, system_encoding())
}

fn decode_with(bytes: &[u8], legacy: &'static Encoding) -> String {
    match std::str::from_utf8(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)) {
        Ok(text) => text.to_string(),
        Err(_) => legacy.decode(bytes).0.into_owned(),
    }
}

/// A tag the reader took for ISO-8859-1: its characters are the raw bytes,
/// read again in the system code page. Kept as it is when it holds anything
/// beyond one byte per character or does not decode cleanly.
pub fn repair_latin1(text: &str) -> String {
    repair_latin1_with(text, system_encoding())
}

fn repair_latin1_with(text: &str, legacy: &'static Encoding) -> String {
    if legacy == encoding_rs::WINDOWS_1252 || text.chars().all(|c| c.is_ascii()) || text.chars().any(|c| c as u32 > 0xFF) {
        return text.to_string();
    }
    let bytes: Vec<u8> = text.chars().map(|c| c as u32 as u8).collect();
    let (decoded, _, had_errors) = legacy.decode(&bytes);
    if had_errors { text.to_string() } else { decoded.into_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_latin1(bytes: &[u8]) -> String {
        bytes.iter().map(|&byte| byte as char).collect()
    }

    #[test]
    fn a_gbk_tag_read_as_latin1_comes_back_as_chinese() {
        let bytes = encoding_rs::GBK.encode("人生如戏").0.into_owned();
        assert_eq!(repair_latin1_with(&as_latin1(&bytes), encoding_rs::GBK), "人生如戏");
    }

    #[test]
    fn a_cyrillic_tag_read_as_latin1_comes_back_as_russian() {
        let bytes = encoding_rs::WINDOWS_1251.encode("Дорога домой").0.into_owned();
        assert_eq!(repair_latin1_with(&as_latin1(&bytes), encoding_rs::WINDOWS_1251), "Дорога домой");
    }

    #[test]
    fn unicode_and_ascii_tags_are_left_alone() {
        assert_eq!(repair_latin1_with("人生如戏", encoding_rs::GBK), "人生如戏");
        assert_eq!(repair_latin1_with("Night Rider", encoding_rs::GBK), "Night Rider");
        assert_eq!(repair_latin1_with("Café", encoding_rs::WINDOWS_1252), "Café");
    }

    #[test]
    fn a_file_in_the_code_page_decodes() {
        let bytes = encoding_rs::GBK.encode("歌词").0.into_owned();
        assert_eq!(decode_with(&bytes, encoding_rs::GBK), "歌词");
        assert_eq!(decode_with("\u{FEFF}lyrics".as_bytes(), encoding_rs::GBK), "lyrics");
    }
}
