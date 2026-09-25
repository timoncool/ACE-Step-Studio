//! Describing a song by ear, as HOT-Step's Training Studio does.
//!
//! MOSS-Music-8B hears the recording through `ace-caption` (HOT-Step's native
//! GGML port) and writes, off one encode of the audio, a plain caption and a
//! MiniMax Music 3 structured caption. It is unreliable on numbers, so the
//! tempo and key it states are replaced with the ones `audio_facts` measured,
//! and the genre in Basic Attributes comes from the plain caption's tags.
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

/// mossCaption.ts MM3_INSTRUCTIONS.
pub const MM3_PROMPT: &str = "Listen to this track and write a Structured Caption in exactly the format below.

Use these three section labels on their own lines, with no markdown, no '#', and no extra sections:

Global Metadata
Basic Attributes: bpm is <N>. key is <X>, and scale is <major|minor>. <Genre / Subgenre>.
Global Emotional Progression: <how the emotional arc moves across the song>
Application Scenarios & Imagery: <where this music would be used; concrete visual imagery>
Sonics & Production Profile: <soundstage, density, frequency balance, dynamics, mix character>
Vocal Details
Vocal Gender & Timbre: <Singer A (Male/Female). timbre, register, texture>
Vocal Style: <delivery and how it evolves across sections>
Harmony/Backing Vocals: <layering, doubling, gang vocals, call-and-response, or state none>
Vocal FX: <reverb, delay, distortion, pitch correction, or state minimal>
Arrangement
Instrument Lifecycle Description (Primary/Secondary Layering):
Primary: <the instruments carrying the track and how they change>
Secondary: <supporting instruments and when they enter or drop>
Groove & Foundation Progression: <rhythm section, feel changes, section by section>
Embellishments, Textures & Spatial FX: <risers, sweeps, reverse reverb, glitches, ambience>

Rules:
- Describe only what you actually hear. Do not invent an exact BPM or key if unsure.
- If the track is instrumental, say so under Vocal Details and name the lead instrument.
- Do NOT quote or summarise the lyrics anywhere.
- Write flowing prose inside each field.
- Under 'Instrument Lifecycle Description (Primary/Secondary Layering):', write 'Primary:' and 'Secondary:' each at the start of its own line, exactly as the template shows. Never merge them into one paragraph.
- The caption ENDS after 'Embellishments, Textures & Spatial FX'. Do not append section names, chorus or verse labels, timestamps, or anything else after it.";

/// What MOSS heard in one recording.
#[derive(Clone, Debug, Default)]
pub struct Heard {
    /// The MiniMax structured caption, facts substituted.
    pub mm3: String,
}

/// What the captioner wrote for one song: the plain caption and the
/// structured one.
pub struct Captions {
    pub prose: String,
    pub mm3: String,
}

/// One run of the captioner over several songs, the way HOT-Step labels a
/// dataset: the model loads once, and each song's captions are handed to
/// `done` as soon as both are written, with its index. `libraries` goes first
/// on PATH: the captioner's ggml-cuda imports cuBLAS from the engine's folder,
/// and without it ggml would quietly run the whole model on the processor.
pub fn hear_batch(
    exe: &Path,
    moss: &Path,
    libraries: Option<&Path>,
    audio: &[std::path::PathBuf],
    cancel: &std::sync::atomic::AtomicBool,
    mut done: impl FnMut(usize, Result<Captions>),
) -> Result<Vec<String>> {
    use std::io::BufRead;

    let work = tempfile::tempdir().context("make a working folder for the captioner")?;
    let prose_prompt = work.path().join("prompt.prose.txt");
    let mm3_prompt = work.path().join("prompt.mm3.txt");
    std::fs::write(&prose_prompt, PROSE_PROMPT)?;
    std::fs::write(&mm3_prompt, MM3_PROMPT)?;
    let list = work.path().join("songs.tsv");
    let listing: Vec<String> = audio.iter().enumerate().map(|(index, path)| format!("{}\t{}", path.display(), work.path().join(format!("{index}.txt")).display())).collect();
    std::fs::write(&list, listing.join("\n"))?;

    let mut command = Command::new(exe);
    command
        .arg("--models")
        .arg(moss)
        .arg("--src-list")
        .arg(&list)
        .args(["--mode", "prose,mm3", "--temperature", "0", "--rep-penalty", "1.0", "--freq-penalty", "0.3"])
        .arg("--prompt-file")
        .arg(format!("prose={}", prose_prompt.display()))
        .arg("--prompt-file")
        .arg(format!("mm3={}", mm3_prompt.display()))
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

    // "[MOSS] mm3     -> <dir>/3.mm3.txt (6.1s)": several modes write stem.mode.txt
    let written = Regex::new(r"^\[MOSS\]\s+(prose|mm3)\s+->\s+(.+?)\s+\([0-9.]+s\)\s*$").expect("valid regex");
    let mut prose: Vec<Option<String>> = vec![None; audio.len()];
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
            let text = std::fs::read_to_string(&path).map(|text| text.trim().to_string());
            if &found[1] == "prose" {
                prose[index] = Some(text.unwrap_or_default());
                continue;
            }
            reported[index] = true;
            let captions = text.context("read the structured caption").and_then(|mm3| {
                if mm3.is_empty() {
                    bail!("the captioner returned an empty structured caption");
                }
                Ok(Captions { prose: prose[index].take().unwrap_or_default(), mm3 })
            });
            done(index, captions);
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

/// The structured caption of one song, the numbers MOSS guessed replaced
/// with the measured ones; the genre MOSS names first in its plain caption
/// is the one it heard.
pub fn heard(captions: &Captions, facts: &Facts) -> Heard {
    let fields = parse_prose(&captions.prose);
    let genre = fields.get("genre").cloned().filter(|value| !is_na(value)).unwrap_or_default();
    Heard { mm3: apply_facts(&captions.mm3, facts, &genre) }
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

const MM3_SECTIONS: [&str; 3] = ["Global Metadata", "Vocal Details", "Arrangement"];
const MM3_LABELS: [&str; 13] = [
    "Basic Attributes:",
    "Global Emotional Progression:",
    "Application Scenarios & Imagery:",
    "Sonics & Production Profile:",
    "Vocal Gender & Timbre:",
    "Vocal Style:",
    "Harmony/Backing Vocals:",
    "Vocal FX:",
    "Instrument Lifecycle Description (Primary/Secondary Layering):",
    "Primary:",
    "Secondary:",
    "Groove & Foundation Progression:",
    "Embellishments, Textures & Spatial FX:",
];
const ILD: &str = "Instrument Lifecycle Description (Primary/Secondary Layering):";

/// mossCaption.ts normalizeMm3Shape: one line per field, Primary/Secondary
/// on lines of their own, nothing after the Embellishments field.
pub fn normalize_mm3(mm3: &str) -> String {
    let mut lines: Vec<String> = mm3.lines().map(|line| line.trim().to_string()).filter(|line| !line.is_empty()).collect();
    lines = lines
        .into_iter()
        .flat_map(|line| {
            if line.starts_with(ILD) && line != ILD {
                vec![ILD.to_string(), line[ILD.len()..].trim().to_string()]
            } else {
                vec![line]
            }
        })
        .collect();
    lines = lines
        .into_iter()
        .flat_map(|line| {
            let mut out: Vec<String> = Vec::new();
            let mut current = line;
            for marker in [" Secondary:", " Primary:"] {
                if let Some(at) = current.find(marker).filter(|&at| at > 0) {
                    out.insert(0, current[at + 1..].to_string());
                    current = current[..at].trim().to_string();
                }
            }
            out.insert(0, current);
            out.into_iter().filter(|line| !line.is_empty()).collect::<Vec<_>>()
        })
        .collect();

    let group = Regex::new(r"^[A-Z][A-Za-z /()&'-]{0,34}: \S").expect("valid regex");
    let mut merged: Vec<String> = Vec::new();
    let mut in_ild = false;
    for line in lines {
        if line.starts_with("Instrument Lifecycle Description") {
            in_ild = true;
        } else if line.starts_with("Embellishments, Textures & Spatial FX:") {
            in_ild = false;
        }
        let structural = MM3_SECTIONS.contains(&line.as_str())
            || MM3_LABELS.iter().any(|label| line.starts_with(label))
            || (in_ild && group.is_match(&line));
        match merged.last_mut() {
            Some(previous) if !structural && !MM3_SECTIONS.contains(&previous.as_str()) => {
                previous.push(' ');
                previous.push_str(&line);
            }
            _ => merged.push(line),
        }
    }
    if let Some(end) = merged.iter().position(|line| line.starts_with("Embellishments, Textures & Spatial FX:")) {
        merged.truncate(end + 1);
        // A run of bare "Chorus:" labels after the last field merges into it
        let labels = Regex::new(r"(\s+[A-Z][^:.]{0,40}:)+$").expect("valid regex");
        let last = merged.last_mut().expect("the Embellishments line");
        *last = labels.replace(last, "").into_owned();
    }
    let dangling = Regex::new(r"^[^:]{1,40}:$").expect("valid regex");
    while merged.last().is_some_and(|line| dangling.is_match(line)) {
        merged.pop();
    }
    merged.join("\n")
}

/// mossCaption.ts buildBasicAttributes: `bpm is N. key is K, and scale is S. Genre.`
fn basic_attributes(facts: &Facts, genre: &str) -> String {
    let mut parts = vec![format!("bpm is {}.", facts.bpm)];
    if !facts.tonic().is_empty() {
        parts.push(format!("key is {}, and scale is {}.", facts.tonic(), facts.mode()));
    }
    let genre = genre.split(',').next().map(str::trim).filter(|genre| !genre.is_empty()).unwrap_or("Pop");
    let mut titled = genre.to_string();
    if let Some(first) = titled.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    parts.push(format!("{titled}."));
    format!("Basic Attributes: {}", parts.join(" "))
}

/// mossCaption.ts applyFactSubstitution: the Basic Attributes line rebuilt
/// from the measured facts, every other line's tempo and key claims corrected.
pub fn apply_facts(mm3: &str, facts: &Facts, genre: &str) -> String {
    let mut lines: Vec<String> = normalize_mm3(mm3).lines().map(str::to_string).collect();
    let built = basic_attributes(facts, genre);
    match lines.iter().position(|line| line.starts_with("Basic Attributes:")) {
        Some(at) => lines[at] = built,
        None => {
            let at = lines.iter().position(|line| line == "Global Metadata").map(|at| at + 1).unwrap_or(0);
            lines.insert(at, built);
        }
    }
    lines
        .into_iter()
        .map(|line| if line.starts_with("Basic Attributes:") { line } else { correct_facts_in_prose(&line, facts) })
        .collect::<Vec<_>>()
        .join("\n")
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
    fn the_guessed_numbers_give_way_to_the_measured_ones() {
        let mm3 = "Global Metadata\nBasic Attributes: BPM is 115. The key is C minor, and the scale is minor.\nGlobal Emotional Progression: Builds up at 115 BPM in C minor over Cm and Abmaj7.\nVocal Details\nVocal Style: Clear.\nArrangement\nInstrument Lifecycle Description (Primary/Secondary Layering): Primary: synths. Secondary: guitar.\nGroove & Foundation Progression: Steady.\nEmbellishments, Textures & Spatial FX: Sweeps.\nChorus:";
        let fixed = apply_facts(mm3, &facts(), "indie pop, synth-pop");
        assert!(fixed.contains("Basic Attributes: bpm is 104. key is E, and scale is minor. Indie pop."), "{fixed}");
        assert!(fixed.contains("at 104 BPM in E minor over Cm and Abmaj7"), "{fixed}");
        assert!(fixed.contains(&format!("{ILD}\nPrimary: synths.\nSecondary: guitar.")), "{fixed}");
        assert!(fixed.ends_with("Embellishments, Textures & Spatial FX: Sweeps."), "{fixed}");
    }

    #[test]
    fn the_plain_caption_splits_into_its_fields() {
        let fields = parse_prose("genre: indie pop, synth-pop\ncaption: A bright song.\nIt ends softly.\nbpm: 118\nkey: N/A");
        assert_eq!(fields["genre"], "indie pop, synth-pop");
        assert_eq!(fields["caption"], "A bright song. It ends softly.");
        assert!(is_na(&fields["key"]));
    }
}
