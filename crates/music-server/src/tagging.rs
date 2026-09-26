//! ID3 tags on the MP3s the studio writes.
//!
//! The engine returns bare audio: no title, no artist, no cover, no lyrics. A
//! file like that lands in a player as "instrumental-04" with a blank square,
//! which is the wrong answer for a track that has all of those things stored
//! next to it. ACE-Step Studio tagged its exports, and so does this one.
//!
//! Only MP3 is tagged - WAV has no equivalent container the studio writes, and
//! a malformed chunk is worse than none. Tagging never fails a request: if a
//! tag cannot be written the audio is still the audio, and the caller says so
//! in the log rather than losing the track.

use std::path::Path;

use id3::{frame, Tag, TagLike, Version};

/// What a player should show for one track.
#[derive(Debug, Clone, Default)]
pub struct TrackTags {
    pub title: String,
    /// The studio's own name, so a library of these files is recognisable.
    pub album: String,
    pub artist: String,
    /// The caption, trimmed to something a genre field can hold.
    pub genre: Option<String>,
    pub lyrics: Option<String>,
    pub bpm: Option<u32>,
    /// Cover image bytes with their media type, when the track has one.
    pub cover: Option<(String, Vec<u8>)>,
}

/// Writes the tags onto an MP3 in place, replacing whatever was there.
pub fn write_mp3_tags(path: &Path, tags: &TrackTags) -> anyhow::Result<()> {
    let mut tag = Tag::new();
    tag.set_title(tags.title.clone());
    if !tags.album.is_empty() {
        tag.set_album(tags.album.clone());
    }
    if !tags.artist.is_empty() {
        tag.set_artist(tags.artist.clone());
    }
    if let Some(genre) = tags.genre.as_deref().filter(|value| !value.trim().is_empty()) {
        tag.set_genre(genre);
    }
    if let Some(bpm) = tags.bpm {
        tag.set_text("TBPM", bpm.to_string());
    }
    if let Some(lyrics) = tags.lyrics.as_deref().filter(|value| !value.trim().is_empty()) {
        tag.add_frame(frame::Lyrics {
            lang: "eng".to_string(),
            description: String::new(),
            text: lyrics.to_string(),
        });
    }
    if let Some((media_type, image)) = tags.cover.as_ref() {
        tag.add_frame(frame::Picture {
            mime_type: media_type.clone(),
            picture_type: frame::PictureType::CoverFront,
            description: "Cover".to_string(),
            data: image.clone(),
        });
    }
    // 2.4 is what players have read for twenty years, and what the id3 crate
    // writes without the v2.3 text-encoding caveats.
    tag.write_to_path(path, Version::Id3v24)?;
    Ok(())
}

/// The genre field takes a phrase, and only a phrase.
///
/// A one-line prompt starts with one - "Darkwave, Synth-pop. …" - so its first
/// piece is the genre. A structured caption names its genre a few
/// phrases in, behind the heading and a row of measurements, so the phrases are
/// walked until one can stand as a genre. Nothing plausible means the field is
/// left empty, which every player handles and a wrong genre does not.
pub fn genre_from_caption(caption: &str) -> Option<String> {
    caption
        .split(['.', '\n'])
        .filter_map(|piece| {
            // a style that opens with its vocal language ("English, warm piano
            // pop") names its genre right after it
            let mut parts = piece.split(',').map(str::trim);
            let first = parts.next()?;
            if LANGUAGES.contains(&first.to_lowercase().as_str()) { parts.next() } else { Some(first) }
        })
        .find(|phrase| plausible_genre(phrase))
        .map(str::to_string)
}

/// Vocal languages a style may open with; a language alone is not a genre.
const LANGUAGES: &[&str] = &[
    "english", "mandarin", "chinese", "cantonese", "russian", "japanese", "korean", "spanish",
    "french", "german", "portuguese", "italian", "turkish", "arabic", "hindi", "ukrainian",
];

/// Whether a phrase can stand as a genre.
fn plausible_genre(phrase: &str) -> bool {
    let lowered = phrase.to_lowercase();
    // A section name is not a genre, with or without its colon: a caption that
    // opens "Global Metadata Basic Attributes…" would otherwise file every
    // track under "Global Metadata".
    let is_label = crate::auto_title::LABELS.iter().any(|label| lowered.starts_with(label));
    // "key is D", "scale is minor", "bpm is 180" - a stated measurement, not a name.
    let is_measurement = lowered.contains(" is ");
    !phrase.is_empty()
        && phrase.chars().count() < 40
        && !phrase.contains(':')
        && !is_label
        && !is_measurement
        && !phrase.chars().any(|character| character.is_ascii_digit())
}

/// The tempo, written either as `bpm is 96` the way structured captions state it,
/// or as `124 BPM` the way a person writing a one-line prompt does.
pub fn bpm_from_caption(caption: &str) -> Option<u32> {
    let lowered = caption.to_lowercase();
    let at = lowered.find("bpm")?;

    // "124 BPM": the number sits in front of the word.
    let before: String = lowered[..at]
        .chars()
        .rev()
        .skip_while(|character| character.is_whitespace())
        .take_while(char::is_ascii_digit)
        .collect();
    if let Some(bpm) = before.chars().rev().collect::<String>().parse::<u32>().ok().filter(sensible) {
        return Some(bpm);
    }

    // "bpm is 96": the number follows it, past whatever words are between.
    let after: String = lowered[at + 3..]
        .chars()
        .take_while(|character| !character.is_ascii_digit() || true)
        .skip_while(|character| !character.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    after.parse().ok().filter(sensible)
}

/// A tempo a piece of music could actually have.
fn sensible(bpm: &u32) -> bool {
    (30..=300).contains(bpm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_mp3() -> Vec<u8> {
        // One silent MPEG-1 Layer III frame: enough for a tag writer to work on.
        let mut frame = vec![0xFF, 0xFB, 0x90, 0x64];
        frame.resize(418, 0);
        frame
    }

    #[test]
    fn a_written_tag_reads_back() {
        let path = std::env::temp_dir().join(format!("studio-tag-{}.mp3", uuid::Uuid::now_v7()));
        std::fs::write(&path, sample_mp3()).unwrap();

        let tags = TrackTags {
            title: "Неон".into(),
            album: "Studio".into(),
            artist: "Local Studio".into(),
            genre: Some("Synth-pop".into()),
            lyrics: Some("Неон дрожит над мокрым городом".into()),
            bpm: Some(96),
            cover: Some(("image/png".into(), vec![0x89, b'P', b'N', b'G', 1, 2, 3])),
        };
        write_mp3_tags(&path, &tags).unwrap();

        let read = Tag::read_from_path(&path).unwrap();
        assert_eq!(read.title(), Some("Неон"));
        assert_eq!(read.album(), Some("Studio"));
        assert_eq!(read.artist(), Some("Local Studio"));
        assert_eq!(read.genre(), Some("Synth-pop"));
        assert_eq!(read.get("TBPM").and_then(|frame| frame.content().text()), Some("96"));
        assert_eq!(read.lyrics().next().map(|entry| entry.text.as_str()), Some("Неон дрожит над мокрым городом"));
        assert_eq!(read.pictures().next().map(|picture| picture.mime_type.as_str()), Some("image/png"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rewriting_replaces_rather_than_stacks() {
        let path = std::env::temp_dir().join(format!("studio-tag-{}.mp3", uuid::Uuid::now_v7()));
        std::fs::write(&path, sample_mp3()).unwrap();

        write_mp3_tags(&path, &TrackTags { title: "First".into(), ..TrackTags::default() }).unwrap();
        write_mp3_tags(&path, &TrackTags { title: "Second".into(), ..TrackTags::default() }).unwrap();

        let read = Tag::read_from_path(&path).unwrap();
        assert_eq!(read.title(), Some("Second"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_genre_is_a_phrase_not_a_document() {
        assert_eq!(genre_from_caption("Darkwave, Synth-pop. Global Emotional Progression: …"), Some("Darkwave".into()));
        assert_eq!(genre_from_caption("Global Metadata: Basic Attributes: bpm is 96. key is A"), None);
        assert_eq!(genre_from_caption("Global Metadata Basic Attributes: bpm is 95"), None);
        assert_eq!(genre_from_caption("Warm lo-fi hip hop instrumental, dusty drums"), Some("Warm lo-fi hip hop instrumental".into()));
    }

    #[test]
    fn the_tempo_is_read_from_the_caption() {
        assert_eq!(bpm_from_caption("Basic Attributes: bpm is 96. key is A"), Some(96));
        assert_eq!(bpm_from_caption("[tempo: 124 BPM] progressive house"), Some(124));
        assert_eq!(bpm_from_caption("no tempo here"), None);
    }

    /// A real caption keeps its genre behind the heading and the measurements.
    #[test]
    fn the_genre_is_found_behind_the_heading_and_the_numbers() {
        let caption = concat!(
            "Global Metadata\n",
            "Basic Attributes: bpm is 180. key is D, and scale is minor. ",
            "Melodic Death Metal / Gothenburg Sound. Global Emotional Progression: cold."
        );
        assert_eq!(genre_from_caption(caption).as_deref(), Some("Melodic Death Metal / Gothenburg Sound"));
    }

    #[test]
    fn a_caption_of_pure_measurements_names_no_genre() {
        assert_eq!(genre_from_caption(concat!("Global Metadata\n", "Basic Attributes: bpm is 96. key is A")), None);
    }

    #[test]
    fn a_style_names_its_genre_after_the_language_but_a_language_genre_stays() {
        assert_eq!(genre_from_caption("English, warm piano pop, expressive female voice").as_deref(), Some("warm piano pop"));
        assert_eq!(genre_from_caption("Russian folk rock, accordion, raspy male vocal").as_deref(), Some("Russian folk rock"));
    }
}
