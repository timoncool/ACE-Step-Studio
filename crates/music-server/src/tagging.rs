//! Tags on the tracks the studio writes.
//!
//! The engine returns bare audio: no title, no artist, no cover, no lyrics. A
//! file like that lands in a player as "instrumental-04" with a blank square,
//! which is the wrong answer for a track that has all of those things stored
//! next to it. ACE-Step Studio tagged its exports, and so does this one.
//!
//! Tags go in each format's own place, written by lofty: ID3v2.4 on an MP3,
//! Vorbis comments with a picture block on a FLAC, RIFF INFO and an id3 chunk
//! on a WAV. Tagging never fails a
//! request: if a tag cannot be written the audio is still the audio, and the
//! caller says so in the log rather than losing the track.

use std::path::Path;

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{FileType, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::probe::Probe;
use lofty::tag::Tag;

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

/// The formats the studio tags: the ones its tracks are kept in.
pub fn taggable(path: &Path) -> bool {
    matches!(FileType::from_path(path), Some(FileType::Mpeg | FileType::Flac | FileType::Wav))
}

/// Whether the file's own tag names the track. A FLAC always has a comment
/// block, holding at least the encoder's name, so a block alone is not a tag.
pub fn is_tagged(path: &Path) -> anyhow::Result<bool> {
    let file = Probe::open(path)?.options(ParseOptions::new().read_properties(false)).read()?;
    Ok(file.primary_tag().is_some_and(|tag| tag.title().is_some()))
}

/// The tempo field of a tag: ID3 keeps a whole number in TBPM, Vorbis comments a BPM.
fn bpm_key(tag_type: lofty::tag::TagType) -> ItemKey {
    if tag_type == lofty::tag::TagType::Id3v2 { ItemKey::IntegerBpm } else { ItemKey::Bpm }
}

/// The lyrics field of a tag: ID3's USLT frame, Vorbis comments' LYRICS.
fn lyrics_key(tag_type: lofty::tag::TagType) -> ItemKey {
    if tag_type == lofty::tag::TagType::Id3v2 { ItemKey::UnsyncLyrics } else { ItemKey::Lyrics }
}

/// Writes the tags onto an MP3, a FLAC or a WAV in place, replacing the tag that was there.
pub fn write_tags(path: &Path, tags: &TrackTags) -> anyhow::Result<()> {
    let file_type = FileType::from_path(path).ok_or_else(|| anyhow::anyhow!("{} is not an audio file the studio tags", path.display()))?;
    let mut tag = Tag::new(file_type.primary_tag_type());
    tag.set_title(tags.title.clone());
    if !tags.album.is_empty() {
        tag.set_album(tags.album.clone());
    }
    if !tags.artist.is_empty() {
        tag.set_artist(tags.artist.clone());
    }
    if let Some(genre) = tags.genre.as_deref().filter(|value| !value.trim().is_empty()) {
        tag.set_genre(genre.to_string());
    }
    if let Some(bpm) = tags.bpm {
        tag.insert_text(bpm_key(tag.tag_type()), bpm.to_string());
    }
    if let Some(lyrics) = tags.lyrics.as_deref().filter(|value| !value.trim().is_empty()) {
        tag.insert_text(lyrics_key(tag.tag_type()), lyrics.to_string());
    }
    if let Some((media_type, image)) = tags.cover.as_ref() {
        tag.push_picture(
            Picture::unchecked(image.clone())
                .pic_type(PictureType::CoverFront)
                .mime_type(MimeType::from_str(media_type))
                .description("Cover")
                .build(),
        );
    }
    tag.save_to_path(path, WriteOptions::default())?;
    if file_type == FileType::Wav {
        // RIFF INFO is what most readers of a WAV look at; the id3 chunk holds the lyrics and cover
        let mut info = tag;
        info.re_map(lofty::tag::TagType::RiffInfo);
        info.save_to_path(path, WriteOptions::default())?;
    }
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
    // a prose description names its music in its first words
    if let Some(subject) = crate::auto_title::subject(caption) {
        return Some(subject);
    }
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

    fn sample(extension: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("studio-tag-{}.{extension}", uuid::Uuid::now_v7()));
        let silence = audio_post::Stereo { left: vec![0.0; 4800], right: vec![0.0; 4800], rate: 48_000 };
        match extension {
            "mp3" => std::fs::write(&path, audio_post::encode::mp3(&silence, 320).unwrap()).unwrap(),
            "flac" => std::fs::write(&path, audio_post::encode::flac(&silence).unwrap()).unwrap(),
            _ => crate::audio_pcm::write_wav_f32(&path, &silence).unwrap(),
        }
        path
    }

    /// A 1x1 PNG.
    fn png() -> Vec<u8> {
        vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0x60, 0xf8, 0x5f, 0x0f, 0x00, 0x02, 0x87, 0x01, 0x80, 0xeb, 0x47, 0xba, 0x92, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82]
    }

    #[test]
    fn a_written_tag_reads_back_from_every_kept_format() {
        for extension in ["mp3", "flac", "wav"] {
            let path = sample(extension);
            assert!(taggable(&path));
            assert!(!is_tagged(&path).unwrap());
            let tags = TrackTags {
                title: "Неон".into(),
                album: "Studio".into(),
                artist: "Local Studio".into(),
                genre: Some("Synth-pop".into()),
                lyrics: Some("Неон дрожит над мокрым городом".into()),
                bpm: Some(96),
                cover: Some(("image/png".into(), png())),
            };
            write_tags(&path, &tags).unwrap();

            assert!(is_tagged(&path).unwrap());
            let file = lofty::read_from_path(&path).unwrap();
            let read = file.primary_tag().unwrap();
            assert_eq!(read.title().as_deref(), Some("Неон"), "{extension}");
            assert_eq!(read.album().as_deref(), Some("Studio"));
            assert_eq!(read.artist().as_deref(), Some("Local Studio"));
            assert_eq!(read.genre().as_deref(), Some("Synth-pop"));
            assert_eq!(read.get_string(bpm_key(read.tag_type())), Some("96"), "{extension}");
            assert_eq!(read.get_string(lyrics_key(read.tag_type())), Some("Неон дрожит над мокрым городом"), "{extension}");
            assert_eq!(read.pictures().first().map(|picture| (picture.pic_type(), picture.mime_type().cloned())), Some((PictureType::CoverFront, Some(MimeType::Png))));
            // Symphonia streams a WAV and reads only the INFO ahead of its data
            if extension != "wav" {
                let imported = crate::audio_pcm::tags(&path);
                assert_eq!(imported.title, "Неон", "{extension}");
                assert_eq!(imported.artist, "Local Studio");
                assert_eq!(imported.lyrics, "Неон дрожит над мокрым городом");
            }
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn rewriting_replaces_rather_than_stacks() {
        for extension in ["mp3", "flac", "wav"] {
            let path = sample(extension);
            write_tags(&path, &TrackTags { title: "First".into(), cover: Some(("image/png".into(), png())), ..TrackTags::default() }).unwrap();
            write_tags(&path, &TrackTags { title: "Second".into(), cover: Some(("image/png".into(), png())), ..TrackTags::default() }).unwrap();

            let file = lofty::read_from_path(&path).unwrap();
            let read = file.primary_tag().unwrap();
            assert_eq!(read.title().as_deref(), Some("Second"), "{extension}");
            assert_eq!(read.get_strings(ItemKey::TrackTitle).count(), 1);
            assert_eq!(read.pictures().len(), 1);
            let _ = std::fs::remove_file(path);
        }
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
    fn a_planned_description_gives_its_subject_as_the_genre() {
        let caption = "A classic lo-fi hip-hop instrumental built on a dusty, sampled drum break. A clean, jazzy electric guitar plays a motif.";
        assert_eq!(genre_from_caption(caption).as_deref(), Some("Classic lo-fi hip-hop instrumental"));
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
