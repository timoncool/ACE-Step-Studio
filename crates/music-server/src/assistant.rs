//! The writing assistant: caption, lyrics and metadata for ACE-Step.
//!
//! ACE-Step plans songs with its own language model, so the assistant is an
//! option: a text model (local or OpenRouter) that turns an idea into the
//! request the form holds - a tag caption, sectioned lyrics, tempo, key, time
//! signature and duration. Its contract is ACE-Step Studio's own song-writer
//! prompt, the one the Python studio shipped, with the studio's field names.
//!
//! Two providers, both optional, because the manual form is the primary way in:
//!
//! * a local OpenAI-compatible server (llama.cpp, LM Studio, Ollama) - fully
//!   offline;
//! * OpenRouter, chosen from the live catalogue.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// ACE-Step Studio's song-writer contract: caption, lyrics, metadata, title
/// and cover prompt, with worked examples.
const GUIDE: &str = include_str!("../prompts/ace-song-writer.md");

/// One `# ` section of the guide, heading included.
fn part(heading: &str) -> &'static str {
    let start = GUIDE.find(&format!("\n# {heading}")).map(|index| index + 1).unwrap_or_else(|| {
        assert!(GUIDE.starts_with(&format!("# {heading}")), "the guide has no section {heading}");
        0
    });
    let rest = &GUIDE[start + 2..];
    let end = rest.find("\n# ").map(|index| start + 2 + index).unwrap_or(GUIDE.len());
    GUIDE[start..end].trim()
}

fn parts(headings: &[&str]) -> String {
    headings.iter().map(|heading| part(heading)).collect::<Vec<_>>().join("\n\n")
}

const CAPTION_PARTS: &[&str] = &["ACE-STEP XL PHILOSOPHY", "`caption` FIELD", "`bpm` FIELD", "`keyscale` FIELD", "`timesignature` FIELD", "`duration_seconds` FIELD"];
const LYRICS_PARTS: &[&str] = &["`lyrics` FIELD", "LANGUAGE HANDLING"];

/// Pronunciation, which the caption cannot reach: the engine reads the lyrics
/// as characters, so the only place to correct a mis-sung word is the word.
const DICTION_RULE: &str = r#"
Diction: the model sings the letters it is given. In Russian write ё as ё rather than е, and mark the stressed vowel with a combining acute - за́мок, замо́к - only where the word would otherwise be read wrong: homographs, rare words, proper names, and a word whose natural stress fights the beat. Never accent every word; a page of accents reads as noise."#;

/// How a recognised recording becomes a lyric sheet: the words stay the
/// singer's, only the layout is the assistant's.
const TRANSCRIPT_RULES: &str = r#"The transcript comes from speech recognition run on the vocals of a finished recording: one line per sung phrase, each after its start time, with the recogniser's mistakes. Write the lyric sheet of that recording exactly as it is sung. Keep the singer's words, in their order, their language and their alphabet - Cyrillic stays Cyrillic, never transliterate; correct a word only where the recognition is plainly wrong and the right word is certain from the line; never invent, rewrite, translate or complete lines, and drop fragments the recogniser picked up in instrumental passages. Leave the times out. Organise the lines into sections: a block of lines that returns is the [Chorus], written out every time it is sung; the blocks between choruses are verses numbered in order ([Verse 1], [Verse 2]); a block sung once that is neither is the [Bridge]; a block that leads into the chorus every time is the [Pre-Chorus]; lines before the first verse are the [Intro] and after the last chorus the [Outro]. Every section starts with its tag in square brackets, in English, on a line of its own, its lines follow below it, and a blank line separates sections. Use no other tags and no section names in words."#;

/// A dataset song is described from what was heard, as ACE-Step's own
/// captioner describes a recording.
const RECORDING_RULE: &str = "The song is a finished recording; describe what it sounds like, from its title, its notes and its lyrics, as tags an ACE-Step caption is written in.";

/// The writing guides an agent connected over MCP reads, by topic.
pub const GUIDE_TOPICS: &[(&str, &str)] = &[
    ("song", "writing a whole song for song_create: caption, lyrics, metadata, title, cover prompt"),
    ("caption", "the tag caption ACE-Step reads and the metadata fields beside it"),
    ("lyrics", "lyrics: section tags, density, language, instrumentals"),
    ("transcript", "turning recognised words into a lyric sheet"),
    ("sections", "marking the sections of a published lyric sheet without changing a word"),
];

/// The rules the studio's own assistant is prompted with, as a guide for an
/// agent connected over MCP: the same text, so an agent writes the way the
/// model expects.
pub fn writing_guide(topic: &str) -> Option<String> {
    Some(match topic {
        "song" => GUIDE.to_string(),
        "caption" => parts(CAPTION_PARTS),
        "lyrics" => format!("{}{DICTION_RULE}", parts(LYRICS_PARTS)),
        "transcript" => TRANSCRIPT_RULES.to_string(),
        "sections" => SHEET_SECTIONS_PROMPT.to_string(),
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistTarget {
    /// Write the whole song: caption, lyrics and metadata.
    All,
    /// Rewrite only the lyrics, keeping them coherent with the current caption.
    Lyrics,
    /// Rewrite only the caption and metadata, keeping them coherent with the lyrics.
    Prompt,
    /// Lay out a recording's recognised words as a lyric sheet.
    Transcript,
    /// Lay out a published lyric sheet in sections, its words untouched.
    Sheet,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssistRequest {
    pub target: AssistTarget,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub instruction: String,
    #[serde(default)]
    pub lyrics: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default = "default_duration")]
    pub duration_seconds: f64,
    #[serde(default)]
    pub instrumental: bool,
    /// The language the song is sung in, when the user chose one.
    #[serde(default)]
    pub vocal_language: String,
    /// A finished recording is being described, not a song written.
    #[serde(default)]
    pub recording: bool,
}

fn default_duration() -> f64 {
    120.0
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AssistDraft {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm: Option<u32>,
    /// "A minor", in the sharps the form lists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyscale: Option<String>,
    /// The beats per bar the engine reads: "4" for 4/4, "6" for 6/8.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timesignature: Option<String>,
    /// A name for the track. The model has the words and the mood in front of
    /// it; asking the user to invent one afterwards is asking twice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What the cover should show, in one sentence, ready for an image model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u32>,
}

const METADATA_KEYS: &str = "bpm, keyscale, timesignature, duration_seconds";

/// The system prompt and the JSON keys the answer must carry.
pub fn instructions(request: &AssistRequest) -> (String, &'static [&'static str]) {
    let references = references_for(request);
    let diction = if request.instrumental { "" } else { DICTION_RULE };
    match request.target {
        AssistTarget::Lyrics => (
            format!(
                "You write lyrics for ACE-Step 1.5, a caption+lyrics music generation model.\n\
                 Given a lyrics instruction, the current caption and a target duration, write lyrics coherent with the caption.\n\n\
                 {}{diction}\n\n\
                 Answer with ONLY a JSON object with key: lyrics.",
                parts(LYRICS_PARTS)
            ),
            &["lyrics"],
        ),
        AssistTarget::Prompt => (
            format!(
                "You write the caption and the metadata for ACE-Step 1.5, a caption+lyrics music generation model.\n\
                 Given a sound instruction and/or lyrics, write the caption and choose {METADATA_KEYS}.{}\n\n\
                 {}\n\n{}\n\n{}\n\n\
                 Answer with ONLY a JSON object with keys: caption, {METADATA_KEYS}, title, cover_prompt.{references}",
                if request.recording { format!(" {RECORDING_RULE}") } else { String::new() },
                parts(CAPTION_PARTS),
                part("`title` FIELD"),
                part("OUTPUT FORMAT"),
            ),
            &["caption"],
        ),
        AssistTarget::Transcript => (
            format!(
                "You prepare training data for ACE-Step 1.5, a model that learns songs from their audio, their captions and their lyric sheets.\n\
                 {TRANSCRIPT_RULES}\n\
                 Answer with ONLY a JSON object with key: lyrics."
            ),
            &["lyrics"],
        ),
        // a sheet asks for boundaries only; see `sheet_in_sections`
        AssistTarget::Sheet => (SHEET_SECTIONS_PROMPT.to_string(), &["sections"]),
        AssistTarget::All => (format!("{GUIDE}{diction}{references}"), &["lyrics", "caption"]),
    }
}

/// Complete official ACE-Step requests close to the brief, as the model was
/// shown them: examples of density and shape, not text to copy.
fn references_for(request: &AssistRequest) -> String {
    let brief = format!("{} {} {}", request.description, request.instruction, request.caption);
    let references = crate::skill::references(&brief);
    if references.is_empty() {
        return String::new();
    }
    let mut block = String::from("\n\n# REFERENCE REQUESTS\n\nOfficial ACE-Step examples close to this request. Use them for the level of musical detail and the section logic; do not copy their sentences, story or lyrics.\n");
    for (index, reference) in references.iter().enumerate() {
        block.push_str(&format!("\n--- reference {} ---\ncaption: {}\nlyrics:\n{}\n", index + 1, reference.caption.trim(), reference.lyrics.trim()));
    }
    block
}

pub fn user_message(request: &AssistRequest) -> String {
    let instruction = request.instruction.trim();
    let description = request.description.trim();
    let brief = if !instruction.is_empty() { instruction } else { description };
    let instrumental = if request.instrumental { "\nThis piece is instrumental: lyrics is exactly [Instrumental]." } else { "" };
    let language = match request.vocal_language.trim() {
        "" | "unknown" => String::new(),
        code => format!("\nThe lyrics are sung in the language with the code {code}."),
    };

    match request.target {
        AssistTarget::Lyrics => format!(
            "Lyrics instruction: {}\nCurrent caption, keep the lyrics coherent with it:\n{}\nTarget duration: {} seconds.{language}{instrumental}",
            if brief.is_empty() { "(none - write lyrics that fit the caption)" } else { brief },
            request.caption.trim(),
            request.duration_seconds.round() as i64,
        ),
        AssistTarget::Transcript => format!("Transcript:\n{description}"),
        AssistTarget::Sheet => format!("Lyric sheet:\n{description}"),
        AssistTarget::Prompt => format!(
            "Sound instruction: {}\nCurrent lyrics, keep the caption coherent with them:\n{}{instrumental}",
            if brief.is_empty() { "(none - describe a sound that fits the lyrics)" } else { brief },
            request.lyrics.trim(),
        ),
        AssistTarget::All => {
            // Whatever the user already wrote is material, not noise: it goes to
            // the model so the rest is built around it instead of replacing it.
            let mut carried = String::new();
            let mut carry = |label: &str, value: &str| {
                let value = value.trim();
                if !value.is_empty() {
                    carried.push_str(&format!("\n{label} (the user wrote this - keep it, build around it):\n{value}"));
                }
            };
            carry("Lyrics", &request.lyrics);
            carry("Caption", &request.caption);
            format!(
                "Song description: {}{carried}{language}{instrumental}",
                if brief.is_empty() { "(none - choose something musical and specific)" } else { brief },
            )
        }
    }
}

/// "4/4" is the engine's "4"; "6/8" its "6".
fn beats(value: &str) -> Option<String> {
    let head = value.trim().split('/').next()?.trim();
    matches!(head, "2" | "3" | "4" | "6").then(|| head.to_owned())
}

/// "Eb major" is the form's "D# major"; anything but a note and a mode is dropped.
pub(crate) fn key_in_sharps(value: &str) -> Option<String> {
    let mut words = value.split_whitespace();
    let note = words.next()?.replace('♭', "b").replace('♯', "#");
    let mode = words.next()?.to_lowercase();
    if mode != "major" && mode != "minor" {
        return None;
    }
    let mut letters = note.chars();
    let letter = letters.next()?.to_ascii_uppercase();
    let accidental: String = letters.collect();
    const SHARPS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let natural = SHARPS.iter().position(|name| *name == letter.to_string())?;
    let index = match accidental.as_str() {
        "" => natural,
        "#" => (natural + 1) % 12,
        "b" => (natural + 11) % 12,
        _ => return None,
    };
    Some(format!("{} {mode}", SHARPS[index]))
}

/// Extracts the answer, tolerating a model that wraps its JSON in prose or a
/// code fence - a local 4B model does that more often than a hosted one.
pub fn parse_draft(content: &str, required: &[&str]) -> Result<AssistDraft> {
    let start = content.find('{').context("the assistant returned no JSON object")?;
    let end = content.rfind('}').context("the assistant returned no JSON object")?;
    if end <= start {
        bail!("the assistant returned no JSON object");
    }
    let value: Value = serde_json::from_str(&content[start..=end]).with_context(|| {
        let sample: String = content.chars().take(220).collect();
        format!("the assistant returned invalid JSON. It answered: {sample}")
    })?;
    // A model answers with what it finds natural: a string for the caption, and
    // very often an array of lines for the lyrics. Both are the same lyric.
    let field = |key: &str| -> Option<String> {
        let text = match value.get(key)? {
            Value::String(text) => text.trim().to_owned(),
            Value::Array(items) => items.iter().filter_map(|item| item.as_str()).collect::<Vec<_>>().join("\n").trim().to_owned(),
            Value::Number(number) => number.to_string(),
            _ => return None,
        };
        (!text.is_empty()).then_some(text)
    };
    let number = |key: &str| value.get(key).and_then(|value| value.as_u64().or_else(|| value.as_f64().map(|f| f.round() as u64)).or_else(|| value.as_str().and_then(|text| text.trim().parse().ok())));

    for key in required {
        if field(key).is_none() {
            bail!("the assistant answer is missing '{key}'");
        }
    }
    Ok(AssistDraft {
        lyrics: field("lyrics"),
        caption: field("caption"),
        bpm: number("bpm").filter(|bpm| (30..=300).contains(bpm)).map(|bpm| bpm as u32),
        keyscale: field("keyscale").and_then(|key| key_in_sharps(&key)),
        timesignature: field("timesignature").and_then(|signature| beats(&signature)),
        title: field("title"),
        cover_prompt: field("cover_prompt"),
        duration_seconds: number("duration_seconds").map(|seconds| seconds.clamp(10, 600) as u32),
    })
}

/// The shape the answer must have, as a schema the server can enforce.
///
/// llama-server turns this into grammar rules and applies them while sampling,
/// so a local model cannot answer with prose, with a fenced block, or with a
/// list where a string belongs.
pub fn draft_schema(required: &[&str]) -> Value {
    // A minimum length, not just a type: "required" only forces the key to be
    // present, and a model that answers with an empty string satisfies that.
    let text = serde_json::json!({ "type": "string", "minLength": 20 });
    let lyric = serde_json::json!({ "type": "string", "minLength": 14 });
    let short = serde_json::json!({ "type": "string", "minLength": 1 });
    serde_json::json!({
        "type": "object",
        "properties": {
            "lyrics": lyric,
            "caption": text,
            "bpm": { "type": "integer" },
            "keyscale": short,
            "timesignature": short,
            "title": short,
            "cover_prompt": short,
            "duration_seconds": { "type": "integer" },
        },
        "required": required,
        "additionalProperties": false,
    })
}

/// An OpenAI-compatible chat request. Both providers speak this shape, so the
/// only difference between them is the endpoint and the credential.
pub fn chat_body(model: &str, system: &str, user: &str) -> Value {
    chat_body_with_reasoning(model, system, user, None)
}

/// The same body, asking the model to think harder.
///
/// `effort` is OpenRouter's unified reasoning control - "minimal" through
/// "max" - which they translate per provider. A local OpenAI-compatible server
/// has no such parameter, so nothing is sent there and the model decides for
/// itself; its thinking is read back out of `reasoning_content` either way.
pub fn chat_body_with_reasoning(model: &str, system: &str, user: &str, effort: Option<&str>) -> Value {
    chat_body_full(model, system, user, effort, None)
}

/// The request as it goes out, with the model's own sampling when it has any.
///
/// OpenRouter publishes `default_parameters` per model, and 83 of them fill it
/// in. Sending one hardcoded temperature to every model overrides what the
/// model asks for; the studio's own value is only a fallback for models that
/// publish nothing.
pub fn chat_body_full(
    model: &str,
    system: &str,
    user: &str,
    effort: Option<&str>,
    defaults: Option<&Value>,
) -> Value {
    chat_body_constrained(model, system, user, effort, defaults, None)
}

/// The same request, with the answer's shape enforced where the server can do
/// it. Asking politely for JSON in the prompt is a hope; a schema is a rule.
pub fn chat_body_constrained(
    model: &str,
    system: &str,
    user: &str,
    effort: Option<&str>,
    defaults: Option<&Value>,
    schema: Option<Value>,
) -> Value {
    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
        "stream": false,
    });

    let mut published = false;
    if let Some(Value::Object(map)) = defaults {
        for (key, value) in map {
            if value.is_null() {
                continue;
            }
            body[key] = value.clone();
            published = true;
        }
    }
    if !published {
        // Nothing published: a little warmth, because these are lyrics.
        body["temperature"] = Value::from(0.8);
    }
    if let Some(effort) = effort.filter(|value| !value.trim().is_empty() && *value != "off") {
        // The draft is what is wanted, not the thinking: exclude keeps the
        // response small and the parser looking in one place.
        //
        // Only for a model that says it takes this. OpenRouter publishes
        // `supported_parameters` for every model and 182 of the 468 do not
        // list reasoning; sending it to those is asking for something they
        // never offered.
        body["reasoning"] = serde_json::json!({ "effort": effort, "exclude": true });
    }
    if let Some(schema) = schema {
        // llama-server reads the schema from `json_schema.schema`, the OpenAI
        // shape; beside `type` it is ignored and any JSON object passes
        body["response_format"] = serde_json::json!({ "type": "json_schema", "json_schema": { "name": "answer", "strict": true, "schema": schema } });
    }

    body
}

/// Reads the answer out of a chat completion.
///
/// Reasoning models served by llama.cpp put their visible answer in
/// `content` and their thinking in `reasoning_content` - but with several
/// Gemma builds `content` comes back empty and everything, the JSON draft
/// included, arrives in `reasoning_content`. Reading only `content` there
/// looks exactly like a model that answered nothing.
pub fn content_of(response: &Value) -> Result<String> {
    let message = response
        .pointer("/choices/0/message")
        .context("the assistant response contained no message")?;
    // OpenRouter calls it `reasoning`, llama.cpp `reasoning_content`; both
    // appear when a model answers with its thinking and an empty content.
    for field in ["content", "reasoning_content", "reasoning"] {
        if let Some(text) = message.get(field).and_then(Value::as_str) {
            if !text.trim().is_empty() {
                return Ok(text.to_owned());
            }
        }
    }
    Err(anyhow!("the assistant response contained no message content"))
}

/// Sampling fitted to the task: laying out a transcript is copying, not
/// writing, so it runs cold whatever the model publishes. The length is
/// bounded by what the task can need, so a model looping on one line fails in
/// seconds instead of at the request timeout.
pub fn fit_to_task(mut body: Value, target: AssistTarget) -> Value {
    if matches!(target, AssistTarget::Transcript | AssistTarget::Sheet) {
        body["temperature"] = Value::from(0.2);
        body["max_tokens"] = Value::from(4096);
    }
    // A caption is a few hundred words; a small model that starts repeating
    // itself inside a JSON string otherwise runs until the request times out.
    if target == AssistTarget::Prompt {
        body["max_tokens"] = Value::from(2048);
    }
    body
}

/// A Cyrillic word with Latin look-alikes in it ("Tут", "oстов"), which a
/// small model writes at the start of a line, spelled in Cyrillic. Only the
/// letters that are one letter both by sight and by sound are swapped: a
/// Latin H stands for Н as often as for Х, and is left for the eye.
pub fn cyrillic_homoglyphs(text: &str) -> String {
    let swap = |c: char| match c {
        'A' => 'А', 'C' => 'С', 'E' => 'Е', 'K' => 'К', 'M' => 'М', 'O' => 'О', 'P' => 'Р', 'T' => 'Т', 'X' => 'Х',
        'a' => 'а', 'c' => 'с', 'e' => 'е', 'o' => 'о', 'p' => 'р', 'x' => 'х', 'y' => 'у',
        other => other,
    };
    let cyrillic = |c: char| ('\u{0400}'..='\u{04FF}').contains(&c);
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.chars().any(cyrillic) && word.chars().any(|c| c.is_ascii_alphabetic()) {
            out.extend(word.chars().map(swap));
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_alphabetic() {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// A published sheet is laid out by asking for its section boundaries only:
/// its lines go in numbered and what comes back is where each section starts
/// and what it is. The studio puts the sheet's own lines under the tags, so no
/// word can be changed, dropped or merged, whatever the model.
pub const SHEET_SECTIONS_PROMPT: &str = r#"You mark the sections of a published lyric sheet. Its lines are numbered. Do not rewrite anything; answer only which lines form each section, in order. A line marked [xN] is sung N times in the song: a block of such lines is the chorus, marked every time it returns, and the lines between two choruses are one verse, not several. A block of lines that returns is a chorus, every time it is sung; the blocks between choruses are verses; a block sung once that is neither is a bridge; a block that leads into the chorus every time is a pre-chorus; lines before the first verse are the intro and after the last chorus the outro. Every line belongs to exactly one section: the first section starts at line 1, each next one starts right after the previous one ends, and the last ends at the last line.
Answer with ONLY a JSON object: {"sections": [{"kind": "verse", "from": 1, "to": 4}, {"kind": "chorus", "from": 5, "to": 8}]}, where kind is one of intro, verse, pre-chorus, chorus, bridge, outro."#;

const SECTION_KINDS: [&str; 6] = ["intro", "verse", "pre-chorus", "chorus", "bridge", "outro"];

/// The sheet's lines as the model reads them: "1. first line".
pub fn numbered_lines(lines: &[&str]) -> String {
    // a small model does not see a returning block in a plain list; each line
    // sung more than once says how many times, so the chorus stands out
    let key = |line: &str| line.to_lowercase().replace('ё', "е").chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for line in lines {
        *counts.entry(key(line)).or_default() += 1;
    }
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| match counts[&key(line)] {
            1 => format!("{}. {line}", index + 1),
            times => format!("{}. {line} [x{times}]", index + 1),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The answer the model is held to when it runs locally.
pub fn sheet_sections_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "sections": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": SECTION_KINDS },
                        "from": { "type": "integer", "minimum": 1 },
                        "to": { "type": "integer", "minimum": 1 },
                    },
                    "required": ["kind", "from", "to"],
                },
            },
        },
        "required": ["sections"],
    })
}

/// The lines of a lyric sheet without its section tags: a line that is only a
/// bracketed tag goes, the words stay as they are.
pub fn without_section_tags(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let line = line.trim();
            !(line.starts_with('[') && line.ends_with(']') && !line[1..line.len() - 1].contains(['[', ']']))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The sheet in sections from where the model says each one starts: a
/// section runs to the line before the next one, so every line is kept once,
/// in order, gaps and overlaps in the answer notwithstanding. None when the
/// answer marks no section.
pub fn sheet_in_sections(answer: &str, lines: &[&str]) -> Option<String> {
    let value: Value = serde_json::from_str(answer.get(answer.find('{')?..=answer.rfind('}')?)?).ok()?;
    let mut starts: Vec<(usize, String)> = value
        .get("sections")?
        .as_array()?
        .iter()
        .filter_map(|section| {
            let kind = section.get("kind")?.as_str()?.trim().to_lowercase();
            let from = section.get("from")?.as_u64()? as usize;
            Some((from, if SECTION_KINDS.contains(&kind.as_str()) { kind } else { "verse".to_string() }))
        })
        .filter(|(from, _)| *from >= 1 && *from <= lines.len())
        .collect();
    starts.sort_by_key(|(from, _)| *from);
    starts.dedup_by_key(|(from, _)| *from);
    let first = starts.first_mut()?;
    first.0 = 1;
    let mut verse = 0;
    let blocks: Vec<String> = starts
        .iter()
        .enumerate()
        .map(|(index, (from, kind))| {
            let to = starts.get(index + 1).map_or(lines.len(), |(next, _)| next - 1);
            if kind == "verse" {
                verse += 1;
            }
            format!("[{}]\n{}", section_tag(kind, verse), lines[from - 1..to].join("\n"))
        })
        .collect();
    Some(blocks.join("\n\n"))
}

/// ACE-Step's tags, as its examples write them: capitalised, verses numbered.
fn section_tag(kind: &str, verse: usize) -> String {
    match kind {
        "verse" => format!("Verse {verse}"),
        "pre-chorus" => "Pre-Chorus".to_string(),
        "post-chorus" => "Post-Chorus".to_string(),
        other => {
            let mut letters = other.chars();
            letters.next().map(|first| first.to_uppercase().collect::<String>() + letters.as_str()).unwrap_or_default()
        }
    }
}

/// The share of letters in a text that are Cyrillic, to tell a lyric sheet
/// that kept its alphabet from one the model transliterated.
pub fn cyrillic_share(text: &str) -> f64 {
    let (mut cyrillic, mut letters) = (0usize, 0usize);
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        letters += 1;
        if ('\u{0400}'..='\u{04FF}').contains(&c) {
            cyrillic += 1;
        }
    }
    if letters == 0 { 0.0 } else { cyrillic as f64 / letters as f64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_tags_go_and_the_words_stay() {
        let sheet = "[Verse 1]\nСреди связок\n[x] в горле\n\n[Chorus]\nНо настала пора";
        assert_eq!(without_section_tags(sheet), "Среди связок\n[x] в горле\n\nНо настала пора");
    }

    #[test]
    fn a_sheet_is_cut_where_the_sections_start() {
        let lines = ["Шёл я как-то по лесу,", "Шёл по грибы", "И тут раз", "Конец"];
        let answer = r#"{"sections": [{"kind": "verse", "from": 1, "to": 2}, {"kind": "Chorus", "from": 3, "to": 3}, {"kind": "coda", "from": 4, "to": 4}]}"#;
        assert_eq!(sheet_in_sections(answer, &lines).as_deref(), Some("[Verse 1]\nШёл я как-то по лесу,\nШёл по грибы\n\n[Chorus]\nИ тут раз\n\n[Verse 2]\nКонец"));
        assert_eq!(numbered_lines(&["Ой, да!", "Куплет", "ой да"]), "1. Ой, да! [x2]\n2. Куплет\n3. ой да [x2]");
    }

    #[test]
    fn an_answer_that_arrives_as_reasoning_is_still_an_answer() {
        let response = serde_json::json!({
            "choices": [{ "message": { "content": "", "reasoning_content": "{\"lyrics\": \"[verse]\"}" } }]
        });
        assert_eq!(content_of(&response).unwrap(), "{\"lyrics\": \"[verse]\"}");

        let empty = serde_json::json!({ "choices": [{ "message": { "content": "  " } }] });
        assert!(content_of(&empty).is_err());
    }

    fn request(target: AssistTarget) -> AssistRequest {
        AssistRequest {
            target,
            description: "a night drive synth pop song".into(),
            instruction: String::new(),
            lyrics: "[Verse]\nneon".into(),
            caption: "synthwave, female vocals, analog bass".into(),
            duration_seconds: 90.0,
            instrumental: false,
            vocal_language: String::new(),
            recording: false,
        }
    }

    #[test]
    fn every_section_the_prompts_cut_from_the_guide_is_there() {
        for heading in CAPTION_PARTS.iter().chain(LYRICS_PARTS).chain(["`title` FIELD", "OUTPUT FORMAT"].iter()) {
            assert!(part(heading).starts_with(&format!("# {heading}")), "no section {heading}");
        }
        assert!(!part("`caption` FIELD").contains("# `lyrics` FIELD"), "a section runs into the next one");
    }

    /// The studio's own song-writer contract reaches the model, whichever
    /// provider answers: the same system message goes to every one of them.
    #[test]
    fn the_ace_contract_is_in_the_request_that_goes_out() {
        let (system, _) = instructions(&request(AssistTarget::All));
        assert!(system.contains("Metadata ≠ caption"), "the whole guide is missing");
        assert!(system.contains("\"keyscale\""), "the field names are not the studio's");
        assert!(!system.contains("\"tags\""), "the tags field is not part of the studio's request");
        let body = chat_body("any-model", &system, "idea");
        assert!(body["messages"][0]["content"].as_str().unwrap_or_default().contains("# `caption` FIELD"));

        let (caption, _) = instructions(&request(AssistTarget::Prompt));
        assert!(caption.contains("# `bpm` FIELD") && !caption.contains("# `lyrics` FIELD"));
    }

    #[test]
    fn a_brief_brings_official_examples_along() {
        let mut dark = request(AssistTarget::All);
        dark.description = "aggressive heavy metal with distorted guitars".into();
        let (system, _) = instructions(&dark);
        assert!(system.contains("--- reference 1 ---"), "no official example travelled with the brief");
    }

    #[test]
    fn every_target_declares_the_fields_it_writes() {
        assert_eq!(instructions(&request(AssistTarget::Lyrics)).1, &["lyrics"]);
        assert_eq!(instructions(&request(AssistTarget::Prompt)).1, &["caption"]);
        assert_eq!(instructions(&request(AssistTarget::All)).1, &["lyrics", "caption"]);
    }

    #[test]
    fn the_other_half_of_the_song_travels_as_context() {
        let lyrics_message = user_message(&request(AssistTarget::Lyrics));
        assert!(lyrics_message.contains("synthwave, female vocals"));
        assert!(lyrics_message.contains("90 seconds"));
        assert!(user_message(&request(AssistTarget::Prompt)).contains("[Verse]"));
    }

    #[test]
    fn the_chosen_language_and_an_instrumental_are_said_to_the_model() {
        let mut sung = request(AssistTarget::All);
        sung.vocal_language = "ru".into();
        assert!(user_message(&sung).contains("code ru"));
        sung.instrumental = true;
        assert!(user_message(&sung).contains("[Instrumental]"));
        let (system, _) = instructions(&sung);
        assert!(!system.contains("combining acute"), "an instrumental was given a diction rule");
    }

    #[test]
    fn a_sung_request_carries_the_diction_rule() {
        let (system, _) = instructions(&request(AssistTarget::All));
        assert!(system.contains("combining acute") && system.contains("ё"));
    }

    #[test]
    fn json_survives_a_code_fence_and_surrounding_prose() {
        let draft = parse_draft("Sure!\n```json\n{\"lyrics\": \"[Verse]\\nline\"}\n```\nHope that helps.", &["lyrics"]).unwrap();
        assert_eq!(draft.lyrics.unwrap(), "[Verse]\nline");
    }

    #[test]
    fn a_missing_required_field_is_an_error_rather_than_a_blank_pane() {
        assert!(parse_draft("{\"lyrics\": \"x\"}", &["caption"]).is_err());
        assert!(parse_draft("no json here", &["lyrics"]).is_err());
    }

    #[test]
    fn lyrics_may_arrive_as_a_list_of_lines() {
        let draft = parse_draft(r#"{"lyrics": ["[Intro]", "[Verse]", "neon on the wet road"]}"#, &["lyrics"]).unwrap();
        assert_eq!(draft.lyrics.as_deref(), Some("[Intro]\n[Verse]\nneon on the wet road"));
    }

    /// The guide asks for "4/4" and "Eb major"; the engine reads "4" and the
    /// form lists sharps.
    #[test]
    fn metadata_arrives_in_the_engine_s_own_terms() {
        let draft = parse_draft(r#"{"caption": "synthwave", "bpm": "118", "keyscale": "Eb major", "timesignature": "6/8", "duration_seconds": 700}"#, &["caption"]).unwrap();
        assert_eq!(draft.bpm, Some(118));
        assert_eq!(draft.keyscale.as_deref(), Some("D# major"));
        assert_eq!(draft.timesignature.as_deref(), Some("6"));
        assert_eq!(draft.duration_seconds, Some(600));
        assert_eq!(key_in_sharps("a minor").as_deref(), Some("A minor"));
        assert_eq!(key_in_sharps("B♭ minor").as_deref(), Some("A# minor"));
        assert!(key_in_sharps("A dorian").is_none());
        assert!(beats("7/8").is_none());
    }

    /// "required" alone let the model answer with an empty string and still
    /// satisfy the schema, which is how a blank description came back.
    /// Two things this request has to get right about a model it did not
    /// choose: use what the model published for itself, and ask for thinking
    /// only where thinking is offered.
    #[test]
    fn a_model_that_published_nothing_gets_the_studio_s_own_warmth() {
        let plain = chat_body_constrained("m", "s", "u", None, None, None);
        assert_eq!(plain["temperature"], serde_json::json!(0.8));

        let published = serde_json::json!({ "temperature": 0.6, "top_p": 0.95 });
        let honoured = chat_body_constrained("m", "s", "u", None, Some(&published), None);
        assert_eq!(honoured["temperature"], serde_json::json!(0.6), "the model's own figure wins");
        assert_eq!(honoured["top_p"], serde_json::json!(0.95));
    }

    #[test]
    fn reasoning_is_only_asked_of_models_that_take_it() {
        let asked = chat_body_constrained("m", "s", "u", Some("high"), None, None);
        assert_eq!(asked["reasoning"], serde_json::json!({ "effort": "high", "exclude": true }));

        // The caller passes None for a model whose supported_parameters do not
        // list reasoning, and for "off".
        let quiet = chat_body_constrained("m", "s", "u", None, None, None);
        assert!(quiet.get("reasoning").is_none(), "nothing is asked of a model that does not offer it");
        let switched_off = chat_body_constrained("m", "s", "u", Some("off"), None, None);
        assert!(switched_off.get("reasoning").is_none());
    }

    #[test]
    fn the_schema_asks_for_content_not_just_a_key() {
        let schema = draft_schema(&["caption"]);
        assert_eq!(schema["properties"]["caption"]["minLength"], 20);
        assert_eq!(schema["required"][0], "caption");
    }
}
