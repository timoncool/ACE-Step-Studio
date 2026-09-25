//! The downloadable half of the optional writing assistant.
//!
//! Music3 needs no language model, so nothing here is required and nothing is
//! fetched on its own: the user picks a model, sees its size, and starts the
//! download. Once a model and the llama.cpp runtime are on disk, Studio runs
//! `llama-server` as a sidecar and talks to it over the same OpenAI-compatible
//! API it would use for a server the user runs themselves.
//!
//! The sizes below are for showing a weight before anything has been fetched.
//! They are not a rule: what a file weighs is whatever its server says today,
//! and that is what the download is checked against - see `sizes`. Treating the
//! compiled-in figure as the truth made three of these models impossible to
//! install after their publisher re-uploaded them.

use std::fs;
use std::io;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// What a downloadable asset is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// A GGUF text model.
    Model,
    /// The llama.cpp binaries, delivered as a zip.
    Runtime,
}

#[derive(Debug, Clone, Copy)]
pub struct Asset {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: AssetKind,
    pub url: &'static str,
    /// Where the file lands, relative to the assistant root.
    pub relative_path: &'static str,
    /// About what it weighs, for showing a size before anything is fetched.
    /// The real one comes from the server; see `sizes`.
    pub bytes: u64,
    /// Extracted into this runtime sub-directory when set. The CUDA build and
    /// the CPU fallback stay apart, and the CUDA libraries land next to the
    /// binary that links against them.
    pub unzip_into: Option<&'static str>,
    /// A file name prefix that proves this asset is unpacked. Without it the
    /// CUDA libraries would look installed as soon as the CUDA binaries were,
    /// because both land in the same directory.
    pub marker: &'static str,
    /// Roughly how much VRAM the model wants; informational only.
    pub vram_gb: Option<u32>,
    pub note: &'static str,
}

/// llama.cpp is pinned to one build so a working setup keeps working.
const LLAMA_BUILD: &str = "b11146";

pub const ASSETS: &[Asset] = &[
    Asset {
        id: "gemma-4-e4b-q4_0",
        label: "Gemma 4 E4B IT (QAT q4_0)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/google/gemma-4-E4B-it-qat-q4_0-gguf/resolve/main/gemma-4-E4B_q4_0-it.gguf",
        relative_path: "models/gemma-4-E4B_q4_0-it.gguf",
        bytes: 5_154_941_280,
        unzip_into: None,
        marker: "",
        vram_gb: Some(6),
        note: "The lighter of the two: quantisation-aware training keeps it usable at q4_0.",
    },
    Asset {
        id: "gemma-4-12b-q4_0",
        label: "Gemma 4 12B IT (QAT q4_0)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/google/gemma-4-12b-it-qat-q4_0-gguf/resolve/main/gemma-4-12b-it-qat-q4_0.gguf",
        relative_path: "models/gemma-4-12b-it-qat-q4_0.gguf",
        bytes: 6_975_879_296,
        unzip_into: None,
        marker: "",
        vram_gb: Some(10),
        note: "Writes noticeably better captions; wants a card that is not already full of Music3.",
    },
    Asset {
        id: "gemma-4-12b-q5_k_m",
        label: "Gemma 4 12B IT (Q5_K_M)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/main/gemma-4-12b-it-Q5_K_M.gguf",
        relative_path: "models/gemma-4-12b-it-Q5_K_M.gguf",
        bytes: 8_413_576_000,
        unzip_into: None,
        marker: "",
        vram_gb: Some(11),
        note: "More accurate than q4_0, and it wants a card with room to spare.",
    },
    Asset {
        id: "gemma-4-12b-q6_k",
        label: "Gemma 4 12B IT (Q6_K)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/main/gemma-4-12b-it-Q6_K.gguf",
        relative_path: "models/gemma-4-12b-it-Q6_K.gguf",
        bytes: 9_786_022_720,
        unzip_into: None,
        marker: "",
        vram_gb: Some(12),
        note: "Closer again to the full model.",
    },
    Asset {
        id: "gemma-4-12b-q8_0",
        label: "Gemma 4 12B IT (Q8_0)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/main/gemma-4-12b-it-Q8_0.gguf",
        relative_path: "models/gemma-4-12b-it-Q8_0.gguf",
        bytes: 12_669_647_680,
        unzip_into: None,
        marker: "",
        vram_gb: Some(15),
        note: "As close to the original as a quantisation gets. For a card that has nothing else to do.",
    },
    Asset {
        id: "llama-cuda",
        label: "llama.cpp runtime (CUDA 13.4)",
        kind: AssetKind::Runtime,
        url: "https://github.com/ggml-org/llama.cpp/releases/download/b11146/llama-b11146-bin-win-cuda-13.4-x64.zip",
        relative_path: "runtime/llama-cuda.zip",
        bytes: 149_758_833,
        unzip_into: Some("cuda"),
        marker: "llama-server",
        vram_gb: None,
        note: "Needs the CUDA runtime companion below.",
    },
    Asset {
        id: "llama-cuda-runtime",
        label: "CUDA runtime for llama.cpp",
        kind: AssetKind::Runtime,
        url: "https://github.com/ggml-org/llama.cpp/releases/download/b11146/cudart-llama-bin-win-cuda-13.4-x64.zip",
        relative_path: "runtime/cudart.zip",
        bytes: 423_535_356,
        unzip_into: Some("cuda"),
        marker: "cudart64",
        vram_gb: None,
        note: "The CUDA libraries llama.cpp links against.",
    },
    Asset {
        id: "llama-cpu",
        label: "llama.cpp runtime (CPU)",
        kind: AssetKind::Runtime,
        url: "https://github.com/ggml-org/llama.cpp/releases/download/b11146/llama-b11146-bin-win-cpu-x64.zip",
        relative_path: "runtime/llama-cpu.zip",
        bytes: 18_560_055,
        unzip_into: Some("cpu"),
        marker: "llama-server",
        vram_gb: None,
        note: "For machines without an NVIDIA card. Slow, but it works.",
    },
];

pub fn asset(id: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|asset| asset.id == id)
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: AssetKind,
    pub bytes: u64,
    pub vram_gb: Option<u32>,
    pub note: &'static str,
    pub installed: bool,
    /// Bytes already on disk, so a resumed download reports honestly.
    pub downloaded_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeStatus {
    pub root: String,
    /// True when llama-server and at least one model are present.
    pub ready: bool,
    pub server_path: Option<String>,
    pub installed_models: Vec<String>,
    pub running_model: Option<String>,
    pub base_url: Option<String>,
    pub assets: Vec<AssetStatus>,
    pub active_download: Option<DownloadProgress>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub asset_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub done: bool,
    pub error: Option<String>,
}

/// Owns the downloads and the sidecar process.
pub struct AssistantRuntime {
    root: PathBuf,
    http: reqwest::Client,
    state: Arc<Mutex<RuntimeState>>,
    /// Raised to stop the download. What arrived stays on disk, so this is a
    /// pause as much as a cancel.
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Default)]
struct RuntimeState {
    download: Option<DownloadProgress>,
    server: Option<RunningServer>,
}

struct RunningServer {
    child: Child,
    model_id: String,
    base_url: String,
}

impl RunningServer {
    /// A sidecar that has exited is not a running server. Without this the
    /// status kept claiming a model was loaded after llama-server had died,
    /// and the next request failed with a bare connection error.
    fn is_alive(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl AssistantRuntime {
    pub fn new(data_root: &Path) -> Self {
        Self {
            root: data_root.join("assistant"),
            http: crate::sizes::client(),
            state: Arc::new(Mutex::new(RuntimeState::default())),
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn path_of(&self, asset: &Asset) -> PathBuf {
        self.root.join(asset.relative_path)
    }

    pub fn log_path(&self) -> PathBuf {
        self.root.join("llama-server.log")
    }

    /// The tail of the sidecar log, for error messages that would otherwise
    /// say only "connection refused".
    pub fn log_tail(&self) -> String {
        let Ok(text) = fs::read_to_string(self.log_path()) else {
            return String::new();
        };
        let lines: Vec<&str> = text.lines().rev().take(12).collect();
        lines.into_iter().rev().collect::<Vec<_>>().join("
")
    }

    /// The extracted llama-server, wherever the zip happened to put it.
    pub fn server_binary(&self) -> Option<PathBuf> {
        self.server_binary_for(None)
    }

    /// The binary for a chosen device, or whichever is installed.
    ///
    /// The card build and the processor build sit in their own directories, so
    /// this is also what decides which of the two a download has to fetch: the
    /// choice is the setting, not a guess made after the fact.
    pub fn server_binary_for(&self, device: Option<&str>) -> Option<PathBuf> {
        let runtime = self.root.join("runtime");
        let name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
        let order: &[&str] = match device {
            Some("cuda") => &["cuda"],
            Some("cpu") => &["cpu"],
            _ => &["cuda", "cpu"],
        };
        for flavour in order {
            let candidate = runtime.join(flavour).join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        let direct = runtime.join(name);
        direct.is_file().then_some(direct)
    }

    pub fn installed_models(&self) -> Vec<&'static Asset> {
        ASSETS
            .iter()
            .filter(|asset| asset.kind == AssetKind::Model && self.is_installed(asset))
            .collect()
    }

    fn is_installed(&self, asset: &Asset) -> bool {
        if let Some(flavour) = asset.unzip_into {
            // A runtime counts as installed once its own directory has content:
            // the CUDA libraries carry no llama-server of their own.
            let directory = self.root.join("runtime").join(flavour);
            let marker = asset.marker;
            return fs::read_dir(&directory)
                .map(|entries| {
                    entries.flatten().any(|entry| {
                        entry.file_name().to_string_lossy().to_ascii_lowercase().starts_with(marker)
                    })
                })
                .unwrap_or(false);
        }
        // A file is installed when it is there. It gets there by one route
        // only - downloaded into `.part`, checked against what the server said
        // it was sending, and renamed - so its presence is the proof, and a
        // stale figure in the catalogue can no longer declare a perfectly good
        // model missing.
        fs::metadata(self.path_of(asset)).map(|meta| meta.len() > 0).unwrap_or(false)
    }

    fn partial_bytes(&self, asset: &Asset) -> u64 {
        let part = self.path_of(asset).with_extension("part");
        fs::metadata(part).map(|meta| meta.len()).unwrap_or(0)
    }

    pub async fn status(&self) -> RuntimeStatus {
        // What these weigh is the servers' business, not a constant compiled in
        // here; the answers arrive in time for the next poll.
        crate::sizes::ask(&self.http, ASSETS.iter().map(|asset| asset.url.to_string()).collect());
        let mut state = self.state.lock().await;
        if state.server.as_mut().is_some_and(|server| !server.is_alive()) {
            state.server = None;
        }
        let models = self.installed_models();
        let server_path = self.server_binary();
        RuntimeStatus {
            root: self.root.display().to_string(),
            ready: server_path.is_some() && !models.is_empty(),
            server_path: server_path.map(|path| path.display().to_string()),
            installed_models: models.iter().map(|asset| asset.id.to_string()).collect(),
            running_model: state.server.as_ref().map(|server| server.model_id.clone()),
            base_url: state.server.as_ref().map(|server| server.base_url.clone()),
            assets: ASSETS
                .iter()
                .map(|asset| AssetStatus {
                    id: asset.id,
                    label: asset.label,
                    kind: asset.kind,
                    bytes: crate::sizes::or_listed(asset.url, asset.bytes),
                    vram_gb: asset.vram_gb,
                    note: asset.note,
                    installed: self.is_installed(asset),
                    downloaded_bytes: if self.is_installed(asset) { crate::sizes::or_listed(asset.url, asset.bytes) } else { self.partial_bytes(asset) },
                })
                .collect(),
            active_download: state.download.clone(),
        }
    }

    /// Installs a whole set - the runtime, its libraries, the model - as one
    /// download, and waits for it.
    ///
    /// Asking for them one at a time did not work and could not: `install`
    /// returns as soon as it has spawned, so the second call found the first
    /// still running and was refused. The caller's loop then stopped at its
    /// first file. That is why the runtime appeared and the model never did -
    /// the panel reported "installed" for the group and "not installed" for the
    /// set in the same breath, because both were true.
    /// Deletes an installed asset and returns the disk it freed.
    ///
    /// The runtime is a directory of unpacked binaries, a model is one file,
    /// and a half-finished download is a `.part` next to it - all three go.
    pub fn remove(&self, id: &str) -> Result<u64> {
        let asset = asset(id).ok_or_else(|| anyhow!("unknown assistant asset: {id}"))?;
        let mut freed = 0u64;
        if let Some(flavour) = asset.unzip_into {
            let directory = self.root.join("runtime").join(flavour);
            if let Ok(entries) = fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    if let Ok(meta) = entry.metadata() {
                        if meta.is_file() {
                            freed += meta.len();
                        }
                    }
                }
            }
            match fs::remove_dir_all(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).with_context(|| format!("remove {}", directory.display())),
            }
        }
        for path in [self.path_of(asset), self.path_of(asset).with_extension("part")] {
            if let Ok(meta) = fs::metadata(&path) {
                freed += meta.len();
                fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
            }
            let manifest = PathBuf::from(format!("{}.done", path.display()));
            fs::remove_file(manifest).ok();
        }
        Ok(freed)
    }

    /// Stops whatever is downloading.
    pub fn cancel(&self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub async fn install_all(&self, ids: &[String]) -> Result<()> {
        let mut assets: Vec<&'static Asset> = Vec::new();
        for id in ids {
            assets.push(asset(id).ok_or_else(|| anyhow!("unknown assistant asset: {id}"))?);
        }
        let pending: Vec<&'static Asset> = assets.into_iter().filter(|asset| !self.is_installed(asset)).collect();
        if pending.is_empty() {
            return Ok(());
        }

        self.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        let total: u64 = pending.iter().map(|asset| asset.bytes).sum();
        {
            let mut state = self.state.lock().await;
            if state.download.as_ref().is_some_and(|progress| !progress.done && progress.error.is_none()) {
                bail!("another assistant download is already running");
            }
            state.download = Some(DownloadProgress {
                asset_id: if pending.len() == 1 { pending[0].id.to_string() } else { crate::downloads::SET.to_string() },
                downloaded_bytes: 0,
                total_bytes: total,
                done: false,
                error: None,
            });
        }

        // Sequential on purpose: the model is most of the set, and the two
        // runtime files are small enough that fetching them at the same time
        // buys nothing but a harder figure to report.
        let mut base = 0u64;
        let mut failure = None;
        for asset in &pending {
            let target = self.path_of(asset);
            let outcome = match download_asset(&self.http, asset, &target, &self.state, base, self.cancel.clone()).await {
                Ok(()) => match asset.unzip_into {
                    Some(flavour) => extract_zip(&target, &self.root.join("runtime").join(flavour)),
                    None => Ok(()),
                },
                Err(error) => Err(error),
            };
            if let Err(error) = outcome {
                failure = Some(error);
                break;
            }
            base += asset.bytes;
            if let Some(progress) = self.state.lock().await.download.as_mut() {
                progress.downloaded_bytes = base;
            }
        }

        let outcome = match failure {
            Some(_) if self.cancel.load(std::sync::atomic::Ordering::Relaxed) => Err(anyhow!("cancelled")),
            Some(error) => Err(error),
            None => Ok(()),
        };
        if let Some(progress) = self.state.lock().await.download.as_mut() {
            progress.done = true;
            progress.error = outcome.as_ref().err().map(|error| error.to_string());
            if outcome.is_ok() {
                progress.downloaded_bytes = total;
            }
        }
        outcome
    }

    /// Starts one download in the background. Only one runs at a time, and an
    /// interrupted file resumes from what is already on disk.
    pub async fn install(&self, id: &str) -> Result<()> {
        let asset = asset(id).ok_or_else(|| anyhow!("unknown assistant asset: {id}"))?;
        {
            let mut state = self.state.lock().await;
            if state.download.as_ref().is_some_and(|progress| !progress.done && progress.error.is_none()) {
                bail!("another assistant download is already running");
            }
            state.download = Some(DownloadProgress {
                asset_id: asset.id.to_string(),
                downloaded_bytes: self.partial_bytes(asset),
                total_bytes: asset.bytes,
                done: false,
                error: None,
            });
        }

        let target = self.path_of(asset);
        let http = self.http.clone();
        let state = self.state.clone();
        let root = self.root.clone();
        tokio::spawn(async move {
            let outcome = download_asset(&http, asset, &target, &state, 0, Arc::new(std::sync::atomic::AtomicBool::new(false))).await;
            let mut guard = state.lock().await;
            match outcome {
                Ok(()) => {
                    let extraction = match asset.unzip_into {
                        Some(flavour) => extract_zip(&target, &root.join("runtime").join(flavour)),
                        None => Ok(()),
                    };
                    if let Some(progress) = guard.download.as_mut() {
                        progress.done = true;
                        progress.error = extraction.err().map(|error| error.to_string());
                    }
                }
                Err(error) => {
                    if let Some(progress) = guard.download.as_mut() {
                        progress.done = true;
                        progress.error = Some(error.to_string());
                    }
                }
            }
        });
        Ok(())
    }

    /// Starts the sidecar on a GGUF that is already on this machine. Models
    /// downloaded by other tools are perfectly good here, so there is no reason
    /// to fetch a second copy of one.
    pub async fn start_path(&self, model_path: &Path, context_size: u32, reasoning: Option<&str>) -> Result<String> {
        if !model_path.is_file() {
            bail!("{} is not a file", model_path.display());
        }
        let mut magic = [0u8; 4];
        {
            use std::io::Read;
            let mut file = fs::File::open(model_path).with_context(|| format!("open {}", model_path.display()))?;
            file.read_exact(&mut magic).with_context(|| format!("read {}", model_path.display()))?;
        }
        if &magic != b"GGUF" {
            bail!("{} is not a GGUF file", model_path.display());
        }
        let label = model_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| model_path.display().to_string());
        self.spawn(model_path.to_path_buf(), label, context_size, reasoning).await
    }

    /// Starts the sidecar for one installed model and returns its base URL.
    pub async fn start(&self, model_id: &str, context_size: u32, reasoning: Option<&str>) -> Result<String> {
        let asset = asset(model_id).ok_or_else(|| anyhow!("unknown assistant model: {model_id}"))?;
        if asset.kind != AssetKind::Model {
            bail!("{model_id} is not a model");
        }
        if !self.is_installed(asset) {
            bail!("{} is not downloaded yet", asset.label);
        }
        self.spawn(self.path_of(asset), model_id.to_string(), context_size, reasoning).await
    }

    async fn spawn(&self, model_path: PathBuf, model_id: String, context_size: u32, reasoning: Option<&str>) -> Result<String> {
        let binary = self
            .server_binary()
            .ok_or_else(|| anyhow!("the llama.cpp runtime is not installed yet"))?;

        let mut state = self.state.lock().await;
        if let Some(server) = state.server.as_mut() {
            if server.model_id == model_id && server.is_alive() {
                return Ok(server.base_url.clone());
            }
        }
        state.server = None;

        let port = free_port()?;
        let mut command = Command::new(&binary);
        command
            .arg("--model")
            .arg(&model_path)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--ctx-size")
            .arg(context_size.to_string())
            .arg("--n-gpu-layers")
            .arg("99")
            // Without the model's own chat template llama-server answers with an
            // empty message for Gemma, which reads as "the assistant returned
            // nothing" downstream.
            .arg("--jinja")
            // Thinking lands in `reasoning_content`, which is where the reader
            // looks; without this some templates inline it into the answer.
            .arg("--reasoning-format")
            .arg("deepseek")
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        // llama-server explains its own failures - a model it cannot load, a
        // card it cannot fit on - and those explanations only exist on stderr.
        match fs::File::create(self.log_path()) {
            Ok(log) => { command.stderr(Stdio::from(log)); }
            Err(_) => { command.stderr(Stdio::null()); }
        }
        // The same setting the cloud path sends as `reasoning.effort`, in the
        // terms this server understands: whether to think at all, and for how
        // many tokens.
        match reasoning.map(str::trim).unwrap_or("") {
            "" | "off" | "none" => {
                command.arg("--reasoning").arg("off");
            }
            effort => {
                let budget = match effort {
                    "minimal" => "256",
                    "low" => "512",
                    "medium" => "1024",
                    "high" => "4096",
                    _ => "-1",
                };
                command.arg("--reasoning").arg("on").arg("--reasoning-budget").arg(budget);
            }
        }
        hide_console(&mut command);

        let child = command
            .spawn()
            .with_context(|| format!("start llama-server {}", binary.display()))?;
        music_core::process::adopt(&child);
        let base_url = format!("http://127.0.0.1:{port}/v1");
        let server = RunningServer { child, model_id: model_id.clone(), base_url: base_url.clone() };
        state.server = Some(server);
        drop(state);

        // Loading several gigabytes into VRAM is not instant.
        let health = format!("http://127.0.0.1:{port}/health");
        let deadline = Instant::now() + Duration::from_secs(300);
        while Instant::now() < deadline {
            if self.http.get(&health).timeout(Duration::from_secs(2)).send().await.is_ok_and(|response| response.status().is_success()) {
                return Ok(base_url);
            }
            {
                let mut state = self.state.lock().await;
                let Some(server) = state.server.as_mut() else {
                    bail!("the writing assistant was stopped while it was loading");
                };
                if server.child.try_wait().ok().flatten().is_some() {
                    state.server = None;
                    drop(state);
                    bail!("llama-server exited before it became ready:\n{}", self.log_tail());
                }
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        self.stop().await;
        bail!("llama-server did not become ready in time")
    }

    pub async fn stop(&self) {
        self.state.lock().await.server = None;
    }

    /// The base URL of the running sidecar, if any.
    pub async fn base_url(&self) -> Option<String> {
        let mut state = self.state.lock().await;
        if state.server.as_mut().is_some_and(|server| !server.is_alive()) {
            state.server = None;
        }
        state.server.as_ref().map(|server| server.base_url.clone())
    }
}

/// `base` is what the rest of the set has already fetched, so a set of three
/// files reports one rising figure instead of three that start again at nought.
///
/// The fetching itself is `chunked`: four connections over ranges, retried
/// piece by piece, resumable down to the last sixteen megabytes. A seven
/// gigabyte model over one connection was the studio waiting on a single TCP
/// stream for no reason.
async fn download_asset(
    http: &reqwest::Client,
    asset: &Asset,
    target: &Path,
    state: &Arc<Mutex<RuntimeState>>,
    base: u64,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    if fs::metadata(target).map(|meta| meta.len() > 0).unwrap_or(false) {
        return Ok(());
    }

    let plan = crate::chunked::probe(http, asset.url).await?;
    if plan.total > 0 {
        crate::sizes::learn(asset.url, plan.total);
    }

    let part = target.with_extension("part");
    let written = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let reporter = {
        let (written, state) = (written.clone(), state.clone());
        tokio::spawn(async move {
            loop {
                let value = written.load(std::sync::atomic::Ordering::Relaxed);
                if let Some(progress) = state.lock().await.download.as_mut() {
                    progress.downloaded_bytes = base + value;
                }
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        })
    };
    let outcome = crate::chunked::fetch(http, asset.url, &part, plan, written.clone(), cancel).await;
    reporter.abort();
    outcome?;
    if let Some(progress) = state.lock().await.download.as_mut() {
        progress.downloaded_bytes = base + written.load(std::sync::atomic::Ordering::Relaxed);
    }

    let size = fs::metadata(&part)?.len();
    if plan.total > 0 && size != plan.total {
        bail!("{} has {size} bytes, the server said {}", asset.label, plan.total);
    }
    fs::rename(&part, target).with_context(|| format!("publish {}", target.display()))?;
    Ok(())
}

/// Extracts a llama.cpp release zip. Entries are flattened into `destination`
/// because the releases nest their binaries one directory deep.
fn extract_zip(archive: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    let file = fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).with_context(|| format!("read {}", archive.display()))?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).context("read zip entry")?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry.enclosed_name().and_then(|path| path.file_name().map(|name| name.to_owned())) else {
            continue;
        };
        let target = destination.join(name);
        let mut out = fs::File::create(&target).with_context(|| format!("create {}", target.display()))?;
        io::copy(&mut entry, &mut out).with_context(|| format!("extract {}", target.display()))?;
    }
    fs::remove_file(archive).ok();
    Ok(())
}

fn free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("reserve a port for llama-server")?;
    Ok(listener.local_addr()?.port())
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_asset_has_a_size_and_a_distinct_id() {
        for entry in ASSETS {
            assert!(entry.bytes > 0, "{} has no size", entry.id);
            assert_eq!(ASSETS.iter().filter(|other| other.id == entry.id).count(), 1);
            assert!(entry.url.starts_with("https://"), "{} is not fetched over https", entry.id);
        }
        assert!(ASSETS.iter().any(|entry| entry.kind == AssetKind::Model));
        assert!(ASSETS.iter().any(|entry| entry.kind == AssetKind::Runtime));
    }

    #[test]
    fn the_runtime_is_pinned_to_one_llama_cpp_build() {
        for entry in ASSETS.iter().filter(|entry| entry.kind == AssetKind::Runtime) {
            assert!(entry.url.contains(LLAMA_BUILD), "{} is not pinned", entry.id);
            assert!(entry.unzip_into.is_some(), "{} has nowhere to extract to", entry.id);
            assert!(!entry.marker.is_empty(), "{} has no proof of extraction", entry.id);
        }
    }

    #[test]
    fn a_partially_downloaded_model_is_not_installed() {
        let temp = std::env::temp_dir().join(format!("assistant-runtime-{}", uuid::Uuid::now_v7()));
        let runtime = AssistantRuntime::new(&temp);
        let model = asset("gemma-4-e4b-q4_0").expect("catalog entry");
        let path = runtime.path_of(model);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path.with_extension("part"), b"not the whole model").unwrap();

        assert!(!runtime.is_installed(model));
        assert_eq!(runtime.partial_bytes(model), 19);
        fs::remove_dir_all(&temp).ok();
    }
}
