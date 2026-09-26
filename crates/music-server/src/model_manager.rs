use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{io::AsyncWriteExt, sync::RwLock};

pub const ENGINE_ID: &str = "acestep-cpp";
/// Every file is pinned to a repository commit and verified by its SHA-256.
/// The official quantisations, XL included, come from Serveurperso; the XL
/// merges, the MXFP4 quantisations for Blackwell cards and the ScragVAE
/// decoder from scragnog.
const MAIN: (&str, &str) = ("Serveurperso/ACE-Step-1.5-GGUF", "666ac70204440867d8c01ba4b119cc79c95b370a");
const MERGES: (&str, &str) = ("scragnog/ace-step-1.5-gguf-merge-models", "c4750313bd892420f5902d346b805d686d963784");
const MXFP4: (&str, &str) = ("scragnog/Ace-Step-1.5-MXFP4-Quants", "05a04ec24513e44f21947a2d1ee8d1cae91d1095");
const SCRAGVAE: (&str, &str) = ("scragnog/Ace-Step-1.5-ScragVAE", "0547ba36ff72b94ca3db3fa9194a59899ca37e5c");
/// mdmachine's Regrind: an XL turbo DiT and VAE decoders retrained against the
/// harmonic hum of the stock weights. CC BY-NC-SA 4.0, not for commercial use.
const REGRIND: (&str, &str) = ("mdmachine/ACEStep-XL-Regrind-V1", "e9b48a00c0f6f59f1ed988774e673ac70816474b");
const REPOSITORY: &str = MAIN.0;
const REVISION: &str = MAIN.1;

/// The recommendation is a property of the machine, not of the catalog: a
/// 24 GB card must land on Full Native and a 12 GB card on Q8 Quality. The
/// Light set is only ever recommended in the low-VRAM tier.
fn recommended_profile() -> &'static str {
    crate::presets::recommended_local_profile()
}

#[derive(Clone)]
pub struct ModelManager {
    root: PathBuf,
    state_path: PathBuf,
    http: reqwest::Client,
    state: Arc<RwLock<PersistentState>>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Catalog {
    pub engine_id: &'static str,
    pub repository: &'static str,
    pub revision: &'static str,
    pub recommended_profile_id: &'static str,
    pub profiles: Vec<Profile>,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub id: &'static str,
    pub label: &'static str,
    pub backend: &'static str,
    pub installable: bool,
    pub recommended: bool,
    pub components: Vec<&'static str>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub id: &'static str,
    pub kind: &'static str,
    pub filename: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    /// Where this file comes from. The lighter quantisations are published by
    /// someone else, and a single hard-coded repository is what kept them out.
    pub repository: &'static str,
    pub revision: &'static str,
    /// The path inside the repository, when the file lives in a folder there;
    /// it is stored under its own name.
    #[serde(skip)]
    pub remote_path: &'static str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstallRequest {
    pub profile_id: Option<String>,
    #[serde(default)]
    pub component_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagerStatus {
    pub engine_id: &'static str,
    pub model_root: String,
    pub first_run: bool,
    pub ready: bool,
    pub download_pending: u64,
    pub recommended_profile_id: String,
    pub active: Option<DownloadJob>,
    pub components: Vec<ComponentStatus>,
    pub installed_components: Vec<String>,
    /// The five files the selected set actually resolves to. The panel used to
    /// print "profile default" beside every role, which says nothing about
    /// what is loaded.
    pub profile_files: Option<ProfileModelFiles>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentStatus {
    pub id: &'static str,
    pub installed: bool,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadJob {
    pub id: String,
    pub profile_id: Option<String>,
    pub component_ids: Vec<String>,
    pub status: DownloadStatus,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

/// The files a runnable set resolves to, by the engine's request field.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ProfileModelFiles {
    pub synth_model: String,
    pub lm_model: String,
    pub text_encoder: String,
    pub vae: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Downloading,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistentState {
    active: Option<DownloadJob>,
}

impl ModelManager {
    pub fn from_environment() -> Result<Self> {
        let root = env::var_os("STUDIO_MODELS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(default_model_root);
        validate_model_root(&root)?;
        let state_path = root.join(".studio-download-state.json");
        let mut state = fs::read_to_string(&state_path)
            .ok()
            .and_then(|body| serde_json::from_str(&body).ok())
            .unwrap_or_default();
        if recover_interrupted_download(&mut state) {
            persist_state_file(&state_path, &state)?;
        }
        Ok(Self {
            root,
            state_path,
            http: crate::sizes::client(),
            state: Arc::new(RwLock::new(state)),
            cancelled: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Where the weights live, for anything that needs to check a file exists.
    pub fn models_directory(&self) -> &Path {
        &self.root
    }

    pub fn catalog(&self) -> Catalog {
        Catalog {
            engine_id: ENGINE_ID,
            repository: REPOSITORY,
            revision: REVISION,
            recommended_profile_id: recommended_profile(),
            profiles: profiles(),
            components: components(),
        }
    }

    /// `target` is the set the user actually selected. Progress and readiness
    /// are reported against it, so a machine that deliberately runs the Light
    /// set is never told it is missing the hardware-recommended download.
    pub async fn status(&self, target: Option<InstallRequest>) -> ManagerStatus {
        let active = self.state.read().await.active.clone();
        let root = self.root.clone();
        // SHA-256 over a GGUF can take seconds. Keep it off the Tokio request
        // workers so setup polling and cancellation remain available.
        tokio::task::spawn_blocking(move || status_snapshot(root, active, target))
            .await
            .unwrap_or_else(|_| status_snapshot(PathBuf::from("."), None, None))
    }

    /// Deletes the files of the named components, freeing the disk they take.
    ///
    /// Downloading is undoable only if the user can also undo it. Ten gigabytes
    /// of weights with no way to remove them from inside the studio is how
    /// people end up hunting through their profile folder by hand.
    pub async fn remove(&self, component_ids: &[String]) -> Result<RemovalReport> {
        if self.state.read().await.active.as_ref().is_some_and(|job| matches!(job.status, DownloadStatus::Downloading)) {
            bail!("a model download is running; cancel it before removing files");
        }
        let catalog = components();
        let mut removed = Vec::new();
        let mut freed_bytes = 0u64;
        for id in component_ids {
            let component = catalog
                .iter()
                .find(|component| component.id == *id)
                .with_context(|| format!("unknown component '{id}'"))?;
            let path = self.root.join(&component.filename);
            let size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            match fs::remove_file(&path) {
                Ok(()) => {
                    freed_bytes += size;
                    removed.push(component.id.to_string());
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).with_context(|| format!("remove {}", path.display())),
            }
            // A half-finished download of the same component is just as much
            // disk as the finished one.
            let _ = fs::remove_file(self.root.join(format!("{}.part", component.filename)));
        }
        // A remembered download resumes on the next start. Deleting the files
        // without forgetting the job is how twenty-six gigabytes came back by
        // themselves after being removed.
        {
            let mut state = self.state.write().await;
            let resumes_removed = state
                .active
                .as_ref()
                .is_some_and(|job| job.component_ids.iter().any(|id| component_ids.contains(id)));
            if resumes_removed {
                state.active = None;
                let _ = persist_state_file(&self.state_path, &state);
            }
        }
        Ok(RemovalReport { removed, freed_bytes })
    }

    pub async fn install(&self, request: InstallRequest) -> Result<DownloadJob> {
        let mut selection = resolve_install(request)?;
        fs::create_dir_all(&self.root)
            .with_context(|| format!("create model root {}", self.root.display()))?;
        preflight_space(&self.root, &selection)?;
        // Progress starts from what is already published on disk. This uses the
        // cheap size check rather than a SHA-256 sweep: hashing a resumed 10 GB
        // set here would block this request — and every setup/status poll behind
        // it — for tens of seconds. Each component is still hash-verified in
        // `download_selection` before it is skipped or published.
        selection.already_present_bytes = selection
            .components
            .iter()
            .filter(|component| published_component(&self.root.join(component.filename), component))
            .map(|component| component.bytes)
            .sum();
        let mut state = self.state.write().await;
        if state.active.as_ref().is_some_and(|job| matches!(job.status, DownloadStatus::Downloading)) {
            bail!("a model download is already running");
        }
        self.cancelled.store(false, Ordering::SeqCst);
        let job = DownloadJob {
            id: uuid::Uuid::now_v7().to_string(),
            profile_id: selection.profile_id.clone(),
            component_ids: selection.components.iter().map(|component| component.id.into()).collect(),
            status: DownloadStatus::Downloading,
            downloaded_bytes: selection.already_present_bytes,
            total_bytes: selection.total_bytes,
            error: None,
        };
        state.active = Some(job.clone());
        self.persist_locked(&state)?;
        drop(state);
        let manager = self.clone();
        tokio::spawn(async move { manager.download_selection(selection).await });
        Ok(job)
    }

    pub async fn cancel(&self, target: Option<InstallRequest>) -> Result<ManagerStatus> {
        self.cancelled.store(true, Ordering::SeqCst);
        Ok(self.status(target).await)
    }

    pub async fn download_job(&self, id: &str) -> Option<DownloadJob> {
        self.state.read().await.active.as_ref().filter(|job| job.id == id).cloned()
    }

    pub fn installed_profile_files(&self, profile_id: &str) -> Result<ProfileModelFiles> {
        let selection = resolve_install(InstallRequest { profile_id: Some(profile_id.into()), component_ids: vec![] })?;
        self.installed_files_from_selection(selection, &format!("selected profile '{profile_id}'"))
    }

    /// Resolves an explicitly selected complete five-component set. This is
    /// deliberately component-id based rather than filename based: callers
    /// cannot persist or submit arbitrary paths to the native engine.
    pub fn installed_component_files(&self, component_ids: &[String]) -> Result<ProfileModelFiles> {
        let selection = resolve_install(InstallRequest { profile_id: None, component_ids: component_ids.to_vec() })?;
        self.installed_files_from_selection(selection, "selected custom component set")
    }

    fn installed_files_from_selection(&self, selection: ResolvedInstall, label: &str) -> Result<ProfileModelFiles> {
        for component in &selection.components {
            let path = self.root.join(component.filename);
            if !published_component(&path, component) {
                bail!("{label} is incomplete: missing or truncated {}", component.filename);
            }
        }
        Ok(profile_files_from_components(&selection.components))
    }

    async fn download_selection(&self, selection: ResolvedInstall) {
        let result = async {
            for component in &selection.components {
                if self.cancelled.load(Ordering::SeqCst) {
                    bail!("cancelled");
                }
                if verified_file_async(self.root.join(component.filename), component.clone()).await? {
                    self.set_published_progress(&selection).await?;
                    continue;
                }
                self.download_component(component).await?;
                // Streaming only counts the bytes that crossed the network. A
                // component resumed from a complete `.part`, or already present
                // from an earlier attempt, would otherwise leave the bar short
                // of 100% on a successful install.
                self.set_published_progress(&selection).await?;
            }
            Ok(())
        }
        .await;
        let mut state = self.state.write().await;
        if let Some(job) = &mut state.active {
            match result {
                Ok(()) => job.status = DownloadStatus::Completed,
                Err(error) if self.cancelled.load(Ordering::SeqCst) => {
                    job.status = DownloadStatus::Cancelled;
                    job.error = Some(error.to_string());
                }
                Err(error) => {
                    job.status = DownloadStatus::Failed;
                    job.error = Some(error.to_string());
                }
            }
            let _ = self.persist_locked(&state);
        }
    }

    /// Fetches one component over four connections at once.
    ///
    /// Eleven gigabytes down a single TCP stream was the studio waiting on a
    /// fraction of the line for no reason. `chunked` cuts the file into pieces,
    /// retries them one by one, and remembers which of them landed - so an
    /// interrupted install resumes to within sixteen megabytes instead of
    /// starting the file again.
    ///
    /// Components are still fetched one after another, because four connections
    /// is the whole budget: Hugging Face's Xet storage drops them above that,
    /// and a dropped range leaves a hole in a file of exactly the right size.
    async fn download_component(&self, component: &Component) -> Result<()> {
        let target = self.root.join(component.filename);
        let part = part_path(&target);
        let url = format!(
            "https://huggingface.co/{}/resolve/{}/{}?download=true",
            component.repository, component.revision, component.remote_path
        );

        let plan = crate::chunked::probe(&self.http, &url).await?;
        let written = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

        // The job's bar counts bytes as they arrive; the pieces count their own.
        // This turns the second into the first without double counting, which is
        // what a shared counter written by four tasks would do.
        let reporter = {
            let (written, manager) = (written.clone(), self.clone());
            tokio::spawn(async move {
                let mut reported = 0u64;
                loop {
                    let value = written.load(Ordering::Relaxed);
                    if value > reported {
                        let _ = manager.add_progress(value - reported).await;
                        reported = value;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
            })
        };
        let outcome = crate::chunked::fetch(&self.http, &url, &part, plan, written, self.cancelled.clone()).await;
        reporter.abort();
        if self.cancelled.load(Ordering::SeqCst) {
            bail!("cancelled");
        }
        outcome?;

        self.publish_verified_part(component, &part, &target).await
    }

    /// Publishes a completed `.part` only after its SHA-256 matches the pinned
    /// Hugging Face LFS oid, so a truncated or corrupted transfer can never be
    /// presented to the engine as an installed component.
    async fn publish_verified_part(&self, component: &Component, part: &Path, target: &Path) -> Result<()> {
        // No size comparison here: the SHA-256 below is the check, and it is a
        // real one. A byte count compiled into the studio can only ever add a
        // way to reject a perfectly good file.
        if !verified_file_async(part.to_path_buf(), component.clone()).await? {
            fs::remove_file(part).ok();
            bail!("{} SHA-256 does not match the pinned Hugging Face LFS oid; the partial file was discarded so the next attempt starts clean", component.filename);
        }
        fs::rename(part, target).with_context(|| format!("publish {}", target.display()))?;
        Ok(())
    }

    /// Re-bases progress on what is actually published on disk.
    async fn set_published_progress(&self, selection: &ResolvedInstall) -> Result<()> {
        let published: u64 = selection
            .components
            .iter()
            .filter(|component| published_component(&self.root.join(component.filename), component))
            .map(|component| component.bytes)
            .sum();
        let mut state = self.state.write().await;
        if let Some(job) = &mut state.active {
            job.downloaded_bytes = published.min(job.total_bytes);
            self.persist_locked(&state)?;
        }
        Ok(())
    }

    async fn add_progress(&self, bytes: u64) -> Result<()> {
        let mut state = self.state.write().await;
        if let Some(job) = &mut state.active {
            job.downloaded_bytes = (job.downloaded_bytes + bytes).min(job.total_bytes);
            self.persist_locked(&state)?;
        }
        Ok(())
    }

    fn persist_locked(&self, state: &PersistentState) -> Result<()> {
        persist_state_file(&self.state_path, state)
    }
}

fn recover_interrupted_download(state: &mut PersistentState) -> bool {
    let Some(job) = &mut state.active else { return false; };
    if !matches!(job.status, DownloadStatus::Downloading) { return false; }
    job.status = DownloadStatus::Cancelled;
    job.error = Some("Download was interrupted by an application restart. Partial .part files were preserved and the next download resumes them with HTTP Range.".into());
    true
}

fn persist_state_file(path: &Path, state: &PersistentState) -> Result<()> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let temporary = part_path(path);
    fs::write(&temporary, serde_json::to_vec_pretty(state)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

/// Direct `music-server` launches use the same OS application-data location
/// as the Tauri shell. The environment variable remains the explicit override
/// for development and a user-managed model library.
fn default_model_root() -> PathBuf {
    // A portable copy sets the studio's data root to its own folder, and the
    // models are the largest thing the studio owns: resolving them separately
    // would put ten gigabytes on the system drive while everything else stayed
    // beside the executable.
    let studio = music_core::studio();
    crate::studio_data_root()
        .unwrap_or_else(|| env::temp_dir().join(&studio.slug))
        .join("models")
        .join(studio.default_engine())
}

/// What a removal actually did.
#[derive(Debug, Clone, Serialize)]
pub struct RemovalReport {
    pub removed: Vec<String>,
    pub freed_bytes: u64,
}

#[derive(Debug, Clone)]
struct ResolvedInstall {
    profile_id: Option<String>,
    components: Vec<Component>,
    total_bytes: u64,
    already_present_bytes: u64,
}

fn resolve_install(request: InstallRequest) -> Result<ResolvedInstall> {
    if request.profile_id.is_some() && !request.component_ids.is_empty() {
        bail!("select either a complete profile or an advanced component set, not both");
    }
    // An empty request is a mistake, not an instruction to download the default
    // set. Silently substituting a profile turned a field-name mismatch into
    // twenty-six gigabytes nobody asked for.
    if request.profile_id.is_none() && request.component_ids.is_empty() {
        bail!("nothing was selected to download: name a profile or the components");
    }
    let profiles = profiles();
    let profile = request.profile_id.unwrap_or_else(|| recommended_profile().into());
    let (profile_id, ids) = if request.component_ids.is_empty() {
        let selected = profiles.iter().find(|candidate| candidate.id == profile)
            .with_context(|| format!("unknown profile '{profile}'"))?;
        if !selected.installable || selected.backend != ENGINE_ID {
            bail!("profile '{}' requires the '{}' backend and is not installable by the native GGUF manager", selected.id, selected.backend);
        }
        (Some(selected.id.into()), selected.components.clone())
    } else {
        (None, request.component_ids.iter().map(String::as_str).collect())
    };
    let catalog = components();
    let selected: Vec<Component> = ids
        .iter()
        .map(|id| catalog.iter().find(|candidate| candidate.id == *id).cloned().with_context(|| format!("unknown component '{id}'")))
        .collect::<Result<_>>()?;
    validate_complete_set(&selected)?;
    let total_bytes = selected.iter().map(|component| component.bytes).sum();
    Ok(ResolvedInstall { profile_id, components: selected, total_bytes, already_present_bytes: 0 })
}

/// The four roles the engine needs: the DiT that renders, the language model
/// that plans, the text encoder and the VAE.
const ROLES: [&str; 4] = ["dit", "lm", "text_encoder", "vae"];

fn validate_complete_set(selected: &[Component]) -> Result<()> {
    for kind in ROLES {
        if selected.iter().filter(|component| component.kind == kind).count() != 1 {
            bail!("a runnable ACE-Step installation needs exactly one {kind} component");
        }
    }
    if selected.len() != ROLES.len() {
        bail!("a component set must name exactly one file per role");
    }
    Ok(())
}

fn profile_files_from_components(components: &[Component]) -> ProfileModelFiles {
    let filename = |kind| components.iter().find(|component| component.kind == kind).expect("complete profile").filename.to_owned();
    ProfileModelFiles { synth_model: filename("dit"), lm_model: filename("lm"), text_encoder: filename("text_encoder"), vae: filename("vae") }
}

fn preflight_space(root: &Path, selection: &ResolvedInstall) -> Result<()> {
    let available = fs2::available_space(root)?;
    // This preflight runs on the HTTP request path. A final GGUF is only
    // published after its SHA-256 has been checked and atomically renamed, so
    // checking its expected final size here is sufficient. Re-hashing a 6 GB
    // language model merely to calculate free space made the first-run UI look
    // frozen.
    let missing = selection.components.iter().filter(|component| !published_component(&root.join(component.filename), component)).map(|component| component.bytes).sum::<u64>();
    if available < missing {
        bail!("not enough disk space: need {missing} bytes, only {available} bytes available");
    }
    Ok(())
}

fn status_snapshot(root: PathBuf, active: Option<DownloadJob>, target: Option<InstallRequest>) -> ManagerStatus {
    let component_statuses: Vec<_> = components()
        .into_iter()
        .map(|component| ComponentStatus {
            id: component.id,
            // Final files are atomically published only after a full SHA-256
            // verification in `download_component`. Status is polled often,
            // therefore it must never hash multi-gigabyte weights again.
            installed: published_component(&root.join(component.filename), &component),
            bytes: component.bytes,
        })
        .collect();
    let target = target
        .and_then(|request| resolve_install(request).ok())
        .or_else(|| resolve_install(InstallRequest { profile_id: Some(recommended_profile().into()), component_ids: vec![] }).ok());
    let installed = |id: &str| component_statuses.iter().find(|component| component.id == id).is_some_and(|component| component.installed);
    let ready = target.as_ref().is_some_and(|selection| selection.components.iter().all(|component| installed(component.id)));
    let download_pending = target.as_ref().map(|selection| selection.components.iter()
        .filter(|component| !installed(component.id)).map(|component| component.bytes).sum()).unwrap_or_default();
    // What the five roles actually resolve to, so the panel can name the files
    // instead of saying "profile default".
    let profile_files = target.as_ref().map(|selection| profile_files_from_components(&selection.components));
    ManagerStatus {
        engine_id: ENGINE_ID, model_root: root.display().to_string(), first_run: !ready, ready, download_pending,
        recommended_profile_id: recommended_profile().into(), active, profile_files,
        installed_components: component_statuses.iter().filter(|component| component.installed).map(|component| component.id.into()).collect(),
        components: component_statuses,
    }
}

/// A component counts as installed when its file is there.
///
/// It gets there by one route: downloaded to `.part`, hashed against the
/// pinned Hugging Face LFS oid, and only then renamed. So presence is the
/// proof, and the alternative - re-hashing eleven gigabytes on every status
/// poll - is not one.
fn published_component(path: &Path, _component: &Component) -> bool {
    fs::metadata(path).map(|metadata| metadata.is_file() && metadata.len() > 0).unwrap_or(false)
}

async fn verified_file_async(path: PathBuf, component: Component) -> Result<bool> {
    tokio::task::spawn_blocking(move || verified_file(&path, &component))
        .await
        .context("join GGUF SHA-256 verification task")?
}

fn profile_complete(profile_id: &str, root: &Path) -> bool {
    let Ok(selection) = resolve_install(InstallRequest { profile_id: Some(profile_id.into()), component_ids: vec![] }) else { return false; };
    selection.components.iter().all(|component| verified_file(&root.join(component.filename), component).unwrap_or(false))
}

fn verified_file(path: &Path, component: &Component) -> Result<bool> {
    if fs::metadata(path).map(|metadata| metadata.len()).unwrap_or(0) == 0 {
        return Ok(false);
    }
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    std::io::copy(&mut file, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()) == component.sha256)
}

fn validate_model_root(root: &Path) -> Result<()> {
    if root.as_os_str().is_empty() || root.parent().is_none() || root.file_name().is_none() {
        bail!("STUDIO_MODELS_ROOT must be a specific non-root directory");
    }
    Ok(())
}

fn part_path(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".part");
    PathBuf::from(value)
}

/// The declared set whose components are exactly these, whatever order they
/// arrive in: picking every component of a set by hand is choosing that set.
pub fn profile_matching(component_ids: &[String]) -> Option<&'static str> {
    let mut wanted: Vec<&str> = component_ids.iter().map(String::as_str).collect();
    wanted.sort_unstable();
    wanted.dedup();
    profiles().into_iter().find_map(|profile| {
        let mut declared = profile.components.clone();
        declared.sort_unstable();
        (declared == wanted).then_some(profile.id)
    })
}

fn profiles() -> Vec<Profile> {
    vec![
        profile("minimal", "Minimal - 2B turbo Q4_K_M, LM 0.6B (4 GB cards)", &["dit-turbo-q4", "lm-0.6b-q8", "te-qwen3-0.6b-q8", "vae-standard-bf16"]),
        profile("recommended-light", "Light - XL turbo Q4_K_M, LM 1.7B (8 GB cards)", &["dit-xl-turbo-q4", "lm-1.7b-q8", "te-qwen3-0.6b-q8", "vae-standard-bf16"]),
        profile("balanced", "Balanced - XL turbo Q6_K, LM 1.7B (10 GB cards)", &["dit-xl-turbo-q6", "lm-1.7b-q8", "te-qwen3-0.6b-q8", "vae-standard-bf16"]),
        profile("quality-q8", "Quality - XL turbo Q8_0, LM 4B Q8_0 (13 GB cards)", &["dit-xl-turbo-q8", "lm-4b-q8", "te-qwen3-0.6b-q8", "vae-standard-bf16"]),
        profile("native", "Full native - XL turbo BF16, LM 4B BF16 (24 GB cards)", &["dit-xl-turbo-bf16", "lm-4b-bf16", "te-qwen3-0.6b-bf16", "vae-standard-bf16"]),
    ]
}

pub fn profile_exists(id: &str) -> bool {
    profiles().iter().any(|profile| profile.id == id && profile.installable && profile.backend == ENGINE_ID)
}

fn profile(id: &'static str, label: &'static str, ids: &[&'static str]) -> Profile {
    let all = components();
    let recommended = id == "quality-q8";
    Profile { id, label, backend: ENGINE_ID, installable: true, recommended, components: ids.to_vec(), total_bytes: ids.iter().filter_map(|id| all.iter().find(|component| component.id == *id)).map(|component| component.bytes).sum() }
}

fn components() -> Vec<Component> {
    vec![
        c("te-qwen3-0.6b-bf16", "text_encoder", "Qwen3-Embedding-0.6B-BF16.gguf", 1196890592, "6f57900773fbd447b2943bc8126e2dc68ed009151ba89a47c14d1be05d4c73df", MAIN),
        c("te-qwen3-0.6b-q8", "text_encoder", "Qwen3-Embedding-0.6B-Q8_0.gguf", 784144960, "972f23255e46adfe744a0eb9a0039f3c63988f65753b0968d776e8b27168c321", MAIN),
        c("lm-0.6b-bf16", "lm", "acestep-5Hz-lm-0.6B-BF16.gguf", 1331108128, "8bc3e1de50fc91751ab893965c78c17589323bf4d2a9c1be2750ada7c7fabfaa", MAIN),
        c("lm-0.6b-q8", "lm", "acestep-5Hz-lm-0.6B-Q8_0.gguf", 709846656, "bdaf9e292d4470f31c19cafeaca1b74936a114667e3a85e5d33b65247e9908ec", MAIN),
        c("lm-1.7b-bf16", "lm", "acestep-5Hz-lm-1.7B-BF16.gguf", 3713827104, "6e1b625ca8dc0bfe912fa7e16a563ec6e8c44e4199b612e3157e1e320a7d3077", MAIN),
        c("lm-1.7b-q8", "lm", "acestep-5Hz-lm-1.7B-Q8_0.gguf", 1975837568, "726f99a82f050b32ebc5c5e36acaa9d7acd06bfe5e579a62b00e0d0d6d0b7ec6", MAIN),
        c("lm-4b-bf16", "lm", "acestep-5Hz-lm-4B-BF16.gguf", 8384454592, "9fca9915919a1108da33d8d98fbe6c1a07bfee101d27098416650dcbde037b72", MAIN),
        c("lm-4b-q5", "lm", "acestep-5Hz-lm-4B-Q5_K_M.gguf", 3025965984, "938ed7067c8897f66acf4c3a86fc1fa8113d5cd1a5f13e6edec2e03207514e2d", MAIN),
        c("lm-4b-q6", "lm", "acestep-5Hz-lm-4B-Q6_K.gguf", 3442713504, "d39b8b5070fa617a0f57dcc26e710f8096a293c21721c5f714318bab7b40cfdb", MAIN),
        c("lm-4b-q8", "lm", "acestep-5Hz-lm-4B-Q8_0.gguf", 4457323648, "972f91147a167f0c041f1b158d67985a82c0f6a852e68cdf70e46030cf08b1bc", MAIN),
        c("dit-base-bf16", "dit", "acestep-v15-base-BF16.gguf", 4791642016, "ef880d77ffec8e6638f268c6237b73a8965af24bdd10fd5cfc27432c9d19ee57", MAIN),
        c("dit-base-q4", "dit", "acestep-v15-base-Q4_K_M.gguf", 1445710208, "942ec6a7bc11a40809af5f337063f2af2acc63e4fcd4a92b0cb42dc2bc7c41e8", MAIN),
        c("dit-base-q5", "dit", "acestep-v15-base-Q5_K_M.gguf", 1700140160, "b7a5c6d1b0729ab8630c96fabc14a47640a708e9bddff60666d8d07279aa57a7", MAIN),
        c("dit-base-q6", "dit", "acestep-v15-base-Q6_K.gguf", 1970472000, "08cf80fcc73f518c9eddfdc1e0ddcb786dc04f7d260479482794f371925b0207", MAIN),
        c("dit-base-q8", "dit", "acestep-v15-base-Q8_0.gguf", 2549527936, "10329f321010cedc3571c24b6371d002ce49238f48be47def4a1c76bf27ee150", MAIN),
        c("dit-sft-bf16", "dit", "acestep-v15-sft-BF16.gguf", 4791642016, "82db4849bfa9fd6438f049a5bce3fbd57b70f57e7433238db2d97becd0eee282", MAIN),
        c("dit-sft-q4", "dit", "acestep-v15-sft-Q4_K_M.gguf", 1445710208, "2531bb61b9e5017fbc8875446e02f54b052eeec41ba3200dd72c6061b59b33d0", MAIN),
        c("dit-sft-q5", "dit", "acestep-v15-sft-Q5_K_M.gguf", 1700140160, "ad8fd75d58c53b3e6a6b23c2660df8f65d96603118a2a5e3c67a744c470efb82", MAIN),
        c("dit-sft-q6", "dit", "acestep-v15-sft-Q6_K.gguf", 1970472000, "8ebf354d21058283fc65f377ceb3488901159cfc96866dd23f2c49347535061b", MAIN),
        c("dit-sft-q8", "dit", "acestep-v15-sft-Q8_0.gguf", 2549527936, "17f1984e48aaab27b3eb8ccbf0b754a6656e677884c60ea7003845cfc0059b70", MAIN),
        c("dit-sftturbo50-bf16", "dit", "acestep-v15-sftturbo50-BF16.gguf", 4791642016, "af24191ae7748e16e5d1860dbe37e581baea5a5d39a7bd479ca09c84fa023fc7", MAIN),
        c("dit-sftturbo50-q8", "dit", "acestep-v15-sftturbo50-Q8_0.gguf", 2549527936, "3fab14cf0d8efa249edf49b6fd0392bac54f1a80514567a5ae4164853339ecb3", MAIN),
        c("dit-turbo-bf16", "dit", "acestep-v15-turbo-BF16.gguf", 4791642080, "c7b463b728cdce2f3d4e5205fc5747d0344ebfea37f19542f5236cd8a76cf462", MAIN),
        c("dit-turbo-q4", "dit", "acestep-v15-turbo-Q4_K_M.gguf", 1445710272, "55b4d8514850f3d0f82536f37e99673aaf48df802b5ae5b153eea32a2e2daa5e", MAIN),
        c("dit-turbo-q5", "dit", "acestep-v15-turbo-Q5_K_M.gguf", 1700140224, "a241c9a721e3704cb04b17ce6a40c9aa714d3ee5cf49c2219972020eb761f5a4", MAIN),
        c("dit-turbo-q6", "dit", "acestep-v15-turbo-Q6_K.gguf", 1970472064, "693fc82977d9af2b5569238e363c11ae9b055d464f036d51127fba4eab016693", MAIN),
        c("dit-turbo-q8", "dit", "acestep-v15-turbo-Q8_0.gguf", 2549528000, "288f708a61cfc241013a98a62f98ba331f83fe34d0d3559acdd9b0f6a2f7cd6b", MAIN),
        c("dit-turbo-continuous-bf16", "dit", "acestep-v15-turbo-continuous-BF16.gguf", 4791642080, "ead4b8717f7297f96cda6dc3d37d69504b007cbb6d79e1c8c931337019bfc377", MAIN),
        c("dit-turbo-continuous-q4", "dit", "acestep-v15-turbo-continuous-Q4_K_M.gguf", 1445710240, "695a304965672025cb55574ff3c78f4da2ad5239fc7ff678a275b93aecac26b5", MAIN),
        c("dit-turbo-continuous-q5", "dit", "acestep-v15-turbo-continuous-Q5_K_M.gguf", 1700140192, "23d66597916bc9da2a214481dadb2bb301e12b7e50bac59372486e2ff7fc4f6b", MAIN),
        c("dit-turbo-continuous-q6", "dit", "acestep-v15-turbo-continuous-Q6_K.gguf", 1970472032, "84fcd2612530ad1d8ec57a46e786db378f6af78e1071c3ee70d4396fc8909282", MAIN),
        c("dit-turbo-continuous-q8", "dit", "acestep-v15-turbo-continuous-Q8_0.gguf", 2549527968, "ed50670ad55ab53bd1c753529e3b6a8e58512e1c5f4df15426948c73fd407abf", MAIN),
        c("dit-turbo-shift1-bf16", "dit", "acestep-v15-turbo-shift1-BF16.gguf", 4791642048, "6c626ac0c282961f1494e6cfa5f1c79956e4fc4b1000e51f29cdc048a06f512c", MAIN),
        c("dit-turbo-shift1-q4", "dit", "acestep-v15-turbo-shift1-Q4_K_M.gguf", 1445710240, "a65d44d12fb88ce99cf3e93197a02592bb3065121417a3d3efced254b63ca6bd", MAIN),
        c("dit-turbo-shift1-q5", "dit", "acestep-v15-turbo-shift1-Q5_K_M.gguf", 1700140192, "b68ae397c326b92316c967e0cb9deea904c0d10e16321052376aa9cef988f033", MAIN),
        c("dit-turbo-shift1-q6", "dit", "acestep-v15-turbo-shift1-Q6_K.gguf", 1970472032, "4c8bb2652ea5bbd28b716059757e9789627d16424f73c0a3374e872e7c12ee5d", MAIN),
        c("dit-turbo-shift1-q8", "dit", "acestep-v15-turbo-shift1-Q8_0.gguf", 2549527968, "3cc621a46b0d4906c0bce09c7b8d563bdc1aa39a4b4856271045bae12287027c", MAIN),
        c("dit-turbo-shift3-bf16", "dit", "acestep-v15-turbo-shift3-BF16.gguf", 4791642048, "8e4c08ccb89030a5be5c21a24d59109341aa79b2fb17d04f790e169705eb112c", MAIN),
        c("dit-turbo-shift3-q4", "dit", "acestep-v15-turbo-shift3-Q4_K_M.gguf", 1445710240, "0d1facb7069f1ad2236408ca8018a21d38424d2526ef488dc3775261c19a9b03", MAIN),
        c("dit-turbo-shift3-q5", "dit", "acestep-v15-turbo-shift3-Q5_K_M.gguf", 1700140192, "33a15a34f90fbdf1d161d3d1a760cd064cd4a454b2103f108123c0e6a3abd404", MAIN),
        c("dit-turbo-shift3-q6", "dit", "acestep-v15-turbo-shift3-Q6_K.gguf", 1970472032, "ef550c4441b1a39ff89e79f6cf5f88676b4a5766f6b684b618763f1de7a1e0a9", MAIN),
        c("dit-turbo-shift3-q8", "dit", "acestep-v15-turbo-shift3-Q8_0.gguf", 2549527968, "21cc9ec83378c71a28aef4a5c1bacab3fe5025e9de8f76487656b6f9de330d21", MAIN),
        c("dit-xl-base-bf16", "dit", "acestep-v15-xl-base-BF16.gguf", 9978530784, "3ebd8e3ff0951dfe9f424a7f7a8e829f05875669b5939bde5c16cf09a940d982", MAIN),
        c("dit-xl-base-q4", "dit", "acestep-v15-xl-base-Q4_K_M.gguf", 2989922656, "13b08ac84e0cdca8ac770d6bc12842046113c9479ddcedd2672ac87ac458ff78", MAIN),
        c("dit-xl-base-q5", "dit", "acestep-v15-xl-base-Q5_K_M.gguf", 3527566432, "c8d639af9f37dca6b02255ef736e6ebbac9318287c9d043db4a30dddc3a2fc3c", MAIN),
        c("dit-xl-base-q6", "dit", "acestep-v15-xl-base-Q6_K.gguf", 4098812960, "44f14805faffa9ee3a28ba02a69915dcc0f0d44218c91292d1e2c0896ac4ee82", MAIN),
        c("dit-xl-base-q8", "dit", "acestep-v15-xl-base-Q8_0.gguf", 5305828704, "45d05b88ccbfa0ea27208ea618d7f0749b2be040b457cfa661d37646ea39f207", MAIN),
        c("dit-xl-sft-bf16", "dit", "acestep-v15-xl-sft-BF16.gguf", 9978530784, "c998effb0c3b447202c49242fd9ebf55ea7afccf766d4b4dd4ecedaed92bcbbb", MAIN),
        c("dit-xl-sft-q4", "dit", "acestep-v15-xl-sft-Q4_K_M.gguf", 2989922656, "fa144d3ea8a892a11a43753070e40bb2105c5bfddb57c99d542fda22297bb1b5", MAIN),
        c("dit-xl-sft-q5", "dit", "acestep-v15-xl-sft-Q5_K_M.gguf", 3527566432, "e7c5fdfa5075d666def2585b76f6537bba478ceedc54eec4d041538ce2fe3842", MAIN),
        c("dit-xl-sft-q6", "dit", "acestep-v15-xl-sft-Q6_K.gguf", 4098812960, "b7af16f54b3181c5f0e4558410245cfa4d73dd2f0635cd7d14a76668fc49660c", MAIN),
        c("dit-xl-sft-q8", "dit", "acestep-v15-xl-sft-Q8_0.gguf", 5305828704, "d7b06fadab214375ef5ebcfa44bf4a10489ea364b119f5b9ecbcd870408bdab9", MAIN),
        c("dit-xl-sftturbo50-bf16", "dit", "acestep-v15-xl-sftturbo50-BF16.gguf", 9978530816, "aef16e967740eeb1f91e145504fe9699fb31c423759fd4bc190683e535f3ba2c", MAIN),
        c("dit-xl-sftturbo50-q4", "dit", "acestep-v15-xl-sftturbo50-Q4_K_M.gguf", 2989922720, "7f7619d78037f9cfbe7b7816f758dc99f2841f48010da12eed732a8faef2eb0d", MAIN),
        c("dit-xl-sftturbo50-q5", "dit", "acestep-v15-xl-sftturbo50-Q5_K_M.gguf", 3527566496, "3d4db8b6ec02c1fd15889bb61be65ce474e3b291fa0ff53fe7be90cd819d3e6b", MAIN),
        c("dit-xl-sftturbo50-q6", "dit", "acestep-v15-xl-sftturbo50-Q6_K.gguf", 4098813024, "d43bad21c85c7a250815367b4c5f7fafdf6b8f7d21f9f27d8a3c47171cf79e7f", MAIN),
        c("dit-xl-sftturbo50-q8", "dit", "acestep-v15-xl-sftturbo50-Q8_0.gguf", 5305828768, "ec6bef50f2aec3176aafa4836401394913f567ceb7a4d53a459948a8b5294e51", MAIN),
        c("dit-xl-turbo-bf16", "dit", "acestep-v15-xl-turbo-BF16.gguf", 9978530816, "10d445a71653a8fef9a16a8457d9fb66da432ac500b357cce028f36c489c1b63", MAIN),
        c("dit-xl-turbo-q4", "dit", "acestep-v15-xl-turbo-Q4_K_M.gguf", 2989922688, "475e60e8644b1f91eebc0cd25f24c060bed06e64a862b4e60ffafe634c740478", MAIN),
        c("dit-xl-turbo-q5", "dit", "acestep-v15-xl-turbo-Q5_K_M.gguf", 3527566464, "108dbf1b1a5cfe41c46b23ee7481a1c6f6d21f7254bed4ac15ff8f307df69d3d", MAIN),
        c("dit-xl-turbo-q6", "dit", "acestep-v15-xl-turbo-Q6_K.gguf", 4098812992, "e1f6665567e9bc0331b8dfd95c2b43a84e8bf114075c9b16abdd25ae97dc58fe", MAIN),
        c("dit-xl-turbo-q8", "dit", "acestep-v15-xl-turbo-Q8_0.gguf", 5305828736, "4f1044fb646374fb5730e10f64325766883eb2fc02a643c1b403f2d61f39dc19", MAIN),
        c("vae-standard-bf16", "vae", "vae-BF16.gguf", 337420928, "0599862ac5d15cd308e1d2e368373aea6c02e25ebd1737ad4a4562a0901b0ef8", MAIN),
        c("dit-merge-base-sft-xl-ta-0.5-bf16", "dit", "acestep-v15-merge-base-sft-xl-ta-0.5-BF16.gguf", 9978530784, "9b91f7f63c320e6fbfd87bc89c7bd24b5b566036edb8d8a824b50a8948c3600a", MERGES),
        c("dit-merge-base-sft-xl-ta-0.5-q4", "dit", "acestep-v15-merge-base-sft-xl-ta-0.5-Q4_K_M.gguf", 2989922688, "145540355c30d998683313debaa4b9a2a1e4cad491fbc45e7fe1746cece3b58c", MERGES),
        c("dit-merge-base-sft-xl-ta-0.5-q5", "dit", "acestep-v15-merge-base-sft-xl-ta-0.5-Q5_K_M.gguf", 3527566464, "2bfd376e38fce556dafabbf54fcbeae5c26b694304ab3f2d31c2ab085e9f3968", MERGES),
        c("dit-merge-base-sft-xl-ta-0.5-q6", "dit", "acestep-v15-merge-base-sft-xl-ta-0.5-Q6_K.gguf", 4098812992, "5c84dd6318913c4ece053ba8d2402d75c91b45434139ba967f647c52b0d13484", MERGES),
        c("dit-merge-base-sft-xl-ta-0.5-q8", "dit", "acestep-v15-merge-base-sft-xl-ta-0.5-Q8_0.gguf", 5305828736, "638cf3904e86db829d768232e079ae06560df7dab14bd3f896672ac2bbe02e41", MERGES),
        c("dit-merge-base-turbo-xl-ta-0.5-bf16", "dit", "acestep-v15-merge-base-turbo-xl-ta-0.5-BF16.gguf", 9978530784, "4ddc5bb457c2d0ea31b99555d6c4b7f54bff86e855d28ba062ebf1de0b483248", MERGES),
        c("dit-merge-base-turbo-xl-ta-0.5-q4", "dit", "acestep-v15-merge-base-turbo-xl-ta-0.5-Q4_K_M.gguf", 2989922688, "2dfa8beceb59b41ad2421a58700b7f55f9d8528648fc4c44ecb39ff571118593", MERGES),
        c("dit-merge-base-turbo-xl-ta-0.5-q5", "dit", "acestep-v15-merge-base-turbo-xl-ta-0.5-Q5_K_M.gguf", 3527566464, "527481ab6b793dfcd9853561c6238e97a9f96527dbfc977ed20ab57c18bc6a59", MERGES),
        c("dit-merge-base-turbo-xl-ta-0.5-q6", "dit", "acestep-v15-merge-base-turbo-xl-ta-0.5-Q6_K.gguf", 4098812992, "815c34b7085c60e5b9851246708a8177e9db5ef794b9735b8d88ac60721c5d2c", MERGES),
        c("dit-merge-base-turbo-xl-ta-0.5-q8", "dit", "acestep-v15-merge-base-turbo-xl-ta-0.5-Q8_0.gguf", 5305828736, "0d46b51054433830d3a589383baca299b8554f0e4c440f354cb218d8c893634f", MERGES),
        c("dit-merge-sft-turbo-xl-ta-0.3-bf16", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.3-BF16.gguf", 9978530784, "fd71304f65faa465b34ec34af4ce761c28e130bed24e61b4a77623b050ef0ec7", MERGES),
        c("dit-merge-sft-turbo-xl-ta-0.3-q4", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.3-Q4_K_M.gguf", 2989922688, "474c49dc4c556e6c7a087ec65bc04b82e8403a1d47ae8778d944f01a4e50082b", MERGES),
        c("dit-merge-sft-turbo-xl-ta-0.3-q5", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.3-Q5_K_M.gguf", 3527566464, "23309c8da98f361ed0f748106e3596fa070ff9d264f070ef90a049c1c8bfb1dd", MERGES),
        c("dit-merge-sft-turbo-xl-ta-0.3-q6", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.3-Q6_K.gguf", 4098812992, "e8f9703ecdac1c69bc0b4f7610a411426057dbf7fee3c15ba76d34524f65591c", MERGES),
        c("dit-merge-sft-turbo-xl-ta-0.3-q8", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.3-Q8_0.gguf", 5305828736, "5d34d816a735edfc80bdafdd94739b624117d74ee048452b0c2af086874e3a3e", MERGES),
        c("dit-merge-sft-turbo-xl-ta-0.7-bf16", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.7-BF16.gguf", 9978530784, "f18579380227f4540bb56a6f8fa1cd33354d88db8002c43ea34a7b9d0aba2c2d", MERGES),
        c("dit-merge-sft-turbo-xl-ta-0.7-q4", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.7-Q4_K_M.gguf", 2989922688, "9589e948ef5f7476a7449bf397bbf97916dae8936139cb7f0ec3f6ca5a3cffba", MERGES),
        c("dit-merge-sft-turbo-xl-ta-0.7-q5", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.7-Q5_K_M.gguf", 3527566464, "59de4078a14c8e706dcb053dad827418bc60434b08cd7b9ad1e261ab160c9548", MERGES),
        c("dit-merge-sft-turbo-xl-ta-0.7-q6", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.7-Q6_K.gguf", 4098812992, "d0693132bd076d098e423431b1c9e9ccd69b534d29cccca462e4ededdfeeef79", MERGES),
        c("dit-merge-sft-turbo-xl-ta-0.7-q8", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.7-Q8_0.gguf", 5305828736, "e246c8478d904086572def8d5ad6306d2a06d98d787a7147f830c95bcc0c5c7b", MERGES),
        c("dit-base-mxfp4", "dit", "acestep-v15-base-MXFP4.gguf", 1281885888, "fc40ba55ea30cacbe9863d0f7ed3a33c0758676df89d74380bc50ae1da070df6", MXFP4),
        c("dit-merge-base-turbo-xl-ta-0.5-mxfp4", "dit", "acestep-v15-merge-base-turbo-xl-ta-0.5-MXFP4.gguf", 2660726464, "484e0cf0a2f46bf4950c9033f700b8698445b0680c23e20f0f7cf2f42424983a", MXFP4),
        c("dit-merge-sft-turbo-xl-ta-0.3-mxfp4", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.3-MXFP4.gguf", 2660726464, "8847b8a566918873205d0920b09b750d4b8dc1ff7f1c55988020552d1654c11e", MXFP4),
        c("dit-merge-sft-turbo-xl-ta-0.7-mxfp4", "dit", "acestep-v15-merge-sft-turbo-xl-ta-0.7-MXFP4.gguf", 2660726464, "baa8fb4c74b2a76c927c89a4bf442a0fed8436efa032c1f8bfa07a83998930d2", MXFP4),
        c("dit-sft-mxfp4", "dit", "acestep-v15-sft-MXFP4.gguf", 1281885888, "2c5a3d1686bd283db8b94132451faa5878f2a4f7b72de915a785e0b5a16d027e", MXFP4),
        c("dit-sftturbo50-mxfp4", "dit", "acestep-v15-sftturbo50-MXFP4.gguf", 1281885888, "d09687038fda3f05c4feddf98d1282def5de27a4187f0e1685cd383bb9096901", MXFP4),
        c("dit-turbo-mxfp4", "dit", "acestep-v15-turbo-MXFP4.gguf", 1281885952, "4c1e6ef7fca88111e3def961372435aa9096cf71200a2887443d5f61234dd92c", MXFP4),
        c("dit-turbo-continuous-mxfp4", "dit", "acestep-v15-turbo-continuous-MXFP4.gguf", 1281885920, "b4023b581530a51b61f572270c1f732634c98086211ff714a8f73ddc07d1b66a", MXFP4),
        c("dit-turbo-shift1-mxfp4", "dit", "acestep-v15-turbo-shift1-MXFP4.gguf", 1281885920, "ccbda7b544fef52d3b4669297a185bca5ead2913cb74025957f674acd2e3302c", MXFP4),
        c("dit-turbo-shift3-mxfp4", "dit", "acestep-v15-turbo-shift3-MXFP4.gguf", 1281885920, "301f059684267d62135a4b4630672a741891aea7137647b7d1eac837cb4ddeff", MXFP4),
        c("dit-xl-base-mxfp4", "dit", "acestep-v15-xl-base-MXFP4.gguf", 2660726432, "4d45d06b7c7304ff0334e378a7538dfc2d750acb1263b4ab1f59190b1722295e", MXFP4),
        c("dit-xl-sft-mxfp4", "dit", "acestep-v15-xl-sft-MXFP4.gguf", 2660726432, "082487454773da575628d613e80da15f88505919cca209bad46dea09b27fd47d", MXFP4),
        c("dit-xl-sftturbo50-mxfp4", "dit", "acestep-v15-xl-sftturbo50-MXFP4.gguf", 2660726496, "faf4bc94052c0c9ce6bc9ddb32a6b78220d512e8ab4d83f79ac54b1f2a755c5c", MXFP4),
        c("dit-xl-turbo-mxfp4", "dit", "acestep-v15-xl-turbo-MXFP4.gguf", 2660726464, "dfb0ba27f2dd25c6bb31ad7f1a7c8d6e5c538593c5fff92e3329d186c81751e4", MXFP4),
        c("vae-scragvae-bf16", "vae", "scragvae-BF16.gguf", 337420928, "2e56bd72b2c1599932513c9089171c171221d6edee02f1f903c9d34daa7d63d6", SCRAGVAE),
        nested(c("dit-xl-turbo-regrind-q4", "dit", "acestep_1.5_xl_turbo_regrind_v1-Q4_K_M.gguf", 2989922688, "f0f77a5c40d3fdee2a3c46731fa181cb8965a9c9e69cbd5d9eaee3057216c692", REGRIND), "dit"),
        nested(c("dit-xl-turbo-regrind-q6", "dit", "acestep_1.5_xl_turbo_regrind_v1-Q6_K.gguf", 4098812992, "bd4690a6233d88de52bc3c222c3695f05a5be4af441f2d3d285eaed3933de254", REGRIND), "dit"),
        nested(c("dit-xl-turbo-regrind-q8", "dit", "acestep_1.5_xl_turbo_regrind_v1-Q8_0.gguf", 5305828736, "63bd1ce2e9668e5373558813e64e6d9b2e3c7eb2710ff808f5dff6ada08d7d34", REGRIND), "dit"),
        nested(c("vae-regrind-v10b-bf16", "vae", "acestep_1.5_vae_Regrind_V10b-BF16.gguf", 337420960, "f1a0dd870b59a4c15f528ff3f890f8d31ff33225533d19e633579f1893263603", REGRIND), "vae"),
        nested(c("vae-regrind-v10b-blend50-bf16", "vae", "acestep_1.5_vae_Regrind_V10b_blend50-BF16.gguf", 337420928, "9cca7d4f48b977e97189cdf80491a87da30a50e8e5c15c2d8d33494390bcc4e7", REGRIND), "vae"),
        nested(c("vae-regrind-v9b-bf16", "vae", "acestep_1.5_vae_Regrind_V9b-BF16.gguf", 337420928, "3758d496e23fa06c12fa963375f75feb52d574525589d498c483ffc470ce1399", REGRIND), "vae"),
        nested(c("vae-regrind-v9b-blend50-bf16", "vae", "acestep_1.5_vae_Regrind_V9b_blend50-BF16.gguf", 337420928, "cf9fa7cc273f2b6215a1ef59eff9a4b945b986d5ece4dfc0cd9e73f12296f6ae", REGRIND), "vae"),
    ]
}

fn c(id: &'static str, kind: &'static str, filename: &'static str, bytes: u64, sha256: &'static str, source: (&'static str, &'static str)) -> Component {
    Component { id, kind, filename, bytes, sha256, repository: source.0, revision: source.1, remote_path: filename }
}

/// A component its repository keeps in a folder.
fn nested(component: Component, folder: &'static str) -> Component {
    Component { remote_path: Box::leak(format!("{folder}/{}", component.filename).into_boxed_str()), ..component }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_picked_set_equal_to_a_declared_one_is_that_set() {
        let ids = |list: &[&str]| list.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        assert_eq!(profile_matching(&ids(&["vae-standard-bf16", "dit-xl-turbo-q8", "te-qwen3-0.6b-q8", "lm-4b-q8"])), Some("quality-q8"));
        assert_eq!(profile_matching(&ids(&["dit-xl-turbo-q8", "lm-4b-q8", "te-qwen3-0.6b-q8"])), None);
        assert_eq!(profile_matching(&ids(&["dit-xl-turbo-q6", "lm-4b-q8", "te-qwen3-0.6b-q8", "vae-standard-bf16"])), None);
    }

    #[tokio::test]
    async fn file_hashing_does_not_block_the_async_runtime() {
        let path = std::env::temp_dir().join(format!("studio-hash-test-{}", uuid::Uuid::now_v7()));
        fs::File::create(&path).unwrap().set_len(8 * 1024 * 1024).unwrap();
        let mut component = components().into_iter().next().unwrap();
        component.bytes = 8 * 1024 * 1024;
        component.sha256 = "not-a-real-digest";
        let hash_task = tokio::spawn(verified_file_async(path.clone(), component));
        tokio::time::timeout(std::time::Duration::from_secs(1), tokio::time::sleep(std::time::Duration::from_millis(1))).await.unwrap();
        assert!(!hash_task.await.unwrap().unwrap());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn startup_marks_orphaned_download_as_cancelled_without_erasing_resume_state() {
        let mut state = PersistentState { active: Some(DownloadJob { id: "job".into(), profile_id: Some("minimal".into()), component_ids: vec!["dit-turbo-q4".into()], status: DownloadStatus::Downloading, downloaded_bytes: 123, total_bytes: 456, error: None }) };
        assert!(recover_interrupted_download(&mut state));
        let recovered = state.active.unwrap();
        assert!(matches!(recovered.status, DownloadStatus::Cancelled));
        assert_eq!(recovered.downloaded_bytes, 123);
    }

    #[test]
    fn every_profile_is_a_complete_runnable_set() {
        for profile in profiles() {
            let selected = resolve_install(InstallRequest { profile_id: Some(profile.id.into()), component_ids: vec![] }).unwrap();
            validate_complete_set(&selected.components).unwrap();
        }
        assert!(profiles().iter().any(|profile| profile.id == recommended_profile()));
    }

    #[test]
    fn an_empty_request_downloads_nothing() {
        let error = resolve_install(InstallRequest { profile_id: None, component_ids: vec![] })
            .expect_err("an empty request is a mistake, not a default");
        assert!(error.to_string().contains("nothing was selected"));
    }

    #[test]
    fn advanced_selection_rejects_partial_sets() {
        let result = resolve_install(InstallRequest { profile_id: None, component_ids: vec!["lm-4b-q8".into(), "dit-xl-turbo-q8".into()] });
        assert!(result.is_err());
    }

    #[test]
    fn the_catalogue_pins_every_file() {
        let all = components();
        assert!(all.iter().all(|component| component.sha256.len() == 64 && component.revision.len() == 40));
        let mut ids: Vec<_> = all.iter().map(|component| component.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "component ids must be unique");
    }

    #[test]
    fn a_profile_resolves_to_engine_request_fields() {
        let selection = resolve_install(InstallRequest { profile_id: Some("quality-q8".into()), component_ids: vec![] }).unwrap();
        let files = profile_files_from_components(&selection.components);
        assert_eq!(files.synth_model, "acestep-v15-xl-turbo-Q8_0.gguf");
        assert_eq!(files.lm_model, "acestep-5Hz-lm-4B-Q8_0.gguf");
        assert_eq!(files.vae, "vae-BF16.gguf");
    }
}
