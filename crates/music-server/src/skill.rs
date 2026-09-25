//! MiniMax's own caption skill, carried inside the binary.
//!
//! `music-caption-rewriter` is published with the model: a genre router,
//! eighteen family indexes and a thousand complete captions written the way
//! Music3 wants to be addressed. The skill is written for an agent that can
//! read files on demand, which the assistant here is not - it makes one chat
//! call - so the disclosure it describes happens in this module instead:
//! route the idea to a family, pick the closest cards from that family's
//! index, and put those complete captions in front of the model as
//! references.
//!
//! The whole skill is embedded rather than downloaded: it is 6 MB of text, it
//! must match the pinned model, and an assistant that silently fetches things
//! is exactly what this studio avoids.

use include_dir::{include_dir, Dir};

static SKILL: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/skills/music-caption-rewriter");

/// How many complete templates to put in the prompt. The skill asks for at
/// most three - Foundation, Modifier, Arrangement - and each is a few hundred
/// words, which is as much as a small local model will read carefully.
const MAX_REFERENCES: usize = 3;

/// One family of the router: the index file and the words that point at it.
struct Family {
    index: &'static str,
    cues: &'static [&'static str],
}

/// Transcribed from `references/genre-router.md`. The cues are that file's own
/// positive cues, lower-cased; the router's rule that "cinematic", "epic" and
/// friends are modifiers rather than genres is kept by leaving them out.
const FAMILIES: &[Family] = &[
    Family { index: "index-east-asian-modern.md", cues: &["mandopop", "c-pop", "cantopop", "j-pop", "k-pop"] },
    Family { index: "index-east-asian-ballad-heritage.md", cues: &["guofeng", "east asian ballad"] },
    Family { index: "index-modern-rnb-neo-soul.md", cues: &["r&b", "rnb", "neo-soul", "trap soul"] },
    Family { index: "index-soul-blues-gospel.md", cues: &["soul", "blues", "gospel", "worship"] },
    Family { index: "index-cinematic-pop-ballad.md", cues: &["cinematic pop", "cinematic ballad", "soundtrack ballad"] },
    Family { index: "index-cinematic-orchestral-epic.md", cues: &["film score", "orchestral", "trailer", "symphonic", "choral"] },
    Family { index: "index-electronic-synth-ambient-pop.md", cues: &["synth-pop", "synthpop", "electropop", "dream pop", "ambient", "darkwave", "retrowave", "synthwave", "downtempo"] },
    Family { index: "index-club-edm-house-trance.md", cues: &["edm", "house", "trance", "techno", "club", "festival", "drop"] },
    Family { index: "index-jazz-swing-big-band.md", cues: &["jazz", "big band", "swing", "bossa", "lounge"] },
    Family { index: "index-traditional-vocal-stage.md", cues: &["crooner", "doo-wop", "a cappella", "musical theatre", "show tune", "cabaret"] },
    Family { index: "index-hip-hop-rap.md", cues: &["hip-hop", "hip hop", "rap", "trap", "drill", "lo-fi hip"] },
    Family { index: "index-metal-heavy-rock.md", cues: &["metal", "metalcore", "symphonic metal", "hard rock", "post-hardcore", "nu-metal"] },
    Family { index: "index-pop-alternative-rock.md", cues: &["rock", "indie rock", "punk", "grunge", "j-rock", "arena"] },
    Family { index: "index-contemporary-folk-acoustic.md", cues: &["folk", "singer-songwriter", "acoustic"] },
    Family { index: "index-country-americana.md", cues: &["country", "americana", "bluegrass", "honky-tonk"] },
    Family { index: "index-dance-pop-disco-funk.md", cues: &["disco", "funk", "dance-pop", "nu-disco", "boogie"] },
    Family { index: "index-roots-traditional-global.md", cues: &["celtic", "traditional", "folk heritage", "world music", "reggae", "afrobeat", "latin"] },
    Family { index: "index-general-pop-ballad.md", cues: &["pop", "ballad"] },
];

/// The families the router lands on: the primary one, and a second when the
/// brief plainly names two styles. The skill asks for exactly that - one
/// family for a clear request, a primary and a secondary for a fusion.
fn route_all(brief: &str) -> Vec<&'static Family> {
    let lowered = brief.to_lowercase();
    let mut scored: Vec<(usize, &'static Family)> = FAMILIES
        .iter()
        .map(|family| {
            let weight: usize = family.cues.iter().filter(|cue| lowered.contains(*cue)).map(|cue| cue.len()).sum();
            (weight, family)
        })
        .filter(|(weight, _)| *weight > 0)
        .collect();
    scored.sort_by_key(|(weight, _)| std::cmp::Reverse(*weight));
    if scored.is_empty() {
        return vec![FAMILIES.last().expect("the router has families")];
    }
    // A second family only when it is a real second style, not a stray word:
    // half the primary's weight is the line the router's own examples draw.
    let primary = scored[0].0;
    scored
        .into_iter()
        .take(2)
        .filter(|(weight, _)| *weight * 2 >= primary)
        .map(|(_, family)| family)
        .collect()
}

/// The family the router lands on. Falls back to general pop, which is what
/// the skill says to do when only mood or imagery is available.
fn route(brief: &str) -> &'static Family {
    let lowered = brief.to_lowercase();
    let mut best: Option<(usize, &'static Family)> = None;
    for family in FAMILIES {
        // Weighted by how specific the match is, not how many words matched:
        // "symphonic metal" is metal, even though "symphonic" alone points at
        // the orchestral family. That is the router's own disambiguation rule.
        let weight: usize = family.cues.iter().filter(|cue| lowered.contains(*cue)).map(|cue| cue.len()).sum();
        if weight > 0 && weight > best.map(|(score, _)| score).unwrap_or(0) {
            best = Some((weight, family));
        }
    }
    best.map(|(_, family)| family).unwrap_or_else(|| FAMILIES.last().expect("the router has families"))
}

/// Template ids named by a family index, in the order the index lists them.
fn cards(index: &str) -> Vec<String> {
    let Some(file) = SKILL.get_file(format!("references/{index}")) else {
        return Vec::new();
    };
    let text = file.contents_utf8().unwrap_or_default();
    let mut ids = Vec::new();
    for line in text.lines() {
        // Cards name their template as `templates/<id>.txt`, in a link or bare.
        if let Some(start) = line.find("templates/") {
            let rest = &line[start + "templates/".len()..];
            if let Some(end) = rest.find(".txt") {
                let id = rest[..end].to_string();
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    ids
}

/// Scores a card id against the brief by the words in its own name: the file
/// names are the style, spelled out - `dark-synthwave-retro_0007`.
fn score(id: &str, brief: &str) -> usize {
    let lowered = brief.to_lowercase();
    id.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| part.len() > 3 && !part.chars().all(|c| c.is_ascii_digit()))
        .filter(|part| lowered.contains(&part.to_lowercase()))
        .count()
}

/// Complete reference captions for one brief, ready to put in a prompt.
///
/// Returns at most [`MAX_REFERENCES`] of them. An empty result is normal and
/// harmless: the contract alone still describes the shape.
pub fn references(brief: &str) -> Vec<String> {
    if brief.trim().is_empty() {
        return Vec::new();
    }
    let families = route_all(brief);
    let mut chosen: Vec<String> = Vec::new();
    for (position, family) in families.iter().enumerate() {
        let mut ids = cards(family.index);
        if ids.is_empty() {
            continue;
        }
        // Best match first, then the family's own order, which the index writes
        // most-representative first.
        ids.sort_by_key(|id| std::cmp::Reverse(score(id, brief)));
        // The primary family gives the foundation and the arrangement, the
        // secondary gives the one dimension it was chosen for.
        let take = if position == 0 { MAX_REFERENCES - families.len().saturating_sub(1) } else { 1 };
        for id in ids.into_iter().take(take) {
            if let Some(text) = SKILL.get_file(format!("templates/{id}.txt")).and_then(|file| file.contents_utf8()) {
                chosen.push(text.to_owned());
            }
        }
    }
    chosen.truncate(MAX_REFERENCES);
    chosen
}

/// The family index a brief routes to; useful in diagnostics and tests.
pub fn routed_index(brief: &str) -> &'static str {
    route(brief).index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_is_carried_whole() {
        assert!(SKILL.get_file("SKILL.md").is_some(), "the skill itself is missing");
        assert!(SKILL.get_file("references/genre-router.md").is_some(), "the router is missing");
        let templates = SKILL.get_dir("templates").expect("templates are missing");
        assert_eq!(templates.files().count(), 1000, "the template library is incomplete");
        for family in FAMILIES {
            assert!(SKILL.get_file(format!("references/{}", family.index)).is_some(), "{} is missing", family.index);
        }
    }

    #[test]
    fn a_brief_routes_to_the_family_the_router_names() {
        assert_eq!(routed_index("a dark synthwave night drive"), "index-electronic-synth-ambient-pop.md");
        assert_eq!(routed_index("melodic trap with 808s"), "index-hip-hop-rap.md");
        assert_eq!(routed_index("symphonic metal with choirs"), "index-metal-heavy-rock.md");
        // Only mood: the skill says fall back to general pop and ballad.
        assert_eq!(routed_index("something sad and beautiful"), "index-general-pop-ballad.md");
    }

    #[test]
    fn a_fusion_brings_a_second_family_in() {
        // The skill asks for a primary and a secondary family when two styles
        // are named, and for one family when only one is.
        let fusion = route_all("symphonic metal with orchestral choirs");
        assert_eq!(fusion.len(), 2, "a fusion should carry a secondary family");
        let single = route_all("a dark synthwave night drive");
        assert_eq!(single.len(), 1, "one style is one family");
    }

    #[test]
    fn references_are_complete_captions_from_the_routed_family() {
        let found = references("a dark synthwave night drive, female vocal");
        assert!(!found.is_empty(), "no reference captions were selected");
        assert!(found.len() <= MAX_REFERENCES);
        for reference in &found {
            assert!(reference.contains("Global Metadata"), "a reference is not a caption");
            assert!(reference.contains("Arrangement"), "a reference has no arrangement");
        }
    }
}
