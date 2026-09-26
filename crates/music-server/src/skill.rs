//! Reference requests for the writing assistant.
//!
//! The official ACE-Step 1.5 text-to-music examples - captions and lyrics the
//! model's authors published - are carried inside the binary, the same file
//! the request form offers as examples. For a brief, the closest ones by
//! shared words are put in front of the text model, so it writes in the shape
//! ACE-Step was shown rather than guessing from an abstract description.

use serde::Serialize;
use serde_json::Value;

const EXAMPLES: &str = include_str!("../../../app/examples/text2music.json");

/// Two references keep the prompt within what a small local model reads
/// carefully; each is a caption and the opening of its lyrics.
const MAX_REFERENCES: usize = 2;
const LYRICS_EXCERPT_CHARS: usize = 600;

#[derive(Debug, Clone, Serialize)]
pub struct Reference {
    pub caption: String,
    pub lyrics: String,
}

fn examples() -> Vec<Reference> {
    let all: Vec<Value> = serde_json::from_str(EXAMPLES).expect("app/examples/text2music.json is a list of requests");
    all.into_iter()
        .filter_map(|value| {
            let caption = value.get("caption")?.as_str()?.trim().to_owned();
            let lyrics = value.get("lyrics").and_then(Value::as_str).unwrap_or_default().trim().to_owned();
            (!caption.is_empty()).then_some(Reference { caption, lyrics })
        })
        .collect()
}

/// Words every caption has, which say nothing about the genre.
const FILLER: &[&str] = &[
    "with", "and", "the", "for", "from", "into", "that", "this", "song", "music", "style", "like", "about", "track", "throughout",
    "its", "are", "has", "over", "features", "overall", "sound",
];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '&' && c != '-')
        .filter(|word| word.chars().count() > 2 && !FILLER.contains(word))
        .map(str::to_owned)
        .collect()
}

fn score(reference: &Reference, brief_words: &[String]) -> usize {
    let haystack = words(&reference.caption);
    brief_words.iter().filter(|word| haystack.contains(word)).map(|word| word.chars().count()).sum()
}

/// The closest official requests to a brief, most similar first. An empty
/// result is normal: the contract alone still describes the shape.
pub fn references(brief: &str) -> Vec<Reference> {
    let brief_words = words(brief);
    if brief_words.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, usize, Reference)> = examples()
        .into_iter()
        .enumerate()
        .map(|(index, reference)| (score(&reference, &brief_words), index, reference))
        .filter(|(score, _, _)| *score > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(MAX_REFERENCES)
        .map(|(_, _, mut reference)| {
            if reference.lyrics.chars().count() > LYRICS_EXCERPT_CHARS {
                reference.lyrics = reference.lyrics.chars().take(LYRICS_EXCERPT_CHARS).collect::<String>() + "\n...";
            }
            reference
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_official_examples_are_carried_whole() {
        let all = examples();
        assert!(all.len() >= 100, "only {} examples are embedded", all.len());
    }

    #[test]
    fn a_brief_finds_requests_of_its_own_genre() {
        let found = references("heavy metal with distorted guitars");
        assert!(!found.is_empty());
        assert!(found.len() <= MAX_REFERENCES);
        assert!(found[0].caption.to_lowercase().contains("metal") || found[0].caption.to_lowercase().contains("distorted"), "{}", found[0].caption);
    }

    #[test]
    fn filler_words_do_not_match() {
        let reference = Reference { caption: "pop with piano and the strings".into(), lyrics: String::new() };
        assert_eq!(score(&reference, &words("rock with the band and drums")), 0);
    }

    #[test]
    fn a_brief_with_no_shared_words_finds_nothing_rather_than_noise() {
        assert!(references("zzqx").is_empty());
        assert!(references("").is_empty());
    }
}
