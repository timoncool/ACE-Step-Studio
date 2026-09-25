//! Cover prompts written once and reused.
//!
//! A cover prompt is two things at the same time: a look the user has settled
//! on, and the particulars of one track. Writing both out every time means
//! retyping the look, so the look is kept as a template with `{placeholders}`
//! and the particulars are filled in from the song the cover is for.
//!
//! Placeholders are deliberately plain words rather than a syntax: `{title}`,
//! `{style}`, `{lyrics}`, `{excerpt}`, `{tags}`, `{duration}`. An unknown
//! placeholder is left as written, so a stray brace in a prompt reaches the
//! image model unchanged instead of disappearing.

use serde::{Deserialize, Serialize};

/// One saved look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoverTemplate {
    pub id: String,
    pub name: String,
    pub template: String,
}

/// The facts of one track, as the template sees them.
#[derive(Debug, Clone, Default)]
pub struct TrackFacts {
    pub title: String,
    pub style: String,
    pub lyrics: String,
    pub duration_seconds: f64,
}

/// How long an `{excerpt}` may be before the model stops reading it as an image
/// brief and starts trying to set it to music.
const EXCERPT_CHARS: usize = 180;

/// The looks the studio ships with. Each one is a complete prompt: a user who
/// never opens the editor still gets a cover that matches the track.
pub fn default_templates() -> Vec<CoverTemplate> {
    vec![
        CoverTemplate {
            id: "photographic".into(),
            name: "Photographic".into(),
            template: "Album cover photograph for \"{title}\". {style}. Cinematic lighting, shallow depth of field, 35mm, square composition, no text, no lettering, no watermark.".into(),
        },
        CoverTemplate {
            id: "painterly".into(),
            name: "Painterly".into(),
            template: "Album cover illustration for \"{title}\", painted in oil on canvas with visible brushwork. Mood taken from the music: {style}. Square composition, no text, no lettering.".into(),
        },
        CoverTemplate {
            id: "graphic".into(),
            name: "Graphic".into(),
            template: "Minimal graphic album cover for \"{title}\". Bold geometric shapes, two-colour risograph print, heavy grain. Genre: {style}. Square, no text, no lettering.".into(),
        },
        CoverTemplate {
            id: "from-the-words".into(),
            name: "From the words".into(),
            template: "Album cover for \"{title}\". Illustrate the scene these words describe: {excerpt}. Style: {style}. Square composition, no text, no lettering.".into(),
        },
    ]
}

/// Fills a template in from one track.
pub fn render(template: &str, facts: &TrackFacts) -> String {
    let mut out = String::with_capacity(template.len() + 64);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match value_of(name, facts) {
                    // A placeholder with nothing behind it leaves no gap.
                    Some(value) => out.push_str(value.trim()),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                // An unclosed brace is text, not a placeholder.
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    tidy(&out)
}

fn value_of(name: &str, facts: &TrackFacts) -> Option<String> {
    match name.trim().to_ascii_lowercase().as_str() {
        "title" => Some(if facts.title.trim().is_empty() { "Untitled".into() } else { facts.title.trim().into() }),
        // The caption is what the studio calls a style everywhere else; both
        // names work, because both are what a user would type.
        "style" | "caption" | "tags" | "genre" => Some(facts.style.trim().into()),
        "lyrics" => Some(sung_text(&facts.lyrics)),
        "excerpt" => Some(excerpt(&facts.lyrics)),
        "duration" => Some(duration(facts.duration_seconds)),
        _ => None,
    }
}

/// The lyrics without the `[chorus]` scaffolding, which means nothing to an
/// image model.
fn sung_text(lyrics: &str) -> String {
    lyrics
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !(line.starts_with('[') && line.ends_with(']')))
        .collect::<Vec<_>>()
        .join(" ")
}

fn excerpt(lyrics: &str) -> String {
    let text = sung_text(lyrics);
    if text.chars().count() <= EXCERPT_CHARS {
        return text;
    }
    let cut: String = text.chars().take(EXCERPT_CHARS).collect();
    // Stop at a word rather than mid-syllable.
    match cut.rfind(' ') {
        Some(space) => format!("{}…", &cut[..space]),
        None => format!("{cut}…"),
    }
}

fn duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Removes the double spaces and orphaned punctuation an empty placeholder
/// leaves behind, so a track with no style still reads as a sentence.
fn tidy(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut previous_space = false;
    for character in value.chars() {
        if character.is_whitespace() {
            if !previous_space {
                out.push(' ');
            }
            previous_space = true;
            continue;
        }
        if character == '.' || character == ',' {
            // ". ." and ", ," come from a placeholder that resolved to nothing.
            if out.ends_with(". ") || out.ends_with(", ") {
                out.truncate(out.len() - 2);
                out.push(character);
                previous_space = false;
                continue;
            }
        }
        out.push(character);
        previous_space = false;
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> TrackFacts {
        TrackFacts {
            title: "Неон".into(),
            style: "dream pop, female vocal".into(),
            lyrics: "[verse]\nНеон дрожит над мокрым городом\nи я иду домой".into(),
            duration_seconds: 154.0,
        }
    }

    #[test]
    fn fills_in_what_the_track_knows() {
        let rendered = render("Cover for \"{title}\". Style: {style}. Length {duration}.", &facts());
        assert_eq!(rendered, "Cover for \"Неон\". Style: dream pop, female vocal. Length 2:34.");
    }

    #[test]
    fn an_excerpt_drops_the_section_markers() {
        let rendered = render("{excerpt}", &facts());
        assert_eq!(rendered, "Неон дрожит над мокрым городом и я иду домой");
    }

    #[test]
    fn a_long_excerpt_is_cut_at_a_word() {
        let mut long = facts();
        long.lyrics = "слово ".repeat(200);
        let rendered = render("{excerpt}", &long);
        assert!(rendered.ends_with('…'));
        assert!(rendered.chars().count() <= EXCERPT_CHARS + 1);
    }

    #[test]
    fn an_unknown_placeholder_is_left_alone() {
        assert_eq!(render("{title} in {unknown}", &facts()), "Неон in {unknown}");
    }

    #[test]
    fn an_empty_value_does_not_leave_a_gap() {
        let mut bare = facts();
        bare.style = String::new();
        assert_eq!(render("Cover for {title}. {style}. Square.", &bare), "Cover for Неон. Square.");
    }

    #[test]
    fn every_shipped_template_renders() {
        for template in default_templates() {
            let rendered = render(&template.template, &facts());
            assert!(!rendered.contains('{'), "{} left a placeholder", template.id);
            assert!(rendered.contains("Неон"));
        }
    }
}
