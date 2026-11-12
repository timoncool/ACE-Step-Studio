//! Ready subtitles (SRT, SSA/ASS) and LRC as a song's karaoke, in place of
//! recognised timings. The SRT and ASS reading is Dub Studio's
//! (`crates/dub-server/src/subimport.rs`); LRC is read here.

/// One subtitle: its time in seconds and its text, markup taken out.
#[derive(Debug, Clone)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Reads a subtitle file by its extension (srt | ass | ssa | lrc). Anything
/// else is tried as SRT, the most common, then as ASS.
pub fn parse(content: &str, ext: &str) -> Vec<Cue> {
    let e = ext.to_ascii_lowercase();
    if e == "lrc" {
        return finalize(parse_lrc(content));
    }
    let cues = match e.as_str() {
        "ass" | "ssa" => parse_ass(content),
        _ => parse_srt(content),
    };
    if cues.is_empty() && e != "ass" && e != "ssa" && content.contains("[Events]") {
        return finalize(parse_ass(content));
    }
    finalize(cues)
}

/// The cues as the LRC karaoke keeps: one `[mm:ss.cc]` line each.
pub fn to_lrc(cues: &[Cue]) -> String {
    cues.iter().map(|cue| format!("[{}]{}\n", stamp(cue.start), cue.text)).collect()
}

fn stamp(seconds: f64) -> String {
    let centis = (seconds.max(0.0) * 100.0).round() as u64;
    format!("{:02}:{:02}.{:02}", centis / 6000, (centis / 100) % 60, centis % 100)
}

/// Sorted by time, the empty and the zero-length left out.
fn finalize(mut cues: Vec<Cue>) -> Vec<Cue> {
    cues.retain(|c| c.end > c.start && !c.text.trim().is_empty());
    cues.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    cues
}

// SRT: a block is [index] / "HH:MM:SS,mmm --> HH:MM:SS,mmm[ settings]" / text
// lines. A BOM and \r are borne; the index may be missing.
fn parse_srt(content: &str) -> Vec<Cue> {
    let mut out = Vec::new();
    let text = content.trim_start_matches('\u{feff}').replace('\r', "");
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            continue;
        }
        let ti = match lines.iter().position(|l| l.contains("-->")) {
            Some(i) => i,
            None => continue,
        };
        let (start, end) = match parse_time_range(lines[ti], srt_ts) {
            Some(v) => v,
            None => continue,
        };
        let body = lines[ti + 1..].join(" ").trim().to_string();
        if body.is_empty() {
            continue;
        }
        out.push(Cue { start, end, text: strip_markup(&body) });
    }
    out
}

/// "HH:MM:SS,mmm" in seconds; a '.' in place of the ',' is borne.
fn srt_ts(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let (hms, frac) = match s.split_once('.') {
        Some((a, b)) => (a, b),
        None => (s.as_str(), "0"),
    };
    let parts: Vec<&str> = hms.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, s] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    let ms = format!("0.{frac}").parse::<f64>().unwrap_or(0.0);
    Some(h * 3600.0 + m * 60.0 + sec + ms)
}

// SSA / ASS: the [Events] section; its Format: line orders the fields of the
// Dialogue: lines. The text is the LAST field and may hold commas.
fn parse_ass(content: &str) -> Vec<Cue> {
    let text = content.trim_start_matches('\u{feff}').replace('\r', "");
    let mut in_events = false;
    let mut fmt: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let lt = line.trim();
        if lt.starts_with('[') && lt.ends_with(']') {
            in_events = lt.eq_ignore_ascii_case("[Events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(rest) = lt.strip_prefix("Format:") {
            fmt = rest.split(',').map(|s| s.trim().to_ascii_lowercase()).collect();
            continue;
        }
        let rest = match lt.strip_prefix("Dialogue:") {
            Some(r) => r,
            None => continue,
        };
        if fmt.is_empty() {
            fmt = ["layer", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        }
        let i_start = fmt.iter().position(|f| f == "start");
        let i_end = fmt.iter().position(|f| f == "end");
        let i_text = fmt.iter().position(|f| f == "text").unwrap_or(fmt.len() - 1);
        let cols: Vec<&str> = rest.splitn(fmt.len(), ',').collect();
        let get = |idx: Option<usize>| idx.and_then(|i| cols.get(i)).map(|s| s.trim());
        let (s, e) = match (get(i_start), get(i_end)) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };
        let (start, end) = match (ass_ts(s), ass_ts(e)) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };
        let body = cols.get(i_text).copied().unwrap_or("");
        out.push(Cue { start, end, text: strip_markup(body) });
    }
    out
}

/// "H:MM:SS.cc" (ASS) in seconds.
fn ass_ts(s: &str) -> Option<f64> {
    srt_ts(s)
}

/// "HH:MM:SS --> HH:MM:SS" as (start, end); SRT settings after the end are dropped.
fn parse_time_range(line: &str, ts: fn(&str) -> Option<f64>) -> Option<(f64, f64)> {
    let (a, b) = line.split_once("-->")?;
    let b = b.split_whitespace().next().unwrap_or(b.trim());
    Some((ts(a)?, ts(b)?))
}

/// Takes out ASS override tags {\...}, the \N \n \h breaks and HTML tags, and
/// collapses the spaces.
fn strip_markup(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                for n in chars.by_ref() {
                    if n == '}' {
                        break;
                    }
                }
            }
            '\\' => match chars.peek() {
                Some('N') | Some('n') => {
                    chars.next();
                    r.push(' ');
                }
                Some('h') => {
                    chars.next();
                    r.push(' ');
                }
                _ => {}
            },
            '<' => {
                let mut closed = false;
                for n in chars.by_ref() {
                    if n == '>' {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    r.push('<');
                }
            }
            _ => r.push(c),
        }
    }
    r.split_whitespace().collect::<Vec<_>>().join(" ")
}

// LRC: "[mm:ss.xx]text", a line may carry several times; [ar:], [ti:] and the
// other tags without a time are passed over. A line lasts until the next one;
// the last one, five seconds.
fn parse_lrc(content: &str) -> Vec<Cue> {
    let text = content.trim_start_matches('\u{feff}').replace('\r', "");
    let mut stamped: Vec<(f64, String)> = Vec::new();
    for line in text.lines() {
        let mut rest = line.trim();
        let mut times = Vec::new();
        while let Some(inner) = rest.strip_prefix('[') {
            let Some((tag, after)) = inner.split_once(']') else { break };
            match srt_ts(tag) {
                Some(at) => times.push(at),
                None => break,
            }
            rest = after;
        }
        let body = strip_markup(rest);
        for at in times {
            stamped.push((at, body.clone()));
        }
    }
    stamped.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let ends: Vec<f64> = stamped.iter().skip(1).map(|(at, _)| *at).chain(stamped.last().map(|(at, _)| at + 5.0)).collect();
    stamped.into_iter().zip(ends).map(|((start, text), end)| Cue { start, end, text }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_basic() {
        let s = "1\n00:00:01,000 --> 00:00:04,000\nHello world\n\n2\n00:00:05,500 --> 00:00:07,000\nSecond line";
        let c = parse(s, "srt");
        assert_eq!(c.len(), 2);
        assert!((c[0].start - 1.0).abs() < 1e-6);
        assert!((c[0].end - 4.0).abs() < 1e-6);
        assert_eq!(c[0].text, "Hello world");
        assert!((c[1].start - 5.5).abs() < 1e-6);
    }

    #[test]
    fn srt_multiline_and_tags() {
        let s = "1\n00:00:01,000 --> 00:00:02,000\n<i>Line one</i>\nLine two";
        let c = parse(s, "srt");
        assert_eq!(c[0].text, "Line one Line two");
    }

    #[test]
    fn ass_basic() {
        let s = "[Script Info]\nTitle: x\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.50,Default,,0,0,0,,Hello, there {\\i1}world{\\i0}\n";
        let c = parse(s, "ass");
        assert_eq!(c.len(), 1);
        assert!((c[0].start - 1.0).abs() < 1e-6);
        assert!((c[0].end - 3.5).abs() < 1e-6);
        assert_eq!(c[0].text, "Hello, there world");
    }

    #[test]
    fn ass_newline() {
        let s = "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,A\\NB\n";
        let c = parse(s, "ass");
        assert_eq!(c[0].text, "A B");
    }

    #[test]
    fn skips_empty_and_bad() {
        let s = "1\n00:00:01,000 --> 00:00:01,000\nzero-length\n\n2\ngarbage\n\n3\n00:00:02,000 --> 00:00:04,000\nok";
        let c = parse(s, "srt");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].text, "ok");
    }

    #[test]
    fn lrc_lines_with_several_times_and_word_tags_become_karaoke_lines() {
        let s = "[ti:Song]\n[00:12.41]<00:12.41>Neon <00:12.90>trembles\n[00:20.5][01:02.123]Chorus line\n";
        let c = parse(s, "lrc");
        assert_eq!(c.iter().map(|cue| cue.text.as_str()).collect::<Vec<_>>(), vec!["Neon trembles", "Chorus line", "Chorus line"]);
        assert!((c[1].start - 20.5).abs() < 1e-6 && (c[1].end - 62.123).abs() < 1e-6);
        assert_eq!(to_lrc(&c), "[00:12.41]Neon trembles\n[00:20.50]Chorus line\n[01:02.12]Chorus line\n");
    }

    #[test]
    fn an_srt_becomes_lrc_lines() {
        let c = parse("1\n00:01:05,250 --> 00:01:07,000\nHello\n", "srt");
        assert_eq!(to_lrc(&c), "[01:05.25]Hello\n");
    }
}
