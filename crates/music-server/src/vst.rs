//! VST3 effect chains, run by HOT-Step's `vst-host` beside the studio.
//!
//! The host is a separate process on purpose: a plugin that crashes takes the
//! host down, not the studio. It finds the plugins installed in the system's
//! VST3 folders, opens a plugin's own window to set it up - the settings are
//! kept in a state file per chain slot - and runs a track through a chain of
//! plugins offline, at the track's own rate.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use audio_post::Stereo;
use serde::{Deserialize, Serialize};

/// A plugin the host found installed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VstPlugin {
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    pub path: String,
    #[serde(default)]
    pub uid: String,
}

/// One plugin of a chain, with the settings its window last saved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VstSlot {
    pub path: String,
    pub name: String,
    /// The state file the plugin's window writes, when it was set up.
    #[serde(default)]
    pub state_id: Option<String>,
    #[serde(default = "enabled")]
    pub enabled: bool,
}

fn enabled() -> bool {
    true
}

pub struct VstHost {
    exe: PathBuf,
    root: PathBuf,
}

/// How long a scan may take before a plugin is taken to have hung it.
const SCAN_LIMIT: Duration = Duration::from_secs(180);

/// How long a chain may take over one track before a plugin is taken to have hung.
const PROCESS_LIMIT: Duration = Duration::from_secs(30 * 60);

impl VstHost {
    /// The host shipped beside the studio, or the one `MUSIC_VST_HOST_BIN`
    /// names in a developer build.
    pub fn locate(data_root: &Path) -> Option<Self> {
        let exe = std::env::var_os("MUSIC_VST_HOST_BIN").map(PathBuf::from).or_else(|| {
            let beside = std::env::current_exe().ok()?.parent()?.join("resources").join("vst-host").join("vst-host.exe");
            beside.is_file().then_some(beside)
        })?;
        exe.is_file().then(|| Self { exe, root: data_root.join("processing").join("vst") })
    }

    fn plugins_file(&self) -> PathBuf {
        self.root.join("plugins.json")
    }

    fn state_file(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            bail!("not a plugin state: {id}");
        }
        Ok(self.root.join("states").join(format!("{id}.vststate")))
    }

    /// The plugins of the last scan, none before the first.
    pub fn plugins(&self) -> Option<Vec<VstPlugin>> {
        serde_json::from_slice(&std::fs::read(self.plugins_file()).ok()?).ok()
    }

    /// Looks through the system's VST3 folders and remembers what it found.
    pub fn scan(&self) -> Result<Vec<VstPlugin>> {
        let mut command = Command::new(&self.exe);
        command.arg("--scan").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        hide_window(&mut command);
        let child = command.spawn().with_context(|| format!("start {}", self.exe.display()))?;
        let output = wait_with_limit(child, SCAN_LIMIT).context("the plugin scan did not finish; a plugin may be hanging it")?;
        if !output.status.success() {
            bail!("the plugin scan stopped ({}): {}", output.status, last_line(&output.stderr));
        }
        let plugins: Vec<VstPlugin> = serde_json::from_slice(&output.stdout).context("the plugin scan printed no list")?;
        std::fs::create_dir_all(&self.root)?;
        std::fs::write(self.plugins_file(), serde_json::to_vec_pretty(&plugins)?)?;
        Ok(plugins)
    }

    /// Opens a plugin's own window; closing it saves the settings into the
    /// slot's state file. Returns the state id to keep with the slot.
    pub fn open_editor(&self, path: &str, state_id: Option<String>) -> Result<String> {
        if !self.plugins().unwrap_or_default().iter().any(|plugin| plugin.path == path) {
            bail!("{path} is not one of the plugins the scan found");
        }
        let id = state_id.unwrap_or_else(|| uuid::Uuid::now_v7().simple().to_string());
        let state = self.state_file(&id)?;
        std::fs::create_dir_all(state.parent().context("state folder")?)?;
        Command::new(&self.exe)
            .arg("--gui")
            .arg("--plugin")
            .arg(path)
            .arg("--state")
            .arg(&state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("start {}", self.exe.display()))?;
        Ok(id)
    }

    /// Runs a track through the enabled plugins of a chain, in order.
    pub fn process(&self, audio: &Stereo, chain: &[VstSlot], work: &Path) -> Result<Stereo> {
        // only what the scan found in the system folders is loaded: a path from
        // a request could name any DLL
        let known = self.plugins().unwrap_or_default();
        let plugins: Vec<serde_json::Value> = chain
            .iter()
            .filter(|slot| slot.enabled)
            .map(|slot| -> Result<serde_json::Value> {
                if !known.iter().any(|plugin| plugin.path == slot.path) {
                    bail!("{} is not one of the plugins the scan found", slot.path);
                }
                let state = match &slot.state_id {
                    Some(id) => {
                        let file = self.state_file(id)?;
                        if !file.is_file() {
                            bail!("the settings of {} are not saved yet: close its window first", slot.name);
                        }
                        Some(file.display().to_string())
                    }
                    None => None,
                };
                // no state key at all for a plugin never set up: the host takes a
                // missing state as the plugin's defaults
                let mut entry = serde_json::json!({ "path": slot.path, "enabled": true });
                if let Some(state) = state {
                    entry["state"] = serde_json::Value::String(state);
                }
                Ok(entry)
            })
            .collect::<Result<_>>()?;
        if plugins.is_empty() {
            bail!("the VST chain has no plugin switched on");
        }
        std::fs::create_dir_all(work)?;
        let token = uuid::Uuid::now_v7().simple().to_string();
        let input = work.join(format!("vst-{token}-in.wav"));
        let output = work.join(format!("vst-{token}-out.wav"));
        let chain_file = work.join(format!("vst-{token}.json"));
        let outcome = (|| -> Result<Stereo> {
            crate::audio_pcm::write_wav_f32(&input, audio)?;
            std::fs::write(&chain_file, serde_json::to_vec(&serde_json::json!({ "plugins": plugins }))?)?;
            let mut command = Command::new(&self.exe);
            command
                .arg("--process-chain")
                .arg("--chain")
                .arg(&chain_file)
                .arg("--input")
                .arg(&input)
                .arg("--output")
                .arg(&output)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            hide_window(&mut command);
            let child = command.spawn().with_context(|| format!("start {}", self.exe.display()))?;
            let result = wait_with_limit(child, PROCESS_LIMIT).context("the VST chain did not finish; a plugin may be hanging it")?;
            if !result.status.success() || !output.is_file() {
                bail!("the VST chain stopped ({}): {}", result.status, last_line(&result.stderr));
            }
            let processed = crate::audio_pcm::decode_stereo(&output)?;
            if processed.rate == audio.rate { Ok(processed) } else { processed.resampled(audio.rate) }
        })();
        for file in [&input, &output, &chain_file] {
            let _ = std::fs::remove_file(file);
        }
        outcome
    }
}

fn last_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).lines().rev().find(|line| !line.trim().is_empty()).unwrap_or_default().trim().to_string()
}

fn wait_with_limit(mut child: std::process::Child, limit: Duration) -> Result<std::process::Output> {
    use std::io::Read;
    // read both pipes while waiting: a long list would otherwise fill the pipe
    // and leave the host blocked on a write nobody reads
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = drain(child.stdout.take().map(|pipe| Box::new(pipe) as Box<dyn Read + Send>));
    let stderr = drain(child.stderr.take().map(|pipe| Box::new(pipe) as Box<dyn Read + Send>));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            bail!("timed out after {} s", limit.as_secs());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    Ok(std::process::Output { status, stdout: stdout.join().unwrap_or_default(), stderr: stderr.join().unwrap_or_default() })
}

#[cfg(windows)]
fn hide_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_window(_: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(label: &str, plugins: &[&str]) -> VstHost {
        let root = std::env::temp_dir().join(format!("vst-test-{label}-{}", uuid::Uuid::now_v7().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let found: Vec<VstPlugin> = plugins.iter().map(|path| VstPlugin { name: "p".into(), vendor: String::new(), path: (*path).into(), uid: String::new() }).collect();
        std::fs::write(root.join("plugins.json"), serde_json::to_vec(&found).unwrap()).unwrap();
        VstHost { exe: root.join("missing-host.exe"), root }
    }

    fn slot(path: &str, state_id: Option<&str>) -> VstSlot {
        VstSlot { path: path.into(), name: "p".into(), state_id: state_id.map(Into::into), enabled: true }
    }

    #[test]
    fn a_chain_loads_only_plugins_the_scan_found() {
        let host = host("unknown", &[r"C:\Program Files\Common Files\VST3\Known.vst3"]);
        let audio = Stereo { left: vec![0.0; 8], right: vec![0.0; 8], rate: 44_100 };
        let error = host.process(&audio, &[slot(r"\\elsewhere\share\x.vst3", None)], &host.root.join("work")).unwrap_err();
        assert!(error.to_string().contains("not one of the plugins"), "{error}");
    }

    #[test]
    fn a_plugin_whose_window_has_not_saved_is_refused_rather_than_run_on_defaults() {
        let path = r"C:\Program Files\Common Files\VST3\Known.vst3";
        let host = host("unsaved", &[path]);
        let audio = Stereo { left: vec![0.0; 8], right: vec![0.0; 8], rate: 44_100 };
        let error = host.process(&audio, &[slot(path, Some("0190abcd"))], &host.root.join("work")).unwrap_err();
        assert!(error.to_string().contains("not saved yet"), "{error}");
    }
}
