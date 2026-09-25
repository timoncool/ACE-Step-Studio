//! Who this studio is: its name, folders, repository, port and engines. Every
//! studio built from the shared code differs only by its `studio.json`, so the
//! code reads these values instead of spelling them.

use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Studio {
    /// The product name: window title, album tag, data folder on Windows.
    pub name: String,
    /// The lower-case name: data folder on Linux and macOS, temp folders.
    pub slug: String,
    /// `owner/name` of the studio's GitHub repository.
    pub repo: String,
    /// The artist tag of every track the studio makes: the engine, by name.
    pub artist: String,
    /// The loopback port of the studio's service.
    pub port: u16,
    /// The music engines this studio runs, by engine id, the first the default.
    pub engines: Vec<String>,
}

impl Studio {
    pub fn repo_url(&self) -> String {
        format!("https://github.com/{}", self.repo)
    }

    /// The User-Agent every outgoing request of the studio carries.
    pub fn user_agent(&self, version: &str) -> String {
        format!("{}/{version} ({})", self.name.replace(' ', "-"), self.repo_url())
    }

    pub fn default_engine(&self) -> &str {
        self.engines.first().map(String::as_str).unwrap_or_default()
    }

    pub fn runs(&self, engine_id: &str) -> bool {
        self.engines.iter().any(|engine| engine == engine_id)
    }
}

const STUDIO_JSON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../studio.json"));

pub fn studio() -> &'static Studio {
    static STUDIO: OnceLock<Studio> = OnceLock::new();
    STUDIO.get_or_init(|| serde_json::from_str(STUDIO_JSON).expect("studio.json at the repository root is not a valid studio"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repository_studio_json_is_complete() {
        let studio = studio();
        assert!(!studio.name.is_empty() && !studio.slug.is_empty());
        assert!(studio.repo.contains('/'));
        assert!(studio.port > 0);
        assert!(!studio.engines.is_empty());
    }
}
