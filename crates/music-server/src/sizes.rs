//! What a downloadable file actually weighs.
//!
//! Sizes used to be compiled into the studio: read once with a HEAD request
//! while the asset was being added, written into the catalogue as a constant,
//! and then trusted for ever. Two things are wrong with that, and both bit.
//!
//! The file belongs to somebody else. Re-quantising a GGUF and re-uploading it
//! changes its size by a few hundred bytes of metadata, which is a normal thing
//! for the people who publish these models to do and none of our business. The
//! studio, meanwhile, treated the difference as a corrupt download and refused
//! the file for ever - a break that no retry could clear and only a new release
//! of the studio could fix.
//!
//! And the studio cannot know the number anyway. It is whatever the server says
//! it is at the moment of asking, so that is where it is asked.
//!
//! What remains in the catalogue is a figure to show before anything has been
//! fetched, so a panel can say "about 8 GB" without a network round trip. The
//! moment the real one is known - from a HEAD request here, or from the
//! download itself - it replaces it.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn table() -> &'static Mutex<HashMap<String, u64>> {
    static KNOWN: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    KNOWN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The real size, if anyone has asked the server yet.
pub fn known(url: &str) -> Option<u64> {
    table().lock().ok()?.get(url).copied()
}

/// The real size if it is known, and the catalogue's guess until it is.
pub fn or_listed(url: &str, listed: u64) -> u64 {
    known(url).unwrap_or(listed)
}

/// Records what a server said, whether it said it during a HEAD request or
/// while serving the file itself.
pub fn learn(url: &str, bytes: u64) {
    if bytes == 0 {
        return;
    }
    if let Ok(mut known) = table().lock() {
        known.insert(url.to_string(), bytes);
    }
}

/// Asks for the sizes nobody has asked for yet, in the background.
///
/// The panel that wants them is drawing now and will poll again in a second;
/// making it wait on a dozen HEAD requests would trade a wrong number for a
/// slow one. Failures are silent on purpose - a machine with no network still
/// has a panel to draw, and the listed figure is what it draws.
pub fn ask(http: &reqwest::Client, urls: Vec<String>) {
    // Asked once, whatever the answer. The first version of this only
    // remembered successes, and the panel polls its status every second: two
    // dozen URLs, asked again and again for as long as a page was open, until
    // Hugging Face answered 429 to everything - including the download the user
    // was waiting for. A size nobody could fetch is not worth one request per
    // second, let alone twenty.
    let wanted: Vec<String> = urls
        .into_iter()
        .filter(|url| !url.contains("example.invalid") && claim(url))
        .collect();
    if wanted.is_empty() {
        return;
    }
    let http = http.clone();
    tokio::spawn(async move {
        for url in wanted {
            // One at a time, and not in a hurry. This is a caption on a panel,
            // not the download; it must never compete with one.
            if let Ok(response) = http.head(&url).send().await {
                if response.status().is_success() {
                    if let Some(length) = response.content_length() {
                        learn(&url, length);
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    });
}

/// Takes responsibility for asking about a URL, once per run of the studio.
fn claim(url: &str) -> bool {
    static ASKED: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let asked = ASKED.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    match asked.lock() {
        Ok(mut asked) => known(url).is_none() && asked.insert(url.to_string()),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_learned_size_wins_over_the_listed_one() {
        // The listed figure is what the studio was built with; the learned one
        // is what the server is serving today. Three of the assistant's models
        // differed by 1440 bytes and could not be installed at all.
        let url = "https://example.test/gemma.gguf";
        assert_eq!(or_listed(url, 8_413_574_560), 8_413_574_560);
        learn(url, 8_413_576_000);
        assert_eq!(or_listed(url, 8_413_574_560), 8_413_576_000);
    }

    #[test]
    fn nothing_is_learned_from_a_server_that_said_nothing() {
        let url = "https://example.test/empty.bin";
        learn(url, 0);
        assert_eq!(known(url), None);
    }
}
