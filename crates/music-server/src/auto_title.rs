//! Naming a track the user did not name.
//!
//! This is ACE-Step Studio's rule, taken from its own `autoTitle`, in the order
//! it used: the first sung line of the **chorus** (`[chorus]`, `[припев]`,
//! `[hook]`), then the first sung line of any section, then the description, and
//! only if there is nothing at all, "Untitled". A chorus line is what a listener
//! would call the song, which is why it comes first.
//!
//! The trimming is the same too: a line wrapped in parentheses is unwrapped, the
//! title ends at the first sentence boundary if there is one, keeps at most two
//! comma-separated phrases, and is cut to fifty characters on a word boundary.
//!
//! One thing the old studio never had to handle: Music3 reads the description as
//! a labelled document, so the description branch skips leading section labels -
//! otherwise every instrumental would be called "Global Metadata".

use std::collections::HashSet;

/// As many characters as ACE-Step Studio kept.
const MAX_CHARS: usize = 50;

/// A title for a request that carries none.
pub fn auto_title(caption: &str, lyrics: &str, instrumental: bool) -> String {
    if !instrumental {
        if let Some(line) = chorus_line(lyrics).or_else(|| first_sung_line(lyrics)) {
            return trim_title(line);
        }
    }
    // The genre is the part of a description worth putting on a card: a caption
    // opens with measurements, and "bpm is 180" is a fact about the track, not
    // a name for it.
    if let Some(genre) = crate::tagging::genre_from_caption(caption) {
        return trim_title(&genre);
    }
    let described = strip_labels(caption);
    if !described.is_empty() {
        return trim_title(&described);
    }
    "Untitled".to_string()
}

/// The first sung line after a chorus marker.
fn chorus_line(lyrics: &str) -> Option<&str> {
    let mut in_chorus = false;
    for line in lyrics.lines() {
        let line = line.trim();
        if is_chorus_marker(line) {
            in_chorus = true;
            continue;
        }
        if in_chorus {
            if line.is_empty() {
                continue;
            }
            if !is_marker(line) {
                return Some(line);
            }
            // Another section started before anything was sung.
            in_chorus = false;
        }
    }
    None
}

fn is_marker(line: &str) -> bool {
    line.starts_with('[') && line.ends_with(']')
}

fn is_chorus_marker(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('[') else { return false };
    let lowered = rest.to_lowercase();
    lowered.starts_with("chorus") || lowered.starts_with("припев") || lowered.starts_with("hook")
}

/// Whether anything is sung at all: an instrumental carries its section markers
/// and nothing else.
pub fn has_sung_lines(lyrics: &str) -> bool {
    first_sung_line(lyrics).is_some()
}

/// The first line of the lyrics that is words rather than a `[chorus]` marker.
fn first_sung_line(lyrics: &str) -> Option<&str> {
    lyrics
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !is_marker(line))
}

/// The section names Music3's caption format uses. They are the document's
/// scaffolding, not a description of the song.
pub const LABELS: &[&str] = &[
    "global metadata",
    "basic attributes",
    "global emotional progression",
    "application scenarios & imagery",
    "sonics & production profile",
    "vocal details",
    "vocal gender & timbre",
    "vocal style",
    "harmony/backing vocals",
    "vocal fx",
    "arrangement",
    "instrument lifecycle description",
    "groove & foundation progression",
    "embellishments, textures & spatial fx",
];

/// The caption with its leading labels removed.
fn strip_labels(caption: &str) -> String {
    let known: HashSet<&str> = LABELS.iter().copied().collect();
    let mut rest = caption.trim();
    loop {
        // A heading stands alone on its line and carries no colon, so the loop
        // below never recognised it and "Global Metadata" led the title.
        if let Some(line_end) = rest.find('\n') {
            if known.contains(rest[..line_end].trim().to_lowercase().as_str()) {
                rest = rest[line_end + 1..].trim_start();
                continue;
            }
        }
        let Some(colon) = rest.find(':') else { break };
        let (head, tail) = rest.split_at(colon);
        let candidate = head.trim().trim_start_matches('[').trim_end_matches(']').to_lowercase();
        let candidate = candidate.trim_end_matches(" (primary/secondary layering)").trim().to_string();
        if !known.contains(candidate.as_str()) {
            break;
        }
        rest = tail[1..].trim_start();
    }
    rest.trim().to_string()
}

/// ACE-Step Studio's `trimTitle`, character-safe: at most two phrases, cut at a
/// sentence end, fifty characters, on a word boundary.
fn trim_title(raw: &str) -> String {
    let mut value = raw.trim().to_string();

    // "(Вою, спасу)" is a stage direction around the line, not part of it.
    if value.starts_with('(') && value.ends_with(')') && value.chars().count() > 2 {
        value = value[1..value.len() - 1].trim().to_string();
    }

    // End at the first sentence boundary, if one comes early enough to be a title.
    if let Some(end) = sentence_end(&value) {
        if end < MAX_CHARS {
            value = value.chars().take(end + 1).collect::<String>().trim().to_string();
        }
    }

    // Keep at most two comma-separated phrases.
    let phrases: Vec<&str> = value.split(',').collect();
    if phrases.len() > 2 {
        value = phrases[..2].join(",").trim().to_string();
    }

    if value.chars().count() <= MAX_CHARS {
        return value;
    }

    let cut: String = value.chars().take(MAX_CHARS).collect();
    let trimmed = match cut.rfind(' ') {
        Some(space) => cut[..space].trim_end().to_string(),
        None => cut,
    };
    format!("{trimmed}…")
}

/// The character index of the first `.`, `!` or `?` that is followed by a space.
fn sentence_end(value: &str) -> Option<usize> {
    let characters: Vec<char> = value.chars().collect();
    characters.windows(2).position(|pair| matches!(pair[0], '.' | '!' | '?') && pair[1].is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chorus_names_the_song() {
        let lyrics = "[verse]\nдождь по крыше\n\n[chorus]\nНеон дрожит над мокрым городом\n[bridge]\nещё строка";
        assert_eq!(auto_title("synth-pop", lyrics, false), "Неон дрожит над мокрым городом");
    }

    #[test]
    fn a_russian_chorus_marker_counts_too() {
        let lyrics = "[Куплет]\nпервая строка\n[Припев]\nГори, гори ясно";
        assert_eq!(auto_title("", lyrics, false), "Гори, гори ясно");
    }

    #[test]
    fn without_a_chorus_it_takes_the_first_sung_line() {
        let lyrics = "[verse]\nдождь по крыше\nвторая строка";
        assert_eq!(auto_title("synth-pop", lyrics, false), "дождь по крыше");
    }

    #[test]
    fn without_lyrics_it_takes_the_description() {
        assert_eq!(auto_title("Bright uplifting synth-pop", "", true), "Bright uplifting synth-pop");
    }

    #[test]
    fn a_structured_caption_is_not_named_after_its_labels() {
        let caption = "Global Metadata: Basic Attributes: bpm is 96. key is A, and scale is minor.";
        assert_eq!(auto_title(caption, "", true), "bpm is 96.");
    }

    #[test]
    fn two_phrases_at_most() {
        assert_eq!(
            auto_title("", "warm pads, punchy drums, female vocal, wide stereo", false),
            "warm pads, punchy drums",
        );
    }

    #[test]
    fn a_wrapped_line_is_unwrapped() {
        assert_eq!(auto_title("", "[chorus]\n(Вою, спасу)", false), "Вою, спасу");
    }

    #[test]
    fn a_long_line_is_cut_at_a_word_and_on_a_character_boundary() {
        let lyrics = "Я иду по улице под дождём и думаю о том что было вчера";
        let title = auto_title("", lyrics, false);
        assert!(title.ends_with('…'));
        assert!(title.chars().count() <= MAX_CHARS + 1);
        assert!(lyrics.starts_with(title.trim_end_matches('…')));
    }

    #[test]
    fn an_instrumental_ignores_the_lyrics() {
        assert_eq!(auto_title("lo-fi hip hop", "[chorus]\nnever sung", true), "lo-fi hip hop");
    }

    #[test]
    fn nothing_to_go_on() {
        assert_eq!(auto_title("", "[intro]\n[outro]", false), "Untitled");
    }

    /// The library showed a track named after the caption's own opening line,
    /// heading and measurements and all.
    #[test]
    fn an_instrumental_is_named_after_its_music_not_its_heading() {
        let caption = concat!(
            "Global Metadata\n",
            "Basic Attributes: bpm is 140-160. Melodic Death Metal / Gothenburg Sound. ",
            "Global Emotional Progression: bleak."
        );
        let title = super::auto_title(caption, "", true);
        assert_eq!(title, "Melodic Death Metal / Gothenburg Sound");
        assert!(!title.contains("Global Metadata"));
        assert!(!title.contains("bpm"));
    }

    /// An instrumental carries its section markers and nothing else, and the
    /// studio used to send it to the recogniser anyway.
    #[test]
    fn an_instrumental_has_nothing_sung_in_it() {
        assert!(!has_sung_lines(concat!("[intro]\n\n", "[instrumental]\n\n", "[chorus]\n\n", "[outro]")));
        assert!(!has_sung_lines(""));
        assert!(has_sung_lines(concat!("[chorus]\n", "дождь по крыше")));
    }
}
