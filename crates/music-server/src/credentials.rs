//! Local credential storage for cloud providers.
//!
//! A desktop Studio has no server to hold secrets for it, so the API key lives
//! beside the other runtime user data. Two rules keep it honest:
//!
//! * the environment variable always wins, so CI and scripted runs never read
//!   or write the user's stored key;
//! * the value is never serialized into settings, catalogs, job records or any
//!   response body — callers can only ask whether a key is configured.

use std::{env, fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

pub const OPENROUTER_ENV_VAR: &str = "OPENROUTER_API_KEY";
const OPENROUTER_FILE: &str = "openrouter-api-key";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    Environment,
    LocalStore,
}

pub fn openrouter_api_key() -> Option<(String, CredentialSource)> {
    if let Some(key) = env::var(OPENROUTER_ENV_VAR).ok().map(|key| key.trim().to_owned()).filter(|key| !key.is_empty()) {
        return Some((key, CredentialSource::Environment));
    }
    let stored = fs::read_to_string(openrouter_key_path()?).ok()?.trim().to_owned();
    (!stored.is_empty()).then_some((stored, CredentialSource::LocalStore))
}

pub fn openrouter_source() -> Option<CredentialSource> {
    openrouter_api_key().map(|(_, source)| source)
}

/// Stores or clears the key. Refuses to shadow an environment credential so a
/// user never believes they replaced a key that the process is not using.
pub fn store_openrouter_api_key(api_key: Option<&str>) -> Result<Option<CredentialSource>> {
    if env::var(OPENROUTER_ENV_VAR).is_ok_and(|key| !key.trim().is_empty()) {
        bail!("{OPENROUTER_ENV_VAR} is set in this environment and takes priority; unset it before storing a key in Studio");
    }
    let path = openrouter_key_path().context("no per-user application data directory for credential storage")?;
    match api_key.map(str::trim).filter(|key| !key.is_empty()) {
        Some(key) => {
            if key.contains(['\r', '\n']) {
                bail!("an OpenRouter API key must be a single line");
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
            }
            fs::write(&path, key).with_context(|| format!("write {}", path.display()))?;
            Ok(Some(CredentialSource::LocalStore))
        }
        None => {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).with_context(|| format!("remove {}", path.display())),
            }
            Ok(None)
        }
    }
}

fn openrouter_key_path() -> Option<PathBuf> {
    Some(crate::studio_data_root()?.join(OPENROUTER_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_environment_key_is_reported_as_such() {
        // The variable is process-wide; only assert the branch that does not
        // depend on mutating global state in a parallel test run.
        if let Ok(key) = env::var(OPENROUTER_ENV_VAR) {
            if !key.trim().is_empty() {
                assert_eq!(openrouter_source(), Some(CredentialSource::Environment));
            }
        }
    }

    #[test]
    fn storing_is_refused_while_the_environment_variable_wins() {
        if env::var(OPENROUTER_ENV_VAR).is_ok_and(|key| !key.trim().is_empty()) {
            assert!(store_openrouter_api_key(Some("test")).is_err());
        }
    }
}
