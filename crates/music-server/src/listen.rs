//! Describing a song by ear, as HOT-Step's Training Studio does.
//!
//! MOSS-Music-8B hears the recording through `ace-caption` (HOT-Step's native
//! GGML port) and writes the plain caption an ACE-Step dataset trains on: the
//! genre tags, two to four sentences, and its guesses at tempo, key and time
//! signature. It is unreliable on numbers, so tempo and key come from what
//! `audio_facts` measured, and a tempo or key the prose states is corrected.
//!
//! Prompts, sampling and the caption clean-up follow HOT-Step-CPP 8a5e42c4:
//! server/src/services/training/{captionPrompt,mossCaption}.ts.

use std::{collections::HashMap, path::Path, process::Command};

use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::audio_facts::Facts;

/// captionPrompt.ts CAPTION_INSTRUCTIONS, with the genre line first: named
/// before any prose exists, the genre stays what the model heard.
pub const PROSE_PROMPT: &str = "Write music dataset metadata grounded in the song's audible content. If audio is attached, describe what you actually HEAR and use title, artist, and lyrics only as weak secondary context.

Return EXACTLY 5 lines in plain text and nothing else. Each field must start at the beginning of its own new line. Never place two fields on the same line.

Use this exact output template:
genre: <comma-separated genre/style tags, most specific first>
caption: <2 to 4 sentences on one line>
bpm: <estimated BPM as integer, e.g. 120>
key: <note plus lowercase mode, e.g. 'C minor' or 'F# major'>
signature: <numerator only — one of 2, 3, 4, 6>

Caption rules:
- Line 1 is REQUIRED and must begin with `genre:`. Line 2 is REQUIRED and must begin with `caption:` followed by the description. Never omit it, never leave it blank, and never answer with the metadata fields alone. If you are unsure of everything else, still write the caption.
- The caption is 2 to 4 sentences, roughly 25 to 60 words, on a single line. Reference captions average about 30 words; a longer caption is not a better one, and padding it with invented detail is worse than stopping.
- Cover these, woven into flowing description rather than listed:
    - genre and subgenre, named plainly
    - the instruments actually present, named concretely
    - vocal character (or state that the track is instrumental and name what carries the lead line)
    - mood and atmosphere
    - production style and sonic character
- NEVER state BPM, key, or time signature in the caption text. They have dedicated fields below, and repeating them in the caption does not match how this model was trained.
- The genre you name in the caption MUST agree with the `genre:` field. Contradicting yourself between the two is worse than naming neither.
- Name things concretely: `808 bass`, `brushed snare`, `detuned saw lead`, `palm-muted guitar`, `upright piano` — not `interesting textures` or `lush soundscapes`.
- No vague imagery or stacked adjectives ('neon skies, electric hearts'), no marketing copy, and no listener-reaction language ('keeps you moving', 'emotionally resonant').
- Avoid generic openings like 'This track is' when more specific wording can be used immediately.
- If the track is instrumental, say so and name the instrument carrying the lead line.
- Start `genre:` on line 1, `caption:` on line 2, `bpm:` on line 3, `key:` on line 4, and `signature:` on line 5.
- Do not merge fields together. For example, do not output `genre: ... bpm: ... key: ...` on one line.
- Do not use markdown, bullets, numbering, code fences, labels before the template, or commentary after the template.
- Do not mention the artist name or song title in the caption.
- If audio is not attached or a field cannot be determined from available evidence, write N/A for that field instead of guessing.";

/// What MOSS heard in one recording, the numbers measured.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heard {
    pub caption: String,
    pub genre: String,
    pub bpm: u32,
    /// "A minor", in sharps.
    pub keyscale: String,
    /// The beats per bar the engine reads: "4" for 4/4, "6" for 6/8.
    pub timesignature: String,
}

/// One run of the captioner over several songs, the way HOT-Step labels a
/// dataset: the model loads once, and each song's caption is handed to
/// `done` as soon as it is written, with its index. `libraries` goes first
/// on PATH: the captioner's ggml-cuda imports cuBLAS from the engine's folder,
/// and without it ggml would quietly run the whole model on the processor.
pub fn hear_batch(
    exe: &Path,
    moss: &Path,
    libraries: Option<&Path>,
    audio: &[std::path::PathBuf],
    cancel: &std::sync::atomic::AtomicBool,
    mut done: impl FnMut(usize, Result<String>),
) -> Result<Vec<String>> {
    use std::io::BufRead;

    let work = tempfile::tempdir().context("make a working folder for the captioner")?;
    let prose_prompt = work.path().join("prompt.prose.txt");
    std::fs::write(&prose_prompt, PROSE_PROMPT)?;
    let list = work.path().join("songs.tsv");
    let listing: Vec<String> = audio.iter().enumerate().map(|(index, path)| format!("{}\t{}", path.display(), work.path().join(format!("{index}.txt")).display())).collect();
    std::fs::write(&list, listing.join("\n"))?;

    let mut command = Command::new(exe);
    command
        .arg("--models")
        .arg(moss)
        .arg("--src-list")
        .arg(&list)
        .args(["--mode", "prose", "--temperature", "0", "--rep-penalty", "1.0", "--freq-penalty", "0.3"])
        .arg("--prompt-file")
        .arg(format!("prose={}", prose_prompt.display()))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    if let Some(libraries) = libraries {
        let mut path = std::ffi::OsString::from(libraries.as_os_str());
        if let Some(existing) = std::env::var_os("PATH") {
            path.push(";");
            path.push(existing);
        }
        command.env("PATH", path);
    }
    quiet(&mut command);
    let mut child = command.spawn().with_context(|| format!("start {}", exe.display()))?;
    let stderr = child.stderr.take().context("the captioner's messages")?;

    // "[MOSS] prose   -> <dir>/3.prose.txt (6.1s)": a mode writes stem.mode.txt
    let written = Regex::new(r"^\[MOSS\]\s+(prose)\s+->\s+(.+?)\s+\([0-9.]+s\)\s*$").expect("valid regex");
    let mut reported = vec![false; audio.len()];
    let mut log: Vec<String> = Vec::new();
    for line in std::io::BufReader::new(stderr).lines() {
        let line = line.context("read the captioner's messages")?;
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            child.kill().ok();
            child.wait().ok();
            bail!("cancelled");
        }
        if let Some(found) = written.captures(&line) {
            let path = std::path::PathBuf::from(&found[2]);
            let index = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.split('.').next()).and_then(|stem| stem.parse::<usize>().ok());
            let Some(index) = index.filter(|&index| index < audio.len()) else { continue };
            reported[index] = true;
            let prose = std::fs::read_to_string(&path).map(|text| text.trim().to_string()).context("read the caption").and_then(|prose| {
                if prose.is_empty() {
                    bail!("the captioner returned an empty caption");
                }
                Ok(prose)
            });
            done(index, prose);
        } else if !line.starts_with("[MOSS] (") {
            log.push(line);
        }
    }
    let status = child.wait().context("wait for the captioner")?;
    let tail = log.iter().rev().take(8).rev().cloned().collect::<Vec<_>>().join(" | ");
    for (index, seen) in reported.iter().enumerate() {
        if !seen {
            done(index, Err(anyhow::anyhow!("the captioner wrote nothing for this song ({status}): {tail}")));
        }
    }
    Ok(log)
}

/// One song's caption, genre and metadata from what MOSS wrote, tempo and
/// key the measured ones; a field MOSS left as N/A stays empty.
pub fn heard(prose: &str, facts: &Facts) -> Result<Heard> {
    let fields = parse_prose(prose);
    let field = |key: &str| fields.get(key).cloned().filter(|value| !is_na(value)).unwrap_or_default();
    let caption = field("caption");
    if caption.is_empty() {
        bail!("the captioner wrote no caption: {prose}");
    }
    let signature = field("signature");
    let beats = signature.split('/').next().map(str::trim).filter(|beats| matches!(*beats, "2" | "3" | "4" | "6")).unwrap_or("4").to_string();
    Ok(Heard {
        caption: correct_facts_in_prose(&caption, facts),
        genre: field("genre"),
        bpm: facts.bpm,
        keyscale: crate::assistant::key_in_sharps(&facts.key).unwrap_or_default(),
        timesignature: beats,
    })
}

fn quiet(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command;
}

fn is_na(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("n/a") || value.trim().is_empty()
}

/// `genre:` / `caption:` / ... lines, keys lowercased; a line without a
/// label continues the previous field.
fn parse_prose(text: &str) -> HashMap<String, String> {
    let label = Regex::new(r"^\s*(genre|caption|bpm|key|signature)\s*:\s*(.*)$").expect("valid regex");
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut last: Option<String> = None;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let line = line.trim_start_matches(['*', '-', ' ']);
        if let Some(found) = label.captures(&line.to_lowercase()) {
            let key = found[1].to_string();
            let value = line[line.find(':').map(|at| at + 1).unwrap_or(0)..].trim().trim_matches('*').trim().to_string();
            fields.insert(key.clone(), value);
            last = Some(key);
        } else if let Some(key) = &last {
            let entry = fields.entry(key.clone()).or_default();
            entry.push(' ');
            entry.push_str(line);
        }
    }
    fields
}

/// mossCaption.ts correctFactsInProse: a stated tempo becomes the measured
/// one, and the first stated key too; chord spellings are left alone.
pub fn correct_facts_in_prose(text: &str, facts: &Facts) -> String {
    let bpm = Regex::new(r"\b(\d{2,3})(\s*)(BPM|bpm)\b").expect("valid regex");
    let out = bpm.replace_all(text, |found: &regex::Captures| format!("{}{}{}", facts.bpm, &found[2], &found[3])).into_owned();
    if facts.tonic().is_empty() {
        return out;
    }
    let key = Regex::new(r"\b([A-G][#b]?)\s+(major|minor)\b").expect("valid regex");
    key.replacen(&out, 1, facts.key.as_str()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts { bpm: 104, key: "E minor".into() }
    }

    #[test]
    fn the_caption_keeps_its_words_and_takes_the_measured_numbers() {
        let prose = "genre: indie pop, synth-pop\ncaption: A bright synth-pop song at 115 BPM in C minor with a breathy female vocal.\nbpm: 115\nkey: C minor\nsignature: 4";
        let heard = heard(prose, &facts()).unwrap();
        assert_eq!(heard.genre, "indie pop, synth-pop");
        assert_eq!(heard.caption, "A bright synth-pop song at 104 BPM in E minor with a breathy female vocal.");
        assert_eq!((heard.bpm, heard.keyscale.as_str(), heard.timesignature.as_str()), (104, "E minor", "4"));
        assert!(super::heard("genre: pop\ncaption: N/A", &facts()).is_err(), "no caption is not a caption");
    }

    #[test]
    fn the_plain_caption_splits_into_its_fields() {
        let fields = parse_prose("genre: indie pop, synth-pop\ncaption: A bright song.\nIt ends softly.\nbpm: 118\nkey: N/A");
        assert_eq!(fields["genre"], "indie pop, synth-pop");
        assert_eq!(fields["caption"], "A bright song. It ends softly.");
        assert!(is_na(&fields["key"]));
    }
}
