//! The cover a track without one of its own wears, and what is written into it.
//!
//! The picture is a photograph from Wikimedia Commons that fits the track's
//! style, or a DiceBear pattern (CC0 styles) drawn from the track's seed. A stem
//! is drawn from the song it was separated from, so it wears its song's. The
//! window draws the same patterns with the same DiceBear version, so a cover
//! written into a track looks like the one shown before it.
//!
//! Photographs are CC0 files only: those that came from Unsplash while its
//! photographs were CC0 first, then any CC0 photograph. Commons answers from
//! Russia, needs no key, and asks for a User-Agent that names the client and
//! for no more than a few requests at a time.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

/// The pattern styles on offer, the default first; the window offers the same list.
pub const PATTERNS: [&str; 21] = [
    "waves", "squircles", "blobs", "constellation", "disco", "glass", "identicon", "landscape", "loops", "marbles",
    "patchwork", "planets", "rings", "shadows", "shape-grid", "shapes", "slice", "stack", "stripes", "triangles", "weave",
];

/// The side of the square a placeholder is written into a track at.
pub const KEPT_SIZE: u32 = 512;

/// How a track without a cover of its own looks, and whether that look is
/// written into the track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoverLook {
    /// The stock photo for the track; off, its pattern.
    #[serde(default = "on")]
    pub photo: bool,
    #[serde(default = "default_pattern")]
    pub pattern: String,
    /// The placeholder is written into the track as its cover, and into an
    /// MP3's tags, so a player shows it after the file is saved.
    #[serde(default = "on")]
    pub keep: bool,
}

fn on() -> bool {
    true
}

fn default_pattern() -> String {
    PATTERNS[0].to_owned()
}

impl Default for CoverLook {
    fn default() -> Self {
        Self { photo: true, pattern: default_pattern(), keep: true }
    }
}

impl CoverLook {
    /// The look with a pattern the studio does not have replaced by the default.
    pub fn checked(mut self) -> Self {
        if !PATTERNS.contains(&self.pattern.as_str()) {
            self.pattern = default_pattern();
        }
        self
    }

    /// What a placeholder drawn with this look is recorded as, so a change of
    /// look draws the placeholders again and leaves real covers alone.
    pub fn label(&self) -> String {
        if self.photo { "photo".to_owned() } else { format!("pattern:{}", self.pattern) }
    }
}

/// The seed a track's placeholder is drawn from: a stem wears its song's.
pub fn cover_seed(song: &crate::library::Song) -> String {
    let derived = &song.metadata["derived"];
    match (derived["tool"].as_str(), derived["from"].as_str()) {
        (Some("stems"), Some(from)) if !from.is_empty() => from.to_owned(),
        _ => song.id.clone(),
    }
}

/// The styles that draw nothing behind their shapes, and the backgrounds they
/// get instead, picked by the seed: a cover written into a file is never
/// see-through. The window's `coverArt.ts` passes the same.
const SEE_THROUGH: [&str; 2] = ["identicon", "rings"];
const BACKDROPS: [&str; 5] = ["b6e3f4", "c0aede", "d1d4f9", "ffd5dc", "ffdfbf"];

/// A pattern for a seed, as SVG.
pub fn pattern_svg(pattern: &str, seed: &str, size: u32) -> Result<String> {
    let definition = dicebear_styles::get(pattern).ok_or_else(|| anyhow!("there is no cover pattern '{pattern}'"))?;
    let style = dicebear_core::Style::from_str(definition).with_context(|| format!("read the cover pattern {pattern}"))?;
    let mut options = serde_json::json!({ "seed": seed, "size": size });
    if SEE_THROUGH.contains(&pattern) {
        options["backgroundColor"] = serde_json::json!(BACKDROPS);
    }
    let avatar = dicebear_core::Avatar::new(&style, options).with_context(|| format!("draw the cover pattern {pattern}"))?;
    Ok(avatar.to_svg().to_owned())
}

/// A pattern for a seed as the PNG that is written into a track.
pub fn pattern_png(pattern: &str, seed: &str, size: u32) -> Result<Vec<u8>> {
    let svg = pattern_svg(pattern, seed, size)?;
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).context("read the pattern's SVG")?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).ok_or_else(|| anyhow!("a {size}px canvas for the pattern"))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap.encode_png().context("encode the pattern as PNG")
}

/// What a photograph is looked for by: the words of a style name a genre, a
/// mood or an instrument, and each calls up a scene a photograph can show.
/// Two words at most - Commons search wants every word of a query in a file.
/// A genre says the most about a picture, an instrument the least.
const GENRES: &[(&str, &[&str])] = &[
    ("synthwave", &["neon lights", "city night"]),
    ("retrowave", &["neon lights", "city night"]),
    ("outrun", &["neon lights", "night highway"]),
    ("darksynth", &["neon lights", "night city"]),
    ("cyberpunk", &["neon city", "neon lights"]),
    ("vaporwave", &["neon lights", "palm trees"]),
    ("lo-fi", &["rain window", "city night"]),
    ("lofi", &["rain window", "city night"]),
    ("chillhop", &["rain window", "coffee cup"]),
    ("chillout", &["sunset beach", "misty lake"]),
    ("metal", &["lightning storm", "fire flames"]),
    ("metalcore", &["lightning storm", "fire flames"]),
    ("rock", &["electric guitar", "concert crowd"]),
    ("punk", &["graffiti wall", "city street"]),
    ("grunge", &["abandoned building", "rain street"]),
    ("indie", &["film camera", "city street"]),
    ("jazz", &["saxophone", "piano bar"]),
    ("blues", &["acoustic guitar", "river sunset"]),
    ("soul", &["vinyl record", "city night"]),
    ("funk", &["vinyl record", "disco ball"]),
    ("disco", &["disco ball", "dance floor"]),
    ("classical", &["piano keys", "concert hall"]),
    ("orchestral", &["mountain peak", "storm clouds"]),
    ("cinematic", &["mountain peak", "storm clouds"]),
    ("epic", &["mountain peak", "storm clouds"]),
    ("soundtrack", &["mountain peak", "starry sky"]),
    ("ambient", &["misty lake", "ocean horizon"]),
    ("new age", &["misty lake", "starry sky"]),
    ("meditative", &["misty lake", "zen garden"]),
    ("folk", &["forest path", "wheat field"]),
    ("acoustic", &["acoustic guitar", "forest path"]),
    ("country", &["country road", "ranch"]),
    ("pop", &["summer beach", "flower field"]),
    ("k-pop", &["city lights", "neon lights"]),
    ("j-pop", &["cherry blossom", "city lights"]),
    ("hip hop", &["graffiti wall", "city street"]),
    ("hip-hop", &["graffiti wall", "city street"]),
    ("rap", &["graffiti wall", "city street"]),
    ("trap", &["city night", "graffiti wall"]),
    ("drill", &["city night", "graffiti wall"]),
    ("r&b", &["city night", "neon lights"]),
    ("edm", &["concert lights", "dance floor"]),
    ("house", &["dance floor", "concert lights"]),
    ("techno", &["concert lights", "industrial building"]),
    ("trance", &["concert lights", "starry sky"]),
    ("dubstep", &["concert lights", "lightning storm"]),
    ("drum and bass", &["city night", "concert lights"]),
    ("dnb", &["city night", "concert lights"]),
    ("electronic", &["concert lights", "neon lights"]),
    ("dance", &["dance floor", "concert lights"]),
    ("reggae", &["tropical beach", "palm trees"]),
    ("latin", &["tropical beach", "dance floor"]),
    ("salsa", &["dance floor", "tropical beach"]),
    ("gospel", &["church interior", "sunrise"]),
    ("opera", &["opera house", "concert hall"]),
    ("choir", &["church interior", "concert hall"]),
];

const MOODS: &[(&str, &[&str])] = &[
    ("chill", &["sunset beach", "misty lake"]),
    ("sad", &["rain street", "autumn leaves"]),
    ("melancholic", &["rain window", "foggy forest"]),
    ("melancholy", &["rain window", "foggy forest"]),
    ("dark", &["night storm", "foggy forest"]),
    ("gloomy", &["foggy forest", "storm clouds"]),
    ("happy", &["sunflower field", "summer beach"]),
    ("uplifting", &["sunrise", "mountain peak"]),
    ("romantic", &["sunset beach", "red roses"]),
    ("love", &["red roses", "sunset beach"]),
    ("dreamy", &["pink clouds", "starry sky"]),
    ("calm", &["misty lake", "ocean horizon"]),
    ("peaceful", &["misty lake", "flower field"]),
    ("energetic", &["lightning storm", "fireworks"]),
    ("aggressive", &["fire flames", "lightning storm"]),
    ("angry", &["fire flames", "lightning storm"]),
    ("nostalgic", &["film camera", "vinyl record"]),
    ("summer", &["summer beach", "sunflower field"]),
    ("winter", &["snowy forest", "snow mountains"]),
    ("autumn", &["autumn leaves", "autumn forest"]),
    ("spring", &["cherry blossom", "flower field"]),
    ("night", &["city night", "starry sky"]),
    ("space", &["starry sky", "milky way"]),
    ("ocean", &["ocean waves", "ocean horizon"]),
    ("rain", &["rain window", "rain street"]),
];

const INSTRUMENTS: &[(&str, &[&str])] = &[
    ("piano", &["piano keys", "grand piano"]),
    ("guitar", &["acoustic guitar", "electric guitar"]),
    ("violin", &["violin", "concert hall"]),
    ("cello", &["cello", "concert hall"]),
    ("saxophone", &["saxophone", "piano bar"]),
    ("synthesizer", &["synthesizer", "neon lights"]),
    ("synth", &["synthesizer", "neon lights"]),
    ("drums", &["drum kit", "concert crowd"]),
];

/// What a style that names none of the scenes is shown with.
const ANY_MUSIC: &[&str] = &["concert lights", "starry sky", "sunset clouds"];

/// The scenes a style calls up, three at most: its genres first, then its
/// moods, then its instruments, each in the order the style names them.
pub fn scenes(style: &str) -> Vec<&'static str> {
    let spaced: String = style
        .to_lowercase()
        .chars()
        .map(|character| if character.is_alphanumeric() || character == '&' || character == '-' { character } else { ' ' })
        .collect();
    let spaced = format!(" {} ", spaced.split_whitespace().collect::<Vec<_>>().join(" "));
    let mut chosen: Vec<&'static str> = Vec::new();
    for table in [GENRES, MOODS, INSTRUMENTS] {
        let mut found: Vec<(usize, &'static str)> = Vec::new();
        for (word, scenes) in table {
            if let Some(at) = spaced.find(&format!(" {word} ")) {
                found.extend(scenes.iter().map(|scene| (at, *scene)));
            }
        }
        found.sort_by_key(|(at, _)| *at);
        for (_, scene) in found {
            if !chosen.contains(&scene) {
                chosen.push(scene);
            }
        }
    }
    if chosen.is_empty() {
        chosen.extend(ANY_MUSIC);
    }
    chosen.truncate(3);
    chosen
}

/// A photograph on Commons that can be a track's cover or a video's background.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Photo {
    /// The file's name on Commons, without "File:".
    pub title: String,
    /// The file's page: where it came from and under what terms.
    pub page: String,
    /// The picture at the width a cover is kept at.
    pub image: String,
    /// The picture at the width a choice is shown at.
    pub preview: String,
    /// The picture as wide as a full-HD frame, or the original when narrower.
    pub large: String,
}

/// A video clip on Commons that can be a video's background.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Clip {
    pub title: String,
    pub page: String,
    /// A still of the clip, for choosing it.
    pub poster: String,
    /// The clip as WebM, at a height a background needs.
    pub video: String,
    pub width: u64,
    pub height: u64,
}

const COMMONS_API: &str = "https://commons.wikimedia.org/w/api.php";
/// Commons draws its thumbnails at fixed widths and throttles every other one.
const KEPT_WIDTH: &str = "960px-";
const PREVIEW_WIDTH: &str = "500px-";
const SEARCH_KEPT: Duration = Duration::from_secs(24 * 60 * 60);
const PHOTOS_PER_SCENE: &str = "40";

/// Commons asks every client to name itself and a way to reach its authors.
pub fn commons_agent() -> String {
    let studio = music_core::studio();
    format!("{}/{} (https://github.com/{})", studio.name.replace(' ', "-"), env!("CARGO_PKG_VERSION"), studio.repo)
}

/// Commons asks for no more than a few requests at a time.
static COMMONS_TURNS: LazyLock<tokio::sync::Semaphore> = LazyLock::new(|| tokio::sync::Semaphore::new(2));
static FOUND: LazyLock<Mutex<HashMap<String, (Instant, Vec<Photo>)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Files whose names say they are not a picture of a scene.
static NOT_A_SCENE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)\b(map|diagram|chart|logo|poster|document|scan|stamp|coin|banknote|seal|flag|coat of arms|page|letter|diary|diaries|STS-\d+|NASA|Army|Navy|Air Force|Marines?|Defense\.gov|launch pad)\b",
    )
    .expect("a valid pattern")
});

/// Clips whose names say they are talk, news, war, a disaster or a weather
/// satellite's loop rather than something to play behind a song.
static NOT_A_BACKGROUND: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)\b(news|interview|lecture|speech|discuss\w*|debate|press|conference|briefing|hearing|trailer|bumper|VideoWiki|Wikipedia|tutorial|explainer|documentary|premiere|episode|podcast|VOA|war|bombing|attack|combat|military|soldiers?|weapons?|missile|gun|disaster|crash\w*|collapse\w*|accident|explosions?|explodes|shooting|police|body camera|security camera|surveillance|protests?|riot|funeral|hurricane|tornado|flood\w*|earthquake|wildfire|burn scar|engulfs|CIRA|GOES-\d+|labels|AI-Generated|Sora)\b",
    )
    .expect("a valid pattern")
});

/// The CC0 photographs Commons has for one scene: the Unsplash ones first.
pub async fn photos_of(client: &reqwest::Client, scene: &str) -> Result<Vec<Photo>> {
    if let Some((when, photos)) = FOUND.lock().expect("the photo cache").get(scene) {
        if when.elapsed() < SEARCH_KEPT {
            return Ok(photos.clone());
        }
    }
    let mut photos = search(client, &format!("{scene} incategory:\"Images from Unsplash\"")).await?;
    for photo in search(client, &format!("{scene} filetype:bitmap haswbstatement:P275=Q6938433")).await? {
        if !photos.iter().any(|known| known.title == photo.title) {
            photos.push(photo);
        }
    }
    FOUND.lock().expect("the photo cache").insert(scene.to_owned(), (Instant::now(), photos.clone()));
    Ok(photos)
}

async fn search(client: &reqwest::Client, query: &str) -> Result<Vec<Photo>> {
    let _turn = COMMONS_TURNS.acquire().await.context("wait for a turn at Commons")?;
    let response = client
        .get(COMMONS_API)
        .header(reqwest::header::USER_AGENT, commons_agent())
        .query(&[
            ("action", "query"),
            ("format", "json"),
            ("formatversion", "2"),
            ("generator", "search"),
            ("gsrsearch", query),
            ("gsrnamespace", "6"),
            ("gsrlimit", PHOTOS_PER_SCENE),
            ("prop", "imageinfo"),
            ("iiprop", "url|size|mime|extmetadata"),
            ("iiextmetadatafilter", "LicenseShortName"),
            ("iiurlwidth", "960"),
        ])
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .context("reach Wikimedia Commons")?;
    let status = response.status();
    if !status.is_success() {
        bail!("Wikimedia Commons answered {status} to a search");
    }
    let found: serde_json::Value = response.json().await.context("read the Commons search")?;
    Ok(photos_in(&found))
}

/// The results of a Commons search that can be a cover: CC0 JPEG photographs,
/// big enough, not a strip or a scan, in the order Commons ranked them.
pub fn photos_in(found: &serde_json::Value) -> Vec<Photo> {
    let mut pages: Vec<&serde_json::Value> = found["query"]["pages"].as_array().map(|pages| pages.iter().collect()).unwrap_or_default();
    pages.sort_by_key(|page| page["index"].as_i64().unwrap_or(i64::MAX));
    pages
        .into_iter()
        .filter_map(|page| {
            let title = page["title"].as_str()?.strip_prefix("File:")?.to_owned();
            let info = &page["imageinfo"][0];
            let (width, height) = (info["width"].as_u64()?, info["height"].as_u64()?);
            let license = info["extmetadata"]["LicenseShortName"]["value"].as_str()?;
            let kept = info["thumburl"].as_str()?.split('?').next()?.to_owned();
            let fits = info["mime"].as_str() == Some("image/jpeg")
                && license == "CC0"
                && width.min(height) >= 800
                && width.max(height) * 2 <= width.min(height) * 5
                && kept.contains(KEPT_WIDTH)
                && !NOT_A_SCENE.is_match(&title);
            fits.then(|| Photo {
                page: info["descriptionurl"].as_str().unwrap_or_default().to_owned(),
                preview: kept.replacen(KEPT_WIDTH, PREVIEW_WIDTH, 1),
                large: if width >= 1920 {
                    kept.replacen(KEPT_WIDTH, "1920px-", 1)
                } else {
                    info["url"].as_str().map(|url| url.split('?').next().unwrap_or(url).to_owned()).unwrap_or_else(|| kept.clone())
                },
                image: kept,
                title,
            })
        })
        .collect()
}

static FOUND_CLIPS: LazyLock<Mutex<HashMap<String, (Instant, Vec<Clip>)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Clips Commons has for a search, free of copyright: the CC0 ones first, then
/// the public-domain ones, which are mostly archive and agency footage.
pub async fn clips_of(client: &reqwest::Client, query: &str) -> Result<Vec<Clip>> {
    if let Some((when, clips)) = FOUND_CLIPS.lock().expect("the clip cache").get(query) {
        if when.elapsed() < SEARCH_KEPT {
            return Ok(clips.clone());
        }
    }
    let mut clips = search_clips(client, &format!("{query} filetype:video haswbstatement:P275=Q6938433")).await?;
    for clip in search_clips(client, &format!("{query} filetype:video haswbstatement:P6216=Q19652")).await? {
        if !clips.iter().any(|known| known.title == clip.title) {
            clips.push(clip);
        }
    }
    FOUND_CLIPS.lock().expect("the clip cache").insert(query.to_owned(), (Instant::now(), clips.clone()));
    Ok(clips)
}

async fn search_clips(client: &reqwest::Client, search: &str) -> Result<Vec<Clip>> {
    let found = {
        let _turn = COMMONS_TURNS.acquire().await.context("wait for a turn at Commons")?;
        let response = client
            .get(COMMONS_API)
            .header(reqwest::header::USER_AGENT, commons_agent())
            .query(&[
                ("action", "query"),
                ("format", "json"),
                ("formatversion", "2"),
                ("generator", "search"),
                ("gsrsearch", search),
                ("gsrnamespace", "6"),
                ("gsrlimit", PHOTOS_PER_SCENE),
                ("prop", "videoinfo"),
                ("viprop", "url|size|mime|derivatives|extmetadata"),
                ("viextmetadatafilter", "LicenseShortName"),
                ("viurlwidth", "960"),
            ])
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .context("reach Wikimedia Commons")?;
        let status = response.status();
        if !status.is_success() {
            bail!("Wikimedia Commons answered {status} to a search");
        }
        response.json::<serde_json::Value>().await.context("read the Commons search")?
    };
    Ok(clips_in(&found))
}

/// The clips of a Commons search a background can be: free of copyright, with
/// a WebM a window plays - 720p first, then full HD, then 480p.
pub fn clips_in(found: &serde_json::Value) -> Vec<Clip> {
    let mut pages: Vec<&serde_json::Value> = found["query"]["pages"].as_array().map(|pages| pages.iter().collect()).unwrap_or_default();
    pages.sort_by_key(|page| page["index"].as_i64().unwrap_or(i64::MAX));
    pages
        .into_iter()
        .filter_map(|page| {
            let title = page["title"].as_str()?.strip_prefix("File:")?.to_owned();
            let info = &page["videoinfo"][0];
            let license = info["extmetadata"]["LicenseShortName"]["value"].as_str()?;
            if !(license == "CC0" || license.eq_ignore_ascii_case("public domain")) || NOT_A_SCENE.is_match(&title) || NOT_A_BACKGROUND.is_match(&title) {
                return None;
            }
            let derivatives = info["derivatives"].as_array()?;
            let video = ["720p.vp9.webm", "1080p.vp9.webm", "480p.vp9.webm"].iter().find_map(|key| {
                derivatives.iter().find(|derivative| derivative["transcodekey"].as_str() == Some(key))?["src"].as_str()
            })?;
            Some(Clip {
                page: info["descriptionurl"].as_str().unwrap_or_default().to_owned(),
                poster: info["thumburl"].as_str()?.to_owned(),
                video: video.to_owned(),
                width: info["width"].as_u64().unwrap_or(0),
                height: info["height"].as_u64().unwrap_or(0),
                title,
            })
        })
        .collect()
}

/// The photographs a style calls up, scene after scene, each once.
pub async fn photos_for(client: &reqwest::Client, style: &str) -> Result<Vec<Photo>> {
    let mut all: Vec<Photo> = Vec::new();
    for scene in scenes(style) {
        for photo in photos_of(client, scene).await? {
            if !all.iter().any(|known| known.title == photo.title) {
                all.push(photo);
            }
        }
    }
    Ok(all)
}

/// The photograph a track wears: the same one for the same seed, and the next
/// one along for each variant asked for.
pub fn chosen<'a>(photos: &'a [Photo], seed: &str, variant: u64) -> Option<&'a Photo> {
    if photos.is_empty() {
        return None;
    }
    let start = seed.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    Some(&photos[(start.wrapping_add(variant) % photos.len() as u64) as usize])
}

/// A picture from Commons' image servers, and nowhere else.
pub async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let parsed = reqwest::Url::parse(url).context("read the picture's address")?;
    if parsed.scheme() != "https" || !matches!(parsed.host_str(), Some("upload.wikimedia.org" | "thumb.wikimedia.org")) {
        bail!("a cover photo comes from Wikimedia Commons only, not {url}");
    }
    let _turn = COMMONS_TURNS.acquire().await.context("wait for a turn at Commons")?;
    let response = client
        .get(parsed)
        .header(reqwest::header::USER_AGENT, commons_agent())
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .context("reach Wikimedia Commons")?;
    let status = response.status();
    if !status.is_success() {
        bail!("Wikimedia Commons answered {status} for {url}");
    }
    let bytes = response.bytes().await.context("download the photo")?;
    if bytes.len() < 1024 || bytes.len() > 16 * 1024 * 1024 {
        bail!("the photo from {url} is {} bytes, not a cover", bytes.len());
    }
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_draws_the_same_picture_for_the_same_seed() {
        for pattern in PATTERNS {
            let first = pattern_svg(pattern, "night", 64).unwrap();
            assert_eq!(first, pattern_svg(pattern, "night", 64).unwrap(), "{pattern} is drawn from its seed alone");
            assert_ne!(first, pattern_svg(pattern, "morning", 64).unwrap(), "{pattern} differs between seeds");
        }
        let png = pattern_png("waves", "night", 64).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert!(pattern_svg("adventurer", "night", 64).is_err(), "only the offered styles are drawn");
    }

    #[test]
    fn no_pattern_is_see_through() {
        for pattern in PATTERNS {
            for seed in ["night", "song-42", "a~3"] {
                let png = pattern_png(pattern, seed, 64).unwrap();
                let pixmap = resvg::tiny_skia::Pixmap::decode_png(&png).unwrap();
                assert!(pixmap.pixels().iter().all(|pixel| pixel.alpha() == 255), "{pattern} is see-through for {seed}");
            }
        }
    }

    #[test]
    fn a_look_keeps_to_the_offered_patterns() {
        let look: CoverLook = serde_json::from_value(serde_json::json!({ "pattern": "adventurer" })).unwrap();
        assert!(look.photo && look.keep, "photo and keeping are on unless turned off");
        assert_eq!(look.checked().pattern, "waves");
        assert_eq!(CoverLook { photo: false, pattern: "blobs".into(), keep: true }.label(), "pattern:blobs");
    }

    #[test]
    fn a_style_calls_up_scenes_in_the_order_of_its_words() {
        assert_eq!(scenes("dark synthwave, female vocals, 120 BPM"), ["neon lights", "city night", "night storm"]);
        assert_eq!(scenes("chill lo-fi hip hop"), ["rain window", "city night", "graffiti wall"], "a genre goes before a mood");
        assert_eq!(scenes("sad piano ballad"), ["rain street", "autumn leaves", "piano keys"]);
        assert_eq!(scenes("Russian chanson"), ["concert lights", "starry sky", "sunset clouds"], "an unknown style still gets a picture");
        assert_eq!(scenes("popular"), ANY_MUSIC, "a word is matched whole, not inside another");
    }

    #[test]
    fn only_cc0_photographs_of_a_scene_are_offered() {
        let page = |index: i64, title: &str, license: &str, mime: &str, width: u64, height: u64| {
            serde_json::json!({ "index": index, "title": format!("File:{title}"), "imageinfo": [{
                "width": width, "height": height, "mime": mime, "descriptionurl": format!("https://commons.wikimedia.org/wiki/File:{title}"),
                "thumburl": format!("https://upload.wikimedia.org/wikipedia/commons/thumb/a/ab/{title}/960px-{title}?utm_source=x"),
                "extmetadata": { "LicenseShortName": { "value": license } } }] })
        };
        let found = serde_json::json!({ "query": { "pages": [
            page(3, "Misty lake (Unsplash).jpg", "CC0", "image/jpeg", 4000, 3000),
            page(1, "Lake shore.jpg", "CC BY-SA 2.0", "image/jpeg", 4000, 3000),
            page(2, "Lake map.jpg", "CC0", "image/jpeg", 4000, 3000),
            page(4, "Lake panorama.jpg", "CC0", "image/jpeg", 9000, 2000),
            page(5, "Lake icon.jpg", "CC0", "image/jpeg", 400, 400),
            page(0, "Lake drawing.png", "CC0", "image/png", 4000, 3000),
            page(6, "Calm lake.jpg", "CC0", "image/jpeg", 3000, 3000),
        ] } });
        let photos = photos_in(&found);
        assert_eq!(photos.iter().map(|photo| photo.title.as_str()).collect::<Vec<_>>(), ["Misty lake (Unsplash).jpg", "Calm lake.jpg"]);
        assert!(photos[0].image.ends_with("/960px-Misty lake (Unsplash).jpg"));
        assert!(photos[0].preview.ends_with("/500px-Misty lake (Unsplash).jpg"));
        let first = chosen(&photos, "song-1", 0).unwrap();
        assert_eq!(chosen(&photos, "song-1", 0), Some(first), "the same seed keeps its photo");
        assert_ne!(chosen(&photos, "song-1", 1), Some(first), "the next variant is another photo");
        assert_eq!(chosen(&[], "song-1", 0), None);
    }

    #[test]
    fn a_clip_is_offered_free_of_copyright_as_a_webm_a_window_plays() {
        let page = |index: i64, title: &str, license: &str, keys: &[&str]| {
            let derivatives: Vec<serde_json::Value> = keys
                .iter()
                .map(|key| serde_json::json!({ "transcodekey": key, "src": format!("https://upload.wikimedia.org/transcoded/{title}.{key}") }))
                .collect();
            serde_json::json!({ "index": index, "title": format!("File:{title}"), "videoinfo": [{
                "width": 1920, "height": 1080, "descriptionurl": format!("https://commons.wikimedia.org/wiki/File:{title}"),
                "thumburl": format!("https://thumb.wikimedia.org/{title}.jpg"), "derivatives": derivatives,
                "extmetadata": { "LicenseShortName": { "value": license } } }] })
        };
        let found = serde_json::json!({ "query": { "pages": [
            page(2, "Waves.webm", "CC0", &["240p.vp9.webm", "480p.vp9.webm", "1080p.vp9.webm"]),
            page(1, "Clouds.webm", "Public domain", &["480p.vp9.webm"]),
            page(3, "Shared.webm", "CC BY-SA 4.0", &["720p.vp9.webm"]),
            page(4, "Old.ogv", "CC0", &["360p.mpeg4.mov"]),
            page(0, "Bridge crash security camera.webm", "Public domain", &["720p.vp9.webm"]),
        ] } });
        let clips = clips_in(&found);
        assert_eq!(clips.iter().map(|clip| clip.title.as_str()).collect::<Vec<_>>(), ["Clouds.webm", "Waves.webm"], "news footage never");
        assert!(clips[1].video.ends_with("Waves.webm.1080p.vp9.webm"), "full HD before 480p when there is no 720p");
    }
}
