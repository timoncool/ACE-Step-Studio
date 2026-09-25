//! Lyrics from the databases music players read them from, before anything is
//! recognised by ear. The providers and their requests are the ones LyricsX
//! (LyricsKit) and syncedlyrics use: LRCLIB first, the open one, then
//! QQ Music and Kugou. Musixmatch, NetEase and Genius are not asked: the
//! first no longer gives anonymous clients a working token, the second asks
//! them to verify a phone number, the third refuses clients that are not a
//! browser. A song is taken from a source only
//! when the title, the artist and the length agree, so a namesake by someone
//! else is never stored as the song's lyrics.

use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use regex::Regex;
use serde_json::Value;

/// What a song is looked up by.
#[derive(Debug, Clone)]
pub struct Song {
    pub artist: String,
    pub title: String,
    /// The recording's length; a candidate more than `LENGTH_SLACK` away is
    /// a different recording.
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// The words, one line each, and the same lines with their times where
    /// the source had them.
    Lyrics { plain: String, timed: String, source: &'static str },
    /// The source knows the song and says it has no words.
    Instrumental { source: &'static str },
}

const LENGTH_SLACK: f64 = 10.0;
/// How many sources a song is asked of.
pub const SOURCES: usize = 3;
const BROWSER: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0 Safari/537.36";

/// One candidate a provider offered.
struct Candidate {
    artist: String,
    title: String,
    seconds: Option<f64>,
    key: String,
    instrumental: bool,
}

/// The providers, with what they keep between songs of one run.
pub struct Sources {
    http: reqwest::Client,
}

impl Sources {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .context("build the lyrics client")?;
        Ok(Self { http })
    }

    /// The song from the first source that knows it. `failed` gets the
    /// sources that could not be asked at all, with why.
    pub async fn find(&self, song: &Song, failed: &mut Vec<String>) -> Option<Found> {
        if song.title.trim().is_empty() {
            return None;
        }
        for source in ["LRCLIB", "QQ Music", "Kugou"] {
            let outcome = match source {
                "LRCLIB" => self.lrclib(song).await,
                "QQ Music" => self.qq(song).await,
                _ => self.kugou(song).await,
            };
            match outcome {
                Ok(Some(found)) => return Some(found),
                Ok(None) => {}
                Err(error) => failed.push(format!("{source}: {error:#}")),
            }
        }
        None
    }

    // ---------------------------------------------------------------- LRCLIB

    async fn lrclib(&self, song: &Song) -> Result<Option<Found>> {
        let mut seen = Vec::new();
        let agent = music_core::studio().user_agent(env!("CARGO_PKG_VERSION"));
        for (artist, title) in queries(song) {
            let list: Value = send(self
                .http
                .get("https://lrclib.net/api/search")
                .header("User-Agent", &agent)
                .header("Lrclib-Client", &agent)
                .query(&[("track_name", title.as_str()), ("artist_name", artist.as_str())])).await?                .error_for_status()?
                .json()
                .await?;
            for entry in list.as_array().into_iter().flatten() {
                let id = entry["id"].to_string();
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                let candidate = Candidate {
                    artist: text(&entry["artistName"]),
                    title: text(&entry["trackName"]),
                    seconds: entry["duration"].as_f64(),
                    key: String::new(),
                    instrumental: entry["instrumental"].as_bool().unwrap_or(false),
                };
                if !matches(song, &candidate) {
                    continue;
                }
                if candidate.instrumental {
                    return Ok(Some(Found::Instrumental { source: "LRCLIB" }));
                }
                let synced = text(&entry["syncedLyrics"]);
                let plain = text(&entry["plainLyrics"]);
                if let Some(found) = lyrics(if synced.trim().is_empty() { &plain } else { &synced }, "LRCLIB", song) {
                    return Ok(Some(found));
                }
            }
        }
        Ok(None)
    }

    // -------------------------------------------------------------- QQ Music

    async fn qq(&self, song: &Song) -> Result<Option<Found>> {
        let mut seen = Vec::new();
        for query in keywords(song) {
            let request = serde_json::json!({
                "req_1": {
                    "method": "DoSearchForQQMusicDesktop",
                    "module": "music.search.SearchCgiService",
                    "param": { "num_per_page": 10, "page_num": 1, "query": query, "search_type": 0 }
                }
            });
            let body: Value = send(self
                .http
                .post("https://u.y.qq.com/cgi-bin/musicu.fcg")
                .header("User-Agent", BROWSER)
                .header("Referer", "https://y.qq.com/")
                .json(&request)).await?                .error_for_status()?
                .json()
                .await?;
            for entry in body["req_1"]["data"]["body"]["song"]["list"].as_array().into_iter().flatten() {
                let key = text(&entry["mid"]);
                if key.is_empty() || seen.contains(&key) {
                    continue;
                }
                seen.push(key.clone());
                let candidate = Candidate {
                    artist: entry["singer"].as_array().into_iter().flatten().map(|singer| text(&singer["name"])).collect::<Vec<_>>().join(", "),
                    title: text(&entry["name"]),
                    seconds: entry["interval"].as_f64().filter(|seconds| *seconds > 0.0),
                    key,
                    instrumental: false,
                };
                if !matches(song, &candidate) {
                    continue;
                }
                let lyric: Value = send(self
                    .http
                    .get("https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg")
                    .header("User-Agent", BROWSER)
                    .header("Referer", "https://y.qq.com/")
                    .query(&[("songmid", candidate.key.as_str()), ("format", "json"), ("nobase64", "1"), ("g_tk", "5381")])).await?                    .error_for_status()?
                    .json()
                    .await?;
                let body = unescape_html(&text(&lyric["lyric"]));
                if says_instrumental(&body) {
                    return Ok(Some(Found::Instrumental { source: "QQ Music" }));
                }
                if let Some(found) = lyrics(&body, "QQ Music", song) {
                    return Ok(Some(found));
                }
            }
        }
        Ok(None)
    }

    // ----------------------------------------------------------------- Kugou

    async fn kugou(&self, song: &Song) -> Result<Option<Found>> {
        let mut seen = Vec::new();
        for query in keywords(song) {
            let body: Value = send(self
                .http
                .get("http://mobilecdn.kugou.com/api/v3/search/song")
                .header("User-Agent", BROWSER)
                .query(&[("format", "json"), ("keyword", query.as_str()), ("page", "1"), ("pagesize", "10"), ("showtype", "1")])).await?                .error_for_status()?
                .json()
                .await?;
            for entry in body["data"]["info"].as_array().into_iter().flatten() {
                let key = text(&entry["hash"]);
                if key.is_empty() || seen.contains(&key) {
                    continue;
                }
                seen.push(key.clone());
                let candidate = Candidate {
                    artist: text(&entry["singername"]),
                    title: text(&entry["songname"]),
                    seconds: entry["duration"].as_f64().filter(|seconds| *seconds > 0.0),
                    key,
                    instrumental: false,
                };
                if !matches(song, &candidate) {
                    continue;
                }
                let search: Value = send(self
                    .http
                    .get("https://krcs.kugou.com/search")
                    .header("User-Agent", BROWSER)
                    .query(&[("ver", "1"), ("man", "yes"), ("client", "mobi"), ("keyword", ""), ("duration", ""), ("hash", candidate.key.as_str()), ("album_audio_id", &entry["album_audio_id"].to_string())])).await?                    .error_for_status()?
                    .json()
                    .await?;
                let Some(lyric) = search["candidates"].as_array().and_then(|list| list.first()) else { continue };
                let download: Value = send(self
                    .http
                    .get("https://lyrics.kugou.com/download")
                    .header("User-Agent", BROWSER)
                    .query(&[("ver", "1"), ("client", "pc"), ("id", &text(&lyric["id"])), ("accesskey", &text(&lyric["accesskey"])), ("fmt", "lrc"), ("charset", "utf8")])).await?                    .error_for_status()?
                    .json()
                    .await?;
                let bytes = base64::engine::general_purpose::STANDARD.decode(text(&download["content"])).map_err(|error| anyhow!("undecodable lyrics: {error}"))?;
                let body = String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string();
                if says_instrumental(&body) {
                    return Ok(Some(Found::Instrumental { source: "Kugou" }));
                }
                if let Some(found) = lyrics(&body, "Kugou", song) {
                    return Ok(Some(found));
                }
            }
        }
        Ok(None)
    }
}

/// Sends a request, again after a pause when the service is busy (429,
/// 5xx) or the connection failed: lyrics services shed load under bursts.
async fn send(request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
    let mut last = None;
    for pause in [0u64, 1, 3] {
        if pause > 0 {
            tokio::time::sleep(Duration::from_secs(pause)).await;
        }
        let Some(attempt) = request.try_clone() else { break };
        match attempt.send().await {
            Ok(response) if response.status().as_u16() == 429 || response.status().is_server_error() => last = Some(anyhow!("HTTP {}", response.status())),
            Ok(response) => return Ok(response),
            Err(error) => last = Some(anyhow!(error)),
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("the request could not be sent")))
}


fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

// ------------------------------------------------------------------ queries

/// A title without what files add to it: "(feat. X)", "(EP)", "[Remastered 2011]", "- Live".
fn bare_title(title: &str) -> String {
    static BRACKETS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[\(\[\{][^\)\]\}]*[\)\]\}]").expect("valid regex"));
    static TAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s+-\s+(live|remaster(ed)?|radio edit|single version|mono|stereo|bonus track).*$").expect("valid regex"));
    static FEAT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s+(feat\.?|ft\.?|featuring)\s+.*$").expect("valid regex"));
    let bare = BRACKETS.replace_all(title, "");
    let bare = TAIL.replace_all(&bare, "");
    FEAT.replace_all(&bare, "").trim().to_string()
}

/// The first artist of "A feat. B", "A & B", "A, B", "A x B".
fn first_artist(artist: &str) -> String {
    static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s+(feat\.?|ft\.?|featuring|x|vs\.?|and|и)\s+|\s*[,&;/]\s*").expect("valid regex"));
    SPLIT.split(artist).next().unwrap_or(artist).trim().to_string()
}

/// Artist and title pairs to ask with, most exact first, without repeats.
fn queries(song: &Song) -> Vec<(String, String)> {
    let mut list = Vec::new();
    let artists = [song.artist.trim().to_string(), first_artist(&song.artist)];
    let titles = [song.title.trim().to_string(), bare_title(&song.title), song.title.replace(['ё', 'Ё'], "е")];
    for artist in &artists {
        for title in &titles {
            let pair = (artist.clone(), title.clone());
            if !title.is_empty() && !list.contains(&pair) {
                list.push(pair);
            }
        }
    }
    list
}

/// One-line searches for the providers that only take a keyword.
fn keywords(song: &Song) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    for (artist, title) in queries(song) {
        let keyword = format!("{artist} {title}").trim().to_string();
        if !list.contains(&keyword) {
            list.push(keyword);
        }
    }
    list.truncate(3);
    list
}

// ----------------------------------------------------------------- matching

/// Letters and digits only, lower case, "ё" as "е", Cyrillic spelled in Latin
/// letters, so "Neiromonakh Feofan" and "Нейромонах Феофан" compare equal.
fn comparable(text: &str) -> String {
    let mut out = String::new();
    for c in text.to_lowercase().chars() {
        let latin = match c {
            'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' | 'ё' | 'э' => "e", 'ж' => "zh", 'з' => "z",
            'и' | 'й' | 'ы' => "i", 'к' => "k", 'л' => "l", 'м' => "m", 'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r",
            'с' => "s", 'т' => "t", 'у' => "u", 'ф' => "f", 'х' => "h", 'ц' => "ts", 'ч' => "ch", 'ш' => "sh", 'щ' => "sch",
            'ъ' | 'ь' => "", 'ю' => "yu", 'я' => "ya", 'і' => "i", 'ї' => "i", 'є' => "e", 'ґ' => "g",
            'y' => "i", 'j' => "i", 'x' => "h", 'w' => "v", 'q' => "k", 'c' => "k",
            _ if c.is_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => continue,
        };
        out.push_str(latin);
    }
    // "kh" and "h" are both how Latin spells "х"; "ks" and "x" are one sound
    out.replace("kh", "h").replace("ks", "h")
}

fn contains_either(a: &str, b: &str, least: usize) -> bool {
    !a.is_empty() && !b.is_empty() && (a == b || (a.chars().count() >= least && b.contains(a)) || (b.chars().count() >= least && a.contains(b)))
}

fn cyrillic(text: &str) -> bool {
    text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// The consonants of a romanised spelling: "Zamirenie" and "Замиренье"
/// romanise differently in their vowels, never in their consonants.
fn skeleton(text: &str) -> String {
    comparable(text).chars().filter(|c| !"aeiou".contains(*c)).collect()
}

fn close(ours: &str, theirs: &str) -> bool {
    if contains_either(&comparable(ours), &comparable(theirs), 4) {
        return true;
    }
    // written in two alphabets: compare what a romanisation cannot change
    cyrillic(ours) != cyrillic(theirs) && contains_either(&skeleton(ours), &skeleton(theirs), 3)
}

/// Titles are close when one holds most of the other: "Rain" is not "Rain
/// Dance", while "Замиренье" is "Замиренье (feat. Drummatix)" once bared.
fn close_title(ours: &str, theirs: &str) -> bool {
    let (a, b) = (comparable(ours).chars().count(), comparable(theirs).chars().count());
    close(ours, theirs) && a.min(b) * 10 >= a.max(b) * 7
}

/// The same recording: the title agrees, the artist agrees when both are
/// known, and the length is within `LENGTH_SLACK`.
fn matches(song: &Song, candidate: &Candidate) -> bool {
    let title = close_title(&song.title, &candidate.title) || close_title(&bare_title(&song.title), &bare_title(&candidate.title));
    let artist = song.artist.trim().is_empty() || close(&song.artist, &candidate.artist) || close(&first_artist(&song.artist), &candidate.artist);
    let length = match candidate.seconds {
        Some(seconds) if song.seconds > 0.0 => (seconds - song.seconds).abs() <= LENGTH_SLACK,
        _ => true,
    };
    title && artist && length
}

// ------------------------------------------------------------------- lyrics

fn dash_heading(line: &str) -> bool {
    line.contains(" - ") || line.contains(" – ") || line.contains(" — ")
}

fn says_instrumental(body: &str) -> bool {
    let plain = body.to_lowercase();
    ["纯音乐，请欣赏", "纯音乐, 请欣赏", "此歌曲为没有填词的纯音乐", "instrumental"].iter().any(|marker| {
        let words = plain.lines().map(|line| strip_times(line).trim().to_string()).filter(|line| !line.is_empty()).collect::<Vec<_>>();
        words.len() <= 2 && words.iter().any(|line| line.contains(marker))
    })
}

fn strip_times(line: &str) -> String {
    static TIMES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\d{1,3}:\d{1,2}(?:[.:]\d{1,3})?\]|<\d{1,3}:\d{1,2}(?:[.:]\d{1,3})?>").expect("valid regex"));
    TIMES.replace_all(line, "").to_string()
}

fn unescape_html(text: &str) -> String {
    static ENTITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&#(\d+);").expect("valid regex"));
    ENTITY
        .replace_all(text, |found: &regex::Captures| found[1].parse::<u32>().ok().and_then(char::from_u32).map(String::from).unwrap_or_default())
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// The words of an LRC or plain text: time tags, metadata tags, credit lines
/// and a source's own notices taken out; the timed form keeps the start of
/// each line as "[m:ss]". None when no words are left.
fn lyrics(body: &str, source: &'static str, song: &Song) -> Option<Found> {
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[(ar|ti|al|au|by|offset|length|re|ve|la|id|#)\s*:.*\]$").expect("valid regex"));
    static CREDIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(作词|作曲|编曲|制作人|作詞|編曲|词|曲|合声|和声|和聲|混音|录音|錄音|母带|母帶|吉他|贝斯|貝斯|鼓|弦乐|弦樂|制作|製作|监制|監製|出品|发行|發行|op|sp|lyrics by|composed by|written by|music by|producer|arranged by|слова|музыка)\s*[:：]").expect("valid regex"));
    static START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[(\d{1,3}):(\d{1,2})(?:[.:](\d{1,3}))?\]").expect("valid regex"));
    let mut plain: Vec<String> = Vec::new();
    let mut timed: Vec<String> = Vec::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            if plain.last().is_some_and(|last| !last.is_empty()) {
                plain.push(String::new());
            }
            continue;
        }
        if TAG.is_match(line) || line.contains("This Lyrics is NOT for Commercial use") || line.starts_with("******") || line.chars().all(|c| c.is_ascii_digit() || c == '(' || c == ')') {
            continue;
        }
        let at = START.captures(line).map(|found| found[1].parse::<u64>().unwrap_or(0) * 60 + found[2].parse::<u64>().unwrap_or(0));
        let words = strip_times(line).trim().to_string();
        if words.is_empty() {
            if plain.last().is_some_and(|last| !last.is_empty()) {
                plain.push(String::new());
            }
            continue;
        }
        if CREDIT.is_match(&words) {
            continue;
        }
        // "晴天 - 周杰伦 (Jay Chou)": the sources open with the song's own heading
        let heading = comparable(&words);
        if plain.is_empty() && dash_heading(&words) && heading.contains(&comparable(&song.title)) && (song.artist.trim().is_empty() || heading.contains(&comparable(&first_artist(&song.artist)))) {
            continue;
        }
        timed.push(match at {
            Some(seconds) => format!("[{}:{:02}] {words}", seconds / 60, seconds % 60),
            None => words.clone(),
        });
        plain.push(words);
    }
    while plain.last().is_some_and(|last| last.is_empty()) {
        plain.pop();
    }
    if plain.iter().all(|line| line.is_empty()) {
        return None;
    }
    Some(Found::Lyrics { plain: plain.join("\n"), timed: timed.join("\n"), source })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(artist: &str, title: &str, seconds: f64) -> Song {
        Song { artist: artist.into(), title: title.into(), seconds }
    }

    fn candidate(artist: &str, title: &str, seconds: Option<f64>) -> Candidate {
        Candidate { artist: artist.into(), title: title.into(), seconds, key: String::new(), instrumental: false }
    }

    #[test]
    fn a_match_needs_title_artist_and_length() {
        let ours = song("Нейромонах Феофан", "Замиренье (feat. Drummatix)", 301.0);
        assert!(matches(&ours, &candidate("Нейромонах Феофан, Drummatix", "Замиренье", Some(299.0))));
        assert!(matches(&ours, &candidate("Neiromonakh Feofan", "Zamirenie", Some(303.0))));
        assert!(!matches(&ours, &candidate("Нейромонах Феофан", "Замиренье", Some(240.0))));
        assert!(!matches(&ours, &candidate("Другой исполнитель", "Замиренье", Some(301.0))));
        assert!(!matches(&ours, &candidate("Нейромонах Феофан", "Ураган", Some(301.0))));
        assert!(matches(&song("Монеточка", "Каждый раз", 208.0), &candidate("Монеточка", "Каждый раз", Some(209.0))));
        assert!(matches(&song("Queen", "Bohemian Rhapsody - Remastered 2011", 355.0), &candidate("Queen", "Bohemian Rhapsody", Some(354.0))));
        assert!(!matches(&song("Artist", "Rain", 210.0), &candidate("Artist", "Rain Dance", Some(214.0))));
    }

    #[test]
    fn queries_try_the_bare_title_and_the_first_artist() {
        let list = queries(&song("Нейромонах Феофан feat. Drummatix", "Ёлка (EP)", 200.0));
        assert!(list.contains(&("Нейромонах Феофан feat. Drummatix".into(), "Ёлка (EP)".into())));
        assert!(list.contains(&("Нейромонах Феофан".into(), "Ёлка".into())));
        assert!(list.contains(&("Нейромонах Феофан".into(), "елка (EP)".into())));
    }

    #[test]
    fn lrc_becomes_plain_and_timed_lines() {
        let body = "[ar:Монеточка]\n[ti:Каждый раз]\n[00:12.34]Если б мне платили каждый раз,\n[00:15.10]каждый раз, когда я думаю о тебе\n[00:20.00]\n[00:21.00]Я бы бомжевала\n";
        let Some(Found::Lyrics { plain, timed, source }) = lyrics(body, "LRCLIB", &song("Монеточка", "Каждый раз", 208.0)) else { panic!("no lyrics") };
        assert_eq!(source, "LRCLIB");
        assert_eq!(plain, "Если б мне платили каждый раз,\nкаждый раз, когда я думаю о тебе\n\nЯ бы бомжевала");
        assert!(timed.starts_with("[0:12] Если б мне платили"));
        assert!(lyrics("[00:00.00]作词 : 某人\n[00:01.00]作曲 : 某人\n", "QQ Music", &song("x", "y", 0.0)).is_none());
        let Some(Found::Lyrics { plain, .. }) = lyrics("[00:00.10]晴天 - 周杰伦 (Jay Chou)\n[00:29.50]故事的小黄花\n", "Kugou", &song("周杰伦", "晴天", 269.0)) else { panic!("no lyrics") };
        assert_eq!(plain, "故事的小黄花");
        assert!(says_instrumental("[00:00.00]纯音乐，请欣赏"));
        assert!(!says_instrumental("[00:00.00]A song about an instrumental\n[00:02.00]with words\n[00:04.00]and more"));
        assert_eq!(unescape_html("It&#39;s &amp; me"), "It's & me");
    }

    /// Runs the cascade over real files: LYRICS_DB_SONGS is a folder, tags
    /// or file names give artist and title. Network; run by hand.
    #[tokio::test]
    #[ignore]
    async fn finds_lyrics_for_a_real_library() {
        let Some(folder) = std::env::var_os("LYRICS_DB_SONGS") else { return };
        let sources = Sources::new().expect("client");
        let mut files = Vec::new();
        let root = std::path::PathBuf::from(&folder);
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().and_then(|value| value.to_str()).is_some_and(|ext| ["flac", "mp3", "m4a", "ogg", "wav"].contains(&ext.to_lowercase().as_str())) {
                    files.push(path);
                }
            }
        }
        files.sort();
        let limit: usize = std::env::var("LYRICS_DB_LIMIT").ok().and_then(|value| value.parse().ok()).unwrap_or(40);
        let mut tally: std::collections::BTreeMap<String, usize> = Default::default();
        for path in files.iter().take(limit) {
            let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
            let (artist, title) = crate::training::identify(path, &path.strip_prefix(&root).map(|relative| relative.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| stem.to_string()));
            let seconds = crate::audio_pcm::duration_seconds(path).unwrap_or(0.0);
            let mut failed = Vec::new();
            let found = sources.find(&Song { artist: artist.clone(), title: title.clone(), seconds }, &mut failed).await;
            let label = match &found {
                Some(Found::Lyrics { source, plain, .. }) => format!("{source} ({} lines)", plain.lines().count()),
                Some(Found::Instrumental { source }) => format!("{source}: instrumental"),
                None => "not found".into(),
            };
            *tally.entry(label.split(' ').next().unwrap_or_default().trim_end_matches(':').to_string()).or_default() += 1;
            println!("{artist} - {title} [{seconds:.0}s]: {label}{}", if failed.is_empty() { String::new() } else { format!(" | {}", failed.join("; ")) });
        }
        println!("{tally:?}");
    }

    /// Asks every source on its own for one song and prints what each said:
    /// LYRICS_DB_PROBE="artist|title|seconds". Network; run by hand.
    #[tokio::test]
    #[ignore]
    async fn probe_every_source() {
        let Ok(probe) = std::env::var("LYRICS_DB_PROBE") else { return };
        let parts: Vec<&str> = probe.split('|').collect();
        let song = Song { artist: parts[0].into(), title: parts[1].into(), seconds: parts.get(2).and_then(|value| value.parse().ok()).unwrap_or(0.0) };
        let sources = Sources::new().expect("client");
        let keyword = format!("{} {}", song.artist, song.title);
        let request = serde_json::json!({"req_1": {"method": "DoSearchForQQMusicDesktop", "module": "music.search.SearchCgiService", "param": {"num_per_page": 3, "page_num": 1, "query": keyword, "search_type": 0}}});
        match sources.http.post("https://u.y.qq.com/cgi-bin/musicu.fcg").header("User-Agent", BROWSER).header("Referer", "https://y.qq.com/").json(&request).send().await {
            Ok(response) => {
                let body: Value = response.json().await.unwrap_or_default();
                for entry in body["req_1"]["data"]["body"]["song"]["list"].as_array().into_iter().flatten() {
                    println!("QQ candidate: {} - {} [{}]", entry["singer"], entry["name"], entry["interval"]);
                }
                println!("QQ code: {}", body["req_1"]["code"]);
            }
            Err(error) => println!("QQ raw error: {error:#}"),
        }
        for (name, outcome) in [
            ("LRCLIB", sources.lrclib(&song).await),
            ("QQ Music", sources.qq(&song).await),
            ("Kugou", sources.kugou(&song).await),
        ] {
            let said = match outcome {
                Ok(Some(Found::Lyrics { plain, .. })) => format!("{} lines: {}", plain.lines().count(), plain.lines().next().unwrap_or_default()),
                Ok(Some(Found::Instrumental { .. })) => "instrumental".into(),
                Ok(None) => "nothing matching".into(),
                Err(error) => format!("error: {error:#}"),
            };
            println!("{name}: {said}");
        }
    }
}
