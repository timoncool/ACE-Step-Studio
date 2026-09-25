//! The writing assistant: lyrics and the structured caption.
//!
//! MiniMax Music 3 does not write text. Its language model emits audio codes,
//! so every project that offers "a song from one line" uses a separate text
//! model for it — MiniMax's own demo included. This module reproduces that
//! step with the contract the official demo uses, so the caption it produces is
//! the shape the model was trained on.
//!
//! Two providers, both optional, because the manual form is the primary way in:
//!
//! * a local OpenAI-compatible server (llama.cpp, LM Studio, Ollama) — fully
//!   offline, the same approach Dub Studio takes for its own text model;
//! * OpenRouter, chosen from the live catalogue.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The caption contract, transcribed from MiniMax's official demo so the three
/// fields carry exactly the labelled structure the model expects.
/// The two extras every draft carries: a name for the track and a sentence the
/// image model can draw from.
const EXTRA: &str = "title: a short song title, two to five words, no quotation marks, in the language of the lyrics. cover_prompt: one sentence describing a cover image for this track - a scene, not a poster; no text, no lettering, no logos. duration_seconds: how long a track of this genre and arrangement normally runs, in seconds, between 30 and 360.";

const VALIDATION: &str = "Before answering, check your own draft: every explicit user constraint kept, an instrumental request still instrumental, vocal gender not contradicted, every section tag present in its own section, no lyric line quoted or summarised, no song title inside the caption fields, no invented exact BPM or key, and no sentence copied from a reference. Fix what fails, then answer.";

const CAPTION_CONTRACT: &str = r#"The three caption fields follow the exact labeled style the model was trained on, and the rules below are MiniMax's own, from the music-caption-rewriter skill they publish with the model.

Be concrete and musical: describe an energy arc and instrument lifecycles, never a static equipment list or decorative adjectives. Preserve every explicit user constraint - an instrumental request stays instrumental, and a required vocal gender, tempo limit, required instrument or exclusion is never reversed. Do not invent a precise key, BPM, vocal gender or production technique when a broader description is sufficient; use a range or a qualitative tempo instead. Never quote, paraphrase or summarise a lyric line inside the caption, and never include a song title or track id. Total caption length roughly 250-450 English words. Write in English unless the user explicitly asks for another language.

global_metadata: genre and subgenres, tempo, emotional progression, and the overall sonic and production profile, in this order: "Basic Attributes: bpm is <number or range>. key is <letter>, and scale is <major|minor>. <Genre / Subgenre>." then "Global Emotional Progression: <how the emotion evolves from the opening through the final section>." then "Application Scenarios & Imagery: <two or three vivid listening scenarios>." then "Sonics & Production Profile: <soundstage, frequency balance, dynamics, production character>." Include key and scale only when explicit or musically useful.

vocal_details: for vocal music describe the lead configuration, timbre, register, delivery, harmony or backing vocals and restrained vocal effects: "Vocal Gender & Timbre: Singer A (<Male|Female>), <timbre and register>." then "Vocal Style: <delivery, and how it shifts per section>." then "Harmony/Backing Vocals: <where harmonies or doubles appear and their character>." then "Vocal FX: <restrained treatment: reverb, delay, light compression>." For instrumental music state that the piece is instrumental and name the instrument or texture carrying the lead melodic role. Do not invent lyrical subject matter.

arrangement: the song as a section-by-section timeline: "Instrument Lifecycle Description (Primary/Secondary Layering): Primary: <core instruments present start to finish and their role>. Secondary: <instruments that enter, exit or intensify, and in which sections>." then "Groove & Foundation Progression: <how drums, bass and groove develop across sections>." then "Embellishments, Textures & Spatial FX: <fills, textures, transitional gestures, stereo and space treatment where relevant>." For every section say what enters, exits, changes or intensifies, aligned with the lyric section tags, and keep transitions musically plausible. Prefer concrete musical changes over decorative prose."#;

/// The lyric rules, likewise transcribed: the tag vocabulary and the structure
/// sizing are what keep the sung result aligned with the requested length.
const LYRICS_RULES: &str = r#"lyrics: singable lyrics using ONLY these section tags, each ALWAYS ALONE on its own line: [intro] [verse] [pre-chorus] [chorus] [post-chorus] [bridge] [instrumental] [solo] [outro]. Never put words on the same line as a tag - the engine keeps the tag and throws that line's words away. Size the structure to the duration: <=30s: one verse + one chorus; ~60s: verse/pre-chorus/chorus/verse/chorus; >=120s: full structure with bridge and outro. Roughly 12-16 sung words per 10 seconds, and keep neighbouring lines close in length: a line much denser than the one before it gets sung rushed. The engine does not budget time - it sings until the clock runs out and stops there, mid-phrase if it has to - so write slightly less than the duration allows and never leave the song's payoff line for the outro. Musical instructions (tempo, instruments, dynamics) never belong in the lyrics. If the song is instrumental, write the same structure a sung song would have - [intro] [verse] [chorus] [bridge] [outro] - with no words under any of them, and use [instrumental] or [solo] only where a real instrumental passage belongs, the way a band would play one. Alternating [instrumental] with every other tag is not what the tag is for. Write the lyrics in the language the user wrote their request in: a Russian idea gets Russian lyrics, a Japanese one Japanese. The caption fields stay English - that is what the engine reads - but nobody asked for an English song."#;

/// Pronunciation, which the caption cannot reach: the engine reads the lyrics
/// as characters, so the only place to correct a mis-sung word is the word.

const DICTION_RULE: &str = r#"
Diction: the model sings the letters it is given. In Russian write ё as ё rather than е, and mark the stressed vowel with a combining acute - за́мок, замо́к - only where the word would otherwise be read wrong: homographs, rare words, proper names, and a word whose natural stress fights the beat. Never accent every word; a page of accents reads as noise. In other languages do the same locally - respell or transcribe only the individual words that come out wrong, and leave the rest alone."#;

/// Two voices, from a community experiment on the released weights: ~30
/// generations with pinned seeds, one variable at a time. Describing both
/// singers in the caption alone never worked; short tags in the lyrics did.
const DUET_RULE: &str = r#"
Two voices: name both singers in vocal_details ("Singer A (Male), <timbre>. Singer B (Female), <timbre>."), say plainly which one is heard first, and state each assignment in full - "Singer B sings the second verse alone; the male voice is absent there, not even as harmony". When exactly two voices are wanted, say so as an exclusion: no doubling, no stacked harmonies, no backing choir, never more than two human voices at once - otherwise the second voice arrives as a group. Mark the switches in the lyrics with a tag of one or two words alone on its own line - [male vocal], [female vocal], [duet] - and never longer, because a tag of several words gets sung aloud as if it were a line. Switch at section or couplet level, never line by line. Let the male voice open when both are needed, and bring the second voice in early rather than after a long stretch of the first. Describe each voice once, plainly and confidently: repeating a description or hedging it ("small, quiet, never doubled") makes that voice disappear instead."#;

/// The failure mode of an instrumental request: vocals creep back in. Naming
/// what carries the melody instead leaves the model something to sing with.
const INSTRUMENTAL_RULE: &str = r#"
Instrumental: state in vocal_details that the piece is instrumental with no sung words, no wordless or sampled vocals and no choir, and name the instrument carrying the lead melodic line in every section that would otherwise have carried a vocal."#;

/// How a recognised recording becomes a lyric sheet: the words stay the
/// singer's, only the layout is the assistant's.
const TRANSCRIPT_RULES: &str = r#"The transcript comes from speech recognition run on the vocals of a finished recording: one line per sung phrase, each after its start time, with the recogniser's mistakes. Write the lyric sheet of that recording exactly as it is sung. Keep the singer's words, in their order, their language and their alphabet - Cyrillic stays Cyrillic, never transliterate; correct a word only where the recognition is plainly wrong and the right word is certain from the line; never invent, rewrite, translate or complete lines, and drop fragments the recogniser picked up in instrumental passages. Leave the times out. Organise the lines into sections: a block of lines that returns is the [chorus], written out every time it is sung; the blocks between choruses are the [verse]; a block sung once that is neither is the [bridge]; a block that leads into the chorus every time is the [pre-chorus]; lines before the first verse are the [intro] and after the last chorus the [outro]. Every section starts with its tag in square brackets, lowercase, in English, on a line of its own, its lines follow below it, and a blank line separates sections. Use no other tags and no section names in words."#;

/// How the three caption fields become the caption the engine reads.
const CAPTION_LAYOUT: &str = "The caption create_song takes, and a dataset song's style, is the three fields under their headings, each heading alone on its line:\n\nGlobal Metadata\n<global_metadata>\nVocal Details\n<vocal_details>\nArrangement\n<arrangement>";

/// The writing guides an agent connected over MCP reads, by topic.
pub const GUIDE_TOPICS: &[(&str, &str)] = &[
    ("song", "writing a whole song for song_create: caption, lyrics, title, cover prompt, duration"),
    ("caption", "the structured caption MiniMax Music 3 reads, for a new song and for a dataset song"),
    ("lyrics", "lyrics: section tags, sizing to the duration, diction, duets, instrumentals"),
    ("transcript", "turning recognised words into a lyric sheet"),
    ("sections", "marking the sections of a published lyric sheet without changing a word"),
];

/// The rules the studio's own assistant is prompted with, as a guide for an
/// agent connected over MCP: the same text, so an agent writes the way the
/// model expects.
pub fn writing_guide(topic: &str) -> Option<String> {
    Some(match topic {
        "song" => format!("{CAPTION_CONTRACT}\n\n{CAPTION_LAYOUT}\n\n{LYRICS_RULES}{DICTION_RULE}{DUET_RULE}{INSTRUMENTAL_RULE}\n\n{EXTRA}\n\n{VALIDATION}"),
        "caption" => format!("{CAPTION_CONTRACT}{INSTRUMENTAL_RULE}\n\n{CAPTION_LAYOUT}\n\nA dataset song is captioned by MOSS-Music from what it hears, with the measured tempo and key put into Basic Attributes; correct what it got wrong and keep that shape."),
        "lyrics" => format!("{LYRICS_RULES}{DICTION_RULE}{DUET_RULE}"),
        "transcript" => TRANSCRIPT_RULES.to_string(),
        "sections" => SHEET_SECTIONS_PROMPT.to_string(),
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistTarget {
    /// Write both the lyrics and the three caption fields.
    All,
    /// Rewrite only the lyrics, keeping them coherent with the current caption.
    Lyrics,
    /// Rewrite only the caption, keeping it coherent with the current lyrics.
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
    pub global_metadata: String,
    #[serde(default)]
    pub vocal_details: String,
    #[serde(default)]
    pub arrangement: String,
    #[serde(default = "default_duration")]
    pub duration_seconds: f64,
    #[serde(default)]
    pub instrumental: bool,
}

fn default_duration() -> f64 {
    60.0
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AssistDraft {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_metadata: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocal_details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<String>,
    /// A name for the track. The model has the words and the mood in front of
    /// it; asking the user to invent one afterwards is asking twice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What the cover should show, in one sentence, ready for an image model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_prompt: Option<String>,
    /// How long the song it just wrote should be. The model laid out the
    /// sections, so it is the one that knows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u32>,
}

/// The system prompt and the JSON keys the answer must carry.
pub fn instructions(request: &AssistRequest) -> (String, &'static [&'static str]) {
    let references = references_for(request);
    let notes = craft_notes(request);
    match request.target {
        AssistTarget::Lyrics => (
            format!(
                "You write lyrics for MiniMax Music 3, a lyrics+description music generation model.\n\
                 Given a lyrics instruction, the current structured prompt (global metadata, vocal details, arrangement) and a target duration, write lyrics coherent with that structured prompt.\n\
                 {LYRICS_RULES}{notes}\n\
                 Answer with ONLY a JSON object with key: lyrics."
            ),
            &["lyrics"],
        ),
        AssistTarget::Prompt => (
            format!(
                "You write the structured caption for MiniMax Music 3, a lyrics+description music generation model.\n\
                 Given a sound instruction and/or lyrics, produce global_metadata, vocal_details and arrangement. Build the arrangement timeline around the lyric section tags when lyrics are provided. {CAPTION_CONTRACT}{notes}\n\
                 Also write {EXTRA}\n\
                 Answer with ONLY a JSON object with keys: global_metadata, vocal_details, arrangement, title, cover_prompt, duration_seconds."
            ),
            &["global_metadata", "vocal_details", "arrangement"],
        ),
        AssistTarget::Transcript => (
            format!(
                "You prepare training data for MiniMax Music 3, a model that learns songs from their audio, their captions and their lyric sheets.\n\
                 {TRANSCRIPT_RULES}\n\
                 Answer with ONLY a JSON object with key: lyrics."
            ),
            &["lyrics"],
        ),
        // a sheet asks for boundaries only; see `sheet_in_sections`
        AssistTarget::Sheet => (SHEET_SECTIONS_PROMPT.to_string(), &["sections"]),
        AssistTarget::All => (
            format!(
                "You write inputs for MiniMax Music 3, a lyrics+description music generation model.\n\
                 Given a song description and a target duration, produce:\n\
                 1. {LYRICS_RULES}\n\
                 2-4. global_metadata, vocal_details, arrangement — a structured caption. {CAPTION_CONTRACT}{notes}\n\
                 5-6. {EXTRA}\n\
                 Answer with ONLY a JSON object with keys: lyrics, global_metadata, vocal_details, arrangement, title, cover_prompt, duration_seconds.
                 {VALIDATION}{references}"
            ),
            &["lyrics", "global_metadata", "vocal_details", "arrangement"],
        ),
    }
}

/// The rules that only apply to some songs.
///
/// Everything here costs prompt room and, on a small local model, attention.
/// A diction rule matters whenever words are being written; the duet rule
/// matters for the few songs that have two singers, and stating it for a solo
/// vocal would only invite one. So each arrives when the request calls for it.
fn craft_notes(request: &AssistRequest) -> String {
    let mut notes = String::new();
    if matches!(request.target, AssistTarget::All | AssistTarget::Lyrics) && !request.instrumental {
        notes.push_str(DICTION_RULE);
    }
    if request.instrumental {
        notes.push_str(INSTRUMENTAL_RULE);
    } else if wants_two_voices(request) {
        notes.push_str(DUET_RULE);
    }
    notes
}

/// Whether the song has two singers, read from whatever the user wrote.
fn wants_two_voices(request: &AssistRequest) -> bool {
    const CUES: &[&str] = &[
        "duet",
        "дуэт",
        "two voices",
        "два голоса",
        "male and female",
        "female and male",
        "мужской и женский",
        "женский и мужской",
        "singer b",
        "call and response",
        "перекличк",
        "вдвоём",
        "вдвоем",
    ];
    let brief = format!(
        "{} {} {} {}",
        request.description, request.instruction, request.vocal_details, request.lyrics
    )
    .to_lowercase();
    CUES.iter().any(|cue| brief.contains(cue))
}

/// The user message, carrying whichever side of the song already exists so the
/// two halves stay coherent.
/// Complete reference captions from MiniMax's own template library, chosen by
/// the skill's genre router. The skill's whole method is to show the model
/// two or three captions from the right family rather than describe the style
/// in the abstract.
fn references_for(request: &AssistRequest) -> String {
    let brief = format!("{} {} {}", request.description, request.instruction, request.global_metadata);
    let references = crate::skill::references(&brief);
    if references.is_empty() {
        return String::new();
    }
    let mut block = String::from("

Reference captions from MiniMax's own library, in the style family this request routes to. Use them for musical identity, section logic and level of detail. Do not copy their sentences, key, bpm, instruments or story - write a new caption for this request.
");
    for (index, reference) in references.iter().enumerate() {
        block.push_str(&format!("
--- reference {} ---
{}
", index + 1, reference.trim()));
    }
    block
}

pub fn user_message(request: &AssistRequest) -> String {
    let instruction = request.instruction.trim();
    let description = request.description.trim();
    let brief = if !instruction.is_empty() { instruction } else { description };
    let instrumental = if request.instrumental { "\nThis piece is instrumental: no sung words." } else { "" };

    match request.target {
        AssistTarget::Lyrics => format!(
            "Lyrics instruction: {}\nCurrent structured prompt, keep the lyrics coherent with it:\nGlobal metadata: {}\nVocal details: {}\nArrangement: {}\nTarget duration: {} seconds.{instrumental}",
            if brief.is_empty() { "(none — write lyrics that fit the structured prompt)" } else { brief },
            request.global_metadata.trim(),
            request.vocal_details.trim(),
            request.arrangement.trim(),
            request.duration_seconds.round() as i64,
        ),
        AssistTarget::Transcript => format!("Transcript:\n{description}"),
        AssistTarget::Sheet => format!("Lyric sheet:\n{description}"),
        AssistTarget::Prompt => format!(
            "Sound instruction: {}\nCurrent lyrics, keep the structured prompt coherent with them:\n{}{instrumental}",
            if brief.is_empty() { "(none — describe a sound that fits the lyrics)" } else { brief },
            request.lyrics.trim(),
        ),
        AssistTarget::All => {
            // Whatever the user already wrote is material, not noise: it goes to
            // the model so the rest is built around it instead of replacing it.
            // Empty fields are simply not mentioned.
            let mut carried = String::new();
            let mut carry = |label: &str, value: &str| {
                let value = value.trim();
                if !value.is_empty() {
                    carried.push_str(&format!("
{label} (the user wrote this - keep it, build around it):
{value}"));
                }
            };
            carry("Lyrics", &request.lyrics);
            carry("Global metadata", &request.global_metadata);
            carry("Vocal details", &request.vocal_details);
            carry("Arrangement", &request.arrangement);
            format!(
                "Song description: {}{carried}{instrumental}",
                if brief.is_empty() { "(none - choose something musical and specific)" } else { brief },
            )
        }
    }
}

/// Extracts the answer, tolerating a model that wraps its JSON in prose or a
/// code fence — a local 4B model does that more often than a hosted one.
pub fn parse_draft(content: &str, required: &[&str]) -> Result<AssistDraft> {
    let start = content.find('{').context("the assistant returned no JSON object")?;
    let end = content.rfind('}').context("the assistant returned no JSON object")?;
    if end <= start {
        bail!("the assistant returned no JSON object");
    }
    let value: Value = serde_json::from_str(&content[start..=end]).with_context(|| {
        // Naming the failure without showing the answer leaves nothing to act
        // on: the interesting part is what the model actually wrote.
        let sample: String = content.chars().take(220).collect();
        format!("the assistant returned invalid JSON. It answered: {sample}")
    })?;
    // A model answers with what it finds natural: a string for the caption, and
    // very often an array of lines for the lyrics. Both are the same lyric.
    let field = |key: &str| -> Option<String> {
        let text = match value.get(key)? {
            Value::String(text) => text.trim().to_owned(),
            Value::Array(items) => items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join("
")
                .trim()
                .to_owned(),
            _ => return None,
        };
        (!text.is_empty()).then_some(text)
    };

    for key in required {
        if field(key).is_none() {
            bail!("the assistant answer is missing '{key}'");
        }
    }
    Ok(AssistDraft {
        lyrics: field("lyrics"),
        global_metadata: field("global_metadata"),
        vocal_details: field("vocal_details"),
        arrangement: field("arrangement"),
        title: field("title"),
        cover_prompt: field("cover_prompt"),
        // Accepted as a number or as the string a model sometimes sends, and
        // kept inside what the engine can render.
        duration_seconds: value
            .get("duration_seconds")
            .and_then(|value| value.as_u64().or_else(|| value.as_str().and_then(|text| text.trim().parse().ok())))
            .map(|seconds| seconds.clamp(10, 360) as u32),
    })
}

/// The shape the answer must have, as a schema the server can enforce.
///
/// llama-server turns this into grammar rules and applies them while sampling,
/// so a local model cannot answer with prose, with a fenced block, or with a
/// list where a string belongs - the three ways it used to come back unusable.
pub fn draft_schema(required: &[&str]) -> Value {
    // A minimum length, not just a type: "required" only forces the key to be
    // present, and a model that answers with an empty string satisfies that
    // while leaving the field blank on screen.
    let text = serde_json::json!({ "type": "string", "minLength": 40 });
    let lyric = serde_json::json!({ "type": "string", "minLength": 20 });
    let short = serde_json::json!({ "type": "string", "minLength": 3 });
    serde_json::json!({
        "type": "object",
        "properties": {
            "lyrics": lyric,
            "global_metadata": text,
            "vocal_details": text,
            "arrangement": text,
            "title": short,
            "cover_prompt": short,
            "duration_seconds": { "type": "number" },
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

/// MiniMax's tags: lowercase, verses unnumbered.
fn section_tag(kind: &str, _verse: usize) -> String {
    kind.to_string()
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

    #[test]
    fn section_tags_go_and_the_words_stay() {
        let sheet = "[Verse 1]\nСреди связок\n[x] в горле\n\n[Chorus]\nНо настала пора";
        assert_eq!(without_section_tags(sheet), "Среди связок\n[x] в горле\n\nНо настала пора");
    }
    /// The skill is 6 MB on disk and none of it may travel: only the routed
    /// reference captions do, and there are at most three.
    #[test]
    fn the_prompt_stays_small_enough_to_send() {
        let request = super::AssistRequest {
            target: super::AssistTarget::All,
            description: String::new(),
            instruction: "symphonic metal with orchestral choirs, female vocal".into(),
            lyrics: String::new(),
            global_metadata: String::new(),
            vocal_details: String::new(),
            arrangement: String::new(),
            duration_seconds: 60.0,
            instrumental: false,
        };
        let (system, _) = super::instructions(&request);
        println!("system prompt: {} characters, {} reference blocks", system.len(), system.matches("--- reference").count());
        assert!(system.len() < 24_000, "the prompt grew to {} characters", system.len());
        assert!(system.matches("--- reference").count() <= 3);
    }

    /// The skill's own reference captions were selected and then never used:
    /// the routing existed, the prompt did not carry it.
    #[test]
    fn the_caption_prompt_carries_the_skill_references() {
        let request = super::AssistRequest {
            target: super::AssistTarget::All,
            description: String::new(),
            instruction: "a dark synthwave night drive, female vocal".into(),
            lyrics: String::new(),
            global_metadata: String::new(),
            vocal_details: String::new(),
            arrangement: String::new(),
            duration_seconds: 60.0,
            instrumental: false,
        };
        let (system, _) = super::instructions(&request);
        assert!(system.contains("Reference captions from MiniMax"), "the skill's references are missing from the prompt");
        assert!(system.contains("Global Metadata"), "a reference caption is not in the prompt");
    }

    /// The form's default was quoted into the prompt, and every answer came
    /// back as sixty seconds. Nothing may put a length in front of the model
    /// when it is writing the whole song.
    #[test]
    fn the_whole_song_request_carries_no_target_length() {
        let request = super::AssistRequest {
            target: super::AssistTarget::All,
            description: String::new(),
            instruction: "club progressive house".into(),
            lyrics: String::new(),
            global_metadata: String::new(),
            vocal_details: String::new(),
            arrangement: String::new(),
            duration_seconds: 60.0,
            instrumental: false,
        };
        let message = super::user_message(&request);
        assert!(!message.contains("60"), "the prompt still carries the default: {message}");
        assert!(!message.to_lowercase().contains("duration"), "the prompt still names a duration: {message}");
    }

    use super::*;

    #[test]
    fn a_sheet_is_cut_where_the_sections_start() {
        let lines = ["Шёл я как-то по лесу,", "Шёл по грибы", "И тут раз", "Конец"];
        let answer = r#"{"sections": [{"kind": "verse", "from": 1, "to": 2}, {"kind": "Chorus", "from": 3, "to": 3}, {"kind": "coda", "from": 4, "to": 4}]}"#;
        assert_eq!(sheet_in_sections(answer, &lines).as_deref(), Some("[verse]\nШёл я как-то по лесу,\nШёл по грибы\n\n[chorus]\nИ тут раз\n\n[verse]\nКонец"));
        assert_eq!(numbered_lines(&["Ой, да!", "Куплет", "ой да"]), "1. Ой, да! [x2]\n2. Куплет\n3. ой да [x2]");
    }

    /// The published rules must reach the model itself, whichever provider is
    /// answering: the same system message is what `chat_body` sends to a local
    /// sidecar, to a server the user runs, or to OpenRouter.
    #[test]
    fn minimax_own_caption_rules_are_in_the_request_that_goes_out() {
        for target in [AssistTarget::All, AssistTarget::Prompt] {
            let mut sample = request(target);
            sample.target = target;
            let (system, _) = instructions(&sample);
            assert!(system.contains("music-caption-rewriter"), "the skill is not cited for {target:?}");
            assert!(system.contains("Global Emotional Progression"), "caption shape missing for {target:?}");
            assert!(system.contains("250-450"), "length rule missing for {target:?}");

            let body = chat_body("any-model", &system, "idea");
            let sent = body["messages"][0]["content"].as_str().unwrap_or_default();
            assert!(sent.contains("Instrument Lifecycle Description"), "the contract never reached the body");
        }
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
            lyrics: "[verse]\nneon".into(),
            global_metadata: "Basic Attributes: bpm is 110.".into(),
            vocal_details: "Vocal Gender & Timbre: Singer A (Female)".into(),
            arrangement: "Primary: synths".into(),
            duration_seconds: 90.0,
            instrumental: false,
        }
    }

    /// Each target must ask for exactly the fields it will write back, otherwise
    /// a partial answer would silently blank a pane the user had filled in.
    #[test]
    fn every_target_declares_the_fields_it_writes() {
        assert_eq!(instructions(&request(AssistTarget::Lyrics)).1, &["lyrics"]);
        assert_eq!(instructions(&request(AssistTarget::Prompt)).1, &["global_metadata", "vocal_details", "arrangement"]);
        assert_eq!(instructions(&request(AssistTarget::All)).1.len(), 4);
    }

    #[test]
    fn the_other_half_of_the_song_travels_as_context() {
        let lyrics_message = user_message(&request(AssistTarget::Lyrics));
        assert!(lyrics_message.contains("Basic Attributes: bpm is 110."));
        assert!(lyrics_message.contains("90 seconds"));

        let prompt_message = user_message(&request(AssistTarget::Prompt));
        assert!(prompt_message.contains("[verse]"));
    }

    #[test]
    fn an_instrumental_request_says_so_to_the_model() {
        let mut instrumental = request(AssistTarget::All);
        instrumental.instrumental = true;
        assert!(user_message(&instrumental).contains("instrumental"));
    }

    #[test]
    fn json_survives_a_code_fence_and_surrounding_prose() {
        let draft = parse_draft(
            "Sure!\n```json\n{\"lyrics\": \"[verse]\\nline\"}\n```\nHope that helps.",
            &["lyrics"],
        )
        .unwrap();
        assert_eq!(draft.lyrics.unwrap(), "[verse]\nline");
    }

    #[test]
    fn a_missing_required_field_is_an_error_rather_than_a_blank_pane() {
        assert!(parse_draft("{\"lyrics\": \"x\"}", &["global_metadata"]).is_err());
        assert!(parse_draft("no json here", &["lyrics"]).is_err());
    }

    /// Gemma writes lyrics as a list of lines about as often as it writes them
    /// as one string, and both are the same song.
    #[test]
    fn lyrics_may_arrive_as_a_list_of_lines() {
        let answer = r#"{"lyrics": ["[intro]", "[verse]", "neon on the wet road"], "global_metadata": "g", "vocal_details": "v", "arrangement": "a"}"#;
        let draft = super::parse_draft(answer, &["lyrics"]).expect("a list of lines is a lyric");
        assert_eq!(draft.lyrics.as_deref(), Some("[intro]
[verse]
neon on the wet road"));
    }

    /// A Russian idea used to come back as an English song: the caption rule
    /// ("write in English") had quietly swallowed the lyrics as well.
    #[test]
    fn the_lyrics_follow_the_language_of_the_request() {
        let request = super::AssistRequest {
            target: super::AssistTarget::All,
            description: String::new(),
            instruction: "панк-рок про ёжика в бункере".into(),
            lyrics: String::new(),
            global_metadata: String::new(),
            vocal_details: String::new(),
            arrangement: String::new(),
            duration_seconds: 60.0,
            instrumental: false,
        };
        let (system, _) = super::instructions(&request);
        assert!(system.contains("language the user wrote their request in"));
    }

    /// A stress mark is the only lever there is on pronunciation: the caption
    /// never reaches the singing, the letters do.
    #[test]
    fn a_sung_request_carries_the_diction_rule() {
        let request = super::AssistRequest {
            target: super::AssistTarget::All,
            description: "песня про замок на горе".into(),
            instruction: String::new(),
            lyrics: String::new(),
            global_metadata: String::new(),
            vocal_details: String::new(),
            arrangement: String::new(),
            duration_seconds: 90.0,
            instrumental: false,
        };
        let (system, _) = super::instructions(&request);
        assert!(system.contains("combining acute"), "nothing tells the model how to fix a stress");
        assert!(system.contains("ё"), "the ё rule is missing");
    }

    /// Two singers need rules a solo song must not see: told to a one-voice
    /// song, they would invite a second voice that nobody asked for.
    #[test]
    fn the_duet_rules_arrive_only_for_two_voices() {
        let mut solo = request(AssistTarget::All);
        solo.description = "a night drive synth pop song".into();
        solo.vocal_details = String::new();
        solo.lyrics = String::new();
        let (system, _) = instructions(&solo);
        assert!(!system.contains("[male vocal]"), "a solo song was given duet rules");

        let mut duet = solo.clone();
        duet.description = "дуэт мужского и женского голоса, поп-баллада".into();
        let (system, _) = instructions(&duet);
        assert!(system.contains("[male vocal]"), "the duet has no voice tags to use");
        assert!(system.contains("no backing choir"), "the anti-choir clause is missing");
    }

    /// An instrumental gets the opposite instruction, and no diction rule:
    /// there is nothing to pronounce.
    #[test]
    fn an_instrumental_is_told_what_carries_the_melody() {
        let mut instrumental = request(AssistTarget::All);
        instrumental.instrumental = true;
        let (system, _) = instructions(&instrumental);
        assert!(system.contains("lead melodic line"), "nothing replaces the missing vocal");
        assert!(!system.contains("combining acute"), "an instrumental was given a diction rule");
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
        let schema = super::draft_schema(&["global_metadata"]);
        assert_eq!(schema["properties"]["global_metadata"]["minLength"], 40);
        assert_eq!(schema["required"][0], "global_metadata");
    }
}
