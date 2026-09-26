#![recursion_limit = "256"]
mod processing;
mod vst;
mod adapters;
mod training;
mod audio_facts;
mod beat_dbn;
mod listen;
mod prepare;
mod auto_title;
mod tagging;
mod cover_prompt;
mod providers;
mod assistant;
mod assistant_runtime;
mod audio_pcm;
mod downloads;
mod cuda_build;
mod engine_runtime;
mod lyrics_db;
mod mcp;
mod lyrics_sync;
mod credentials;
mod model_manager;
mod presets;
mod request_log;
mod resources;
mod chunked;
mod separation;
mod midi;
mod sizes;
mod skill;
mod library;
mod ace;
mod multipart;
mod openrouter_stream;

use std::{collections::HashMap, env, fs, net::SocketAddr, path::PathBuf, sync::Arc};
use anyhow::Context;
use futures_util::StreamExt;

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    body::Body,
    http::{header, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use music_core::{Capability, EngineDescriptor, ExecutionMode, StudioConfiguration};
use model_manager::{DownloadStatus, InstallRequest, ModelManager};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

const PRIMARY_MUSIC_ENGINE_ID: &str = model_manager::ENGINE_ID;

#[derive(Clone)]
struct AppState {
    configuration: Arc<RwLock<StudioConfiguration>>,
    jobs: Arc<RwLock<HashMap<String, MusicJob>>>,
    music_server: ace::AceClient,
    model_manager: ModelManager,
    selected_profile_id: Arc<RwLock<Option<String>>>,
    selected_component_ids: Arc<RwLock<Option<Vec<String>>>>,
    settings_path: PathBuf,
    openrouter_catalog: Arc<RwLock<OpenRouterCatalogState>>,
    library: library::Library,
    /// Owned local engine process, when this service started one.
    engine: Arc<tokio::sync::Mutex<Option<music_engine::server::ServerSupervisor>>>,
    /// Held for reading by everything that sends the engine work and for
    /// writing by a rescan, which restarts the engine: never under a song.
    engine_use: Arc<tokio::sync::RwLock<()>>,
    engine_options: Arc<RwLock<EngineOptions>>,
    /// Devices that failed under Auto in this session - the engine did not
    /// start on them, or died with a CUDA or Vulkan error - and the one the
    /// running engine computes on. Auto walks CUDA, Vulkan, the processor and
    /// skips the failed; a restart of the studio tries them all again.
    failed_devices: Arc<RwLock<Vec<music_engine::server::ComputeBackend>>>,
    active_device: Arc<RwLock<Option<music_engine::server::ComputeBackend>>>,
    /// The CUDA libraries the engine binary imports. They are downloaded, not
    /// installed, so the engine cannot start until they are on disk.
    engine_runtime: Arc<engine_runtime::EngineRuntime>,
    assistant: Arc<RwLock<AssistantConfig>>,
    assistant_runtime: Arc<assistant_runtime::AssistantRuntime>,
    lyrics_sync: Arc<lyrics_sync::LyricsSync>,
    lyrics_sync_config: Arc<RwLock<lyrics_sync::LyricsSyncConfig>>,
    /// Saved cover looks, filled in from whichever track a cover is for.
    cover_templates: Arc<RwLock<Vec<cover_prompt::CoverTemplate>>>,
    /// The look a new cover starts from, chosen in Settings.
    cover_template_default: Arc<RwLock<Option<String>>>,
    separator: Arc<separation::Separator>,
    separation_config: Arc<RwLock<separation::SeparationConfig>>,
    /// Draw a cover as soon as a track is finished.
    cover_auto: Arc<RwLock<bool>>,
    /// What is being done to finished tracks right now - covers, karaoke - so
    /// the interface can say it instead of leaving the user guessing.
    activity: Arc<RwLock<Vec<Activity>>>,
    /// The separation run in progress, if any. One at a time: the model wants
    /// the whole machine for a minute, and two runs would only make both slow.
    separation_run: Arc<RwLock<Option<SeparationRun>>>,
    /// Audio to MIDI: the transcriber and its weights, the run at work or the
    /// last one, the notes it has heard so far, and the stop signal.
    midi: Arc<midi::Transcriber>,
    midi_run: Arc<RwLock<Option<midi::Run>>>,
    midi_notes: Arc<RwLock<Vec<midi::Note>>>,
    midi_stop: Arc<(std::sync::atomic::AtomicBool, tokio::sync::Notify)>,
    /// The processing run in progress or the last one, with its preview.
    processing_run: Arc<RwLock<Option<processing::ProcessRun>>>,
    /// Installed adapters, the catalogue and their downloads.
    adapters: Arc<adapters::AdapterLibrary>,
    /// Adapter training: its optional weights, datasets and runs.
    training: Arc<training::Training>,
    /// The dataset preparation in progress or the last one.
    prepare: prepare::Shared,
    prepare_cancel: Arc<std::sync::atomic::AtomicBool>,
    prepare_train: prepare::TrainSlot,
}

/// A song the create page asks for: the engine's own request fields, flat,
/// and what the studio does around them.
#[derive(Debug, Clone, Deserialize)]
struct CreateMusicJobRequest {
    /// Library title only; never sent to the engine.
    title: Option<String>,
    /// What the cover should show, when the assistant already described it.
    cover_prompt: Option<String>,
    /// Let the language model plan the song (metadata, lyrics, audio codes)
    /// before the DiT renders it.
    #[serde(default = "default_true")]
    think: bool,
    /// The library track a cover, repaint or stem task works on.
    source_song_id: Option<String>,
    /// A library track whose timbre the render follows.
    reference_song_id: Option<String>,
    /// Installed adapters to apply, in order.
    #[serde(default)]
    adapters: Vec<AdapterUse>,
    /// The window's own mark for this request, handed back on the job so the
    /// window knows the job as its own before the response reaches it.
    client_ref: Option<String>,
    /// Fades the studio puts on the rendered track, in seconds.
    #[serde(default)]
    fade_in: f32,
    #[serde(default)]
    fade_out: f32,
    /// Every other field is an `AceRequest` field, checked by `ace::prepare`.
    #[serde(flatten)]
    engine: serde_json::Map<String, Value>,
}

fn default_true() -> bool {
    true
}

impl CreateMusicJobRequest {
    fn text(&self, key: &str) -> String {
        self.engine.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
    }

    fn caption(&self) -> String {
        self.text("caption")
    }

    fn lyrics(&self) -> String {
        self.text("lyrics")
    }

    fn duration_seconds(&self) -> f64 {
        self.engine.get("duration").and_then(Value::as_f64).unwrap_or_default()
    }
}

/// One adapter of a request: its folder and a strength per engine slot. A slot
/// left out is not changed.
#[derive(Debug, Clone, Default, Deserialize)]
struct AdapterUse {
    id: String,
    #[serde(default)]
    scales: std::collections::BTreeMap<String, f64>,
}

/// The name this request goes into the library under: the user's, or one taken
/// from the song when they left the field empty.
fn titled(request: &CreateMusicJobRequest) -> String {
    request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let lyrics = request.lyrics();
            auto_title::auto_title(&request.caption(), &lyrics, lyrics.trim().is_empty() || lyrics.trim() == "[Instrumental]")
        })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum MusicJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum MusicJobDispatch {
    NotConfigured,
    Local,
    OpenRouter,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum MusicJobPhase {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
struct MusicJob {
    /// Where a track made from another comes from (a re-render).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    derived: Option<Value>,
    id: String,
    /// The mark the submitting window gave the request; absent for an agent's.
    #[serde(skip_serializing_if = "Option::is_none")]
    client_ref: Option<String>,
    engine_id: String,
    /// What the assistant said this track's cover should show, if anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    status: MusicJobStatus,
    dispatch: MusicJobDispatch,
    phase: MusicJobPhase,
    caption: String,
    lyrics: String,
    duration_seconds: f64,
    generation_settings: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    song: Option<CompletedSong>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    songs: Vec<CompletedSong>,
    message: String,
    /// The engine job running for this song now, for cancel to reach.
    #[serde(skip)]
    engine_job: Option<String>,
    #[serde(skip)]
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
struct CompletedSong {
    id: String,
    song: library::Song,
    audio_url: String,
}

#[derive(Debug, Serialize)]
struct LocalMusicModelCatalog {
    engine_id: String,
    catalog: Value,
}

#[derive(Debug, Serialize)]
struct CapabilitiesResponse {
    engines: Vec<EngineDescriptor>,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

#[derive(Debug, Deserialize)]
struct SetupSelectRequest {
    #[serde(default)]
    profile_id: Option<String>,
    #[serde(default)]
    component_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SetupDownloadRequest {
    // The panel calls this field `component_ids`. Reading only `ids` meant a
    // download request arrived empty, and an empty request quietly fell back to
    // the default set - which is how pressing "download" on the 11.9 GB set
    // started fetching the 26.6 GB one.
    #[serde(default, alias = "component_ids")]
    ids: Vec<String>,
    profile_id: Option<String>,
    /// Make the set the studio's once it is on disk; a file fetched for
    /// training only leaves the studio's set as it is.
    #[serde(default = "selects")]
    select: bool,
}

fn selects() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct ApplyPresetRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ReplayMusicJobRequest {
    song_id: Option<String>,
    replay_request: Option<Value>,
    steps: Option<u32>,
    seed: Option<i64>,
    output_format: Option<String>,
    /// Any other engine fields to change for this render (model, solver,
    /// scheduler, guidance...).
    changes: Option<Value>,
    /// The window's own mark, as on a new song.
    client_ref: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedStudioSettings {
    #[serde(default)]
    engine_options: EngineOptions,
    #[serde(default)]
    assistant: AssistantConfig,
    lyrics_sync: lyrics_sync::LyricsSyncConfig,
    configuration: StudioConfiguration,
    selected_profile_id: Option<String>,
    #[serde(default)]
    selected_component_ids: Option<Vec<String>>,
    #[serde(default)]
    cover_templates: Option<Vec<cover_prompt::CoverTemplate>>,
    #[serde(default)]
    cover_template_default: Option<String>,
    #[serde(default)]
    separation: Option<separation::SeparationConfig>,
    /// Whether a finished track gets its cover drawn without being asked.
    #[serde(default)]
    cover_auto: Option<bool>,
}

#[derive(Default)]
struct OpenRouterCatalogState {
    catalog: Option<providers::openrouter::CapabilityCatalog>,
    refreshed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterTranscriptionRequest {
    model_id: String,
    audio_base64: String,
    audio_format: String,
    language: Option<String>,
}

/// Launch flags for the local engine process. They are a property of the
/// running engine, so changing one restarts it; upstream has no way to apply
/// them to a live server.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct EngineOptions {
    backend: music_engine::server::ComputeBackend,
    keep_loaded: bool,
    max_batch: Option<u32>,
    max_seq: Option<u32>,
    disable_flash_attention: bool,
    split_cfg_forwards: bool,
    clamp_fp16: bool,
}

impl EngineOptions {
    /// How many songs one request may render, and the `--max-batch` the engine
    /// is started with - `lm_batch_size` may not exceed it.
    ///
    /// One, like the engine's own default. The flag reserves KV cache for the
    /// full batch when the weights load, whether or not anyone asks for it, so
    /// a studio that quietly asked for four made every single-song generation
    /// pay for three it would never render. Whoever wants more sets it, and the
    /// engine restarts with the memory that choice costs.
    fn effective_max_batch(&self) -> u32 {
        self.max_batch.unwrap_or(1).max(1)
    }

    /// The devices a start tries, in order. A device chosen in Settings is
    /// the only one, whatever happens to it. Auto goes CUDA, Vulkan, the
    /// processor: CUDA when one of its builds runs this card and driver, then
    /// Vulkan, which fails fast on a machine without a Vulkan card, and the
    /// processor always, last. Not Vulkan beside an NVIDIA card: the DiT on
    /// Vulkan hangs an RTX 4090 until Windows resets its driver.
    fn device_chain(&self, failed: &[music_engine::server::ComputeBackend]) -> Vec<music_engine::server::ComputeBackend> {
        use music_engine::server::ComputeBackend;
        if self.backend != ComputeBackend::Auto {
            return vec![self.backend];
        }
        let mut chain = Vec::new();
        if cuda_build::current().is_some() {
            chain.push(ComputeBackend::Cuda);
        }
        if !cuda_build::nvidia_card() {
            chain.push(ComputeBackend::Vulkan);
        }
        chain.retain(|device| !failed.contains(device));
        chain.push(ComputeBackend::Cpu);
        chain
    }

    /// The CUDA build the engine will compute on, and so the cuBLAS it needs:
    /// chosen outright, or left to ggml on an NVIDIA card one of the builds
    /// runs on. None on Vulkan and the processor.
    fn cuda_build(&self) -> Option<cuda_build::CudaBuild> {
        use music_engine::server::ComputeBackend;
        match self.backend {
            ComputeBackend::Cuda | ComputeBackend::Auto => cuda_build::current(),
            ComputeBackend::Vulkan | ComputeBackend::Cpu => None,
        }
    }

    fn to_engine(self) -> music_engine::server::ServerOptions {
        let cuda = self.cuda_build();
        music_engine::server::ServerOptions {
            backend: self.backend,
            keep_loaded: self.keep_loaded,
            // The ceiling the studio offers, given to the engine that has to
            // honour it. `--max-batch` sizes the language model's KV sets when
            // the weights are loaded, and the engine refuses any request above
            // it: leaving the flag off meant it loaded with the upstream
            // default of one while the panel offered four, so asking for two
            // songs failed before it started.
            max_batch: Some(self.effective_max_batch()),
            max_seq: self.max_seq,
            disable_flash_attention: self.disable_flash_attention,
            split_cfg_forwards: self.split_cfg_forwards,
            // Off CUDA - Vulkan, where an AMD Radeon leaves the FP16 range
            // and renders silence, or the processor - and on CUDA cards before
            // Ampere, which accumulate in FP16. In range it changes nothing.
            clamp_fp16: self.clamp_fp16 || cuda.is_none() || cuda_build::accumulates_in_fp16(),
            cuda_folder: cuda.map(cuda_build::CudaBuild::folder),
        }
    }
}

/// Where the optional writing assistant runs.
///
/// `None` is the default and a first-class state: the manual form is the
/// primary way to use this model, and on a modest card nobody wants a language
/// model competing for VRAM. Nothing is downloaded or started unless the user
/// picks a provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct AssistantConfig {
    provider: AssistantProvider,
    /// Base URL of an OpenAI-compatible server (llama.cpp, LM Studio, Ollama).
    local_base_url: Option<String>,
    local_model: Option<String>,
    openrouter_model: Option<String>,
    /// Id of a model downloaded through the assistant runtime, run as a
    /// sidecar by Studio itself.
    managed_model: Option<String>,
    /// A GGUF already on this machine, run by the same sidecar. Machines that
    /// already keep a Gemma around for another tool do not need a second copy.
    managed_path: Option<String>,
    /// How hard a reasoning model should think, in OpenRouter's own terms:
    /// minimal, low, medium, high, xhigh, max - or none.
    reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AssistantProvider {
    #[default]
    None,
    Local,
    OpenRouter,
    /// A model Studio downloaded and runs itself with llama.cpp.
    Managed,
    /// The agent connected over MCP: the studio asks it what it would ask its
    /// own model, with the same instructions and answer schema.
    Agent,
}

impl AssistantConfig {
    fn available(&self) -> bool {
        match self.provider {
            AssistantProvider::None => false,
            AssistantProvider::Agent => true,
            AssistantProvider::Local => {
                self.local_base_url.as_deref().is_some_and(|url| !url.trim().is_empty())
                    && self.local_model.as_deref().is_some_and(|model| !model.trim().is_empty())
            }
            AssistantProvider::OpenRouter => {
                self.openrouter_model.as_deref().is_some_and(|model| !model.trim().is_empty())
                    && credentials::openrouter_source().is_some()
            }
            // Availability is confirmed against the disk in `assistant_status`;
            // a model id alone only says one was chosen.
            AssistantProvider::Managed => {
                self.managed_model.as_deref().is_some_and(|model| !model.trim().is_empty())
                    || self.managed_path.as_deref().is_some_and(|path| !path.trim().is_empty())
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ProxyImageRequest {
    url: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterSettingsRequest {
    /// `None` or an empty string clears the locally stored credential.
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterCompletionRequest {
    model_id: String,
    prompt: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterCoverRequest {
    model_id: String,
    prompt: String,
}

#[derive(Debug, Serialize)]
struct OpenRouterResponse {
    body: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_id: Option<String>,
}

/// Runs the studio service until the process is asked to stop.
///
/// This is a library entry point on purpose: the desktop application hosts it
/// in-process, so a release is a single executable rather than a launcher that
/// has to start a second binary and keep it alive.
pub async fn serve() -> anyhow::Result<()> {
    let settings_path = studio_settings_path();
    let persisted = load_studio_settings(&settings_path);
    let model_manager = ModelManager::from_environment()?;
    let persisted_components = persisted
        .as_ref()
        .and_then(|settings| settings.selected_component_ids.clone())
        .filter(|ids| model_manager.installed_component_files(ids).is_ok());
    // a hand-picked selection that is exactly a declared set is that set
    let (selected_profile_id, selected_component_ids) = match persisted_components {
        Some(ids) => match model_manager::profile_matching(&ids) {
            Some(profile) => (Some(profile.to_owned()), None),
            None => (None, Some(ids)),
        },
        None => (
            persisted
                .as_ref()
                .and_then(|settings| settings.selected_profile_id.clone())
                .or_else(|| Some(presets::recommended_local_profile().into())),
            None,
        ),
    };
    let state = AppState {
        configuration: Arc::new(RwLock::new(sanitize_persisted_configuration(
            persisted.as_ref().map(|settings| settings.configuration.clone()).unwrap_or_else(initial_configuration),
        ))),
        jobs: Arc::new(RwLock::new(HashMap::new())),
        music_server: ace::AceClient::from_environment(),
        model_manager,
        cover_templates: Arc::new(RwLock::new(
            persisted
                .as_ref()
                .and_then(|settings| settings.cover_templates.clone())
                .filter(|templates| !templates.is_empty())
                .unwrap_or_else(cover_prompt::default_templates),
        )),
        cover_template_default: Arc::new(RwLock::new(
            persisted.as_ref().and_then(|settings| settings.cover_template_default.clone()),
        )),
        activity: Arc::new(RwLock::new(Vec::new())),
        cover_auto: Arc::new(RwLock::new(
            persisted.as_ref().and_then(|settings| settings.cover_auto).unwrap_or(false),
        )),
        separation_config: Arc::new(RwLock::new(
            persisted.as_ref().and_then(|settings| settings.separation.clone()).unwrap_or_default(),
        )),
        separator: Arc::new(separation::Separator::new(
            &studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")),
        )),
        separation_run: Arc::new(RwLock::new(None)),
        midi: Arc::new(midi::Transcriber::new(&studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")))),
        midi_run: Arc::new(RwLock::new(None)),
        midi_notes: Arc::new(RwLock::new(Vec::new())),
        midi_stop: Arc::new((std::sync::atomic::AtomicBool::new(false), tokio::sync::Notify::new())),
        processing_run: Arc::new(RwLock::new(None)),
        prepare: Arc::new(std::sync::Mutex::new(None)),
        prepare_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        prepare_train: Arc::new(std::sync::Mutex::new(None)),
        adapters: Arc::new(adapters::AdapterLibrary::new(
            &studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")),
            PRIMARY_MUSIC_ENGINE_ID,
        )),
        training: Arc::new(training::Training::new(
            &studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")),
            PRIMARY_MUSIC_ENGINE_ID,
        )),
        selected_profile_id: Arc::new(RwLock::new(selected_profile_id)),
        selected_component_ids: Arc::new(RwLock::new(selected_component_ids)),
        settings_path,
        openrouter_catalog: Arc::new(RwLock::new(OpenRouterCatalogState::default())),
        library: library::Library::open_default()?,
        engine: Arc::new(tokio::sync::Mutex::new(None)),
        engine_use: Arc::new(tokio::sync::RwLock::new(())),
        engine_options: Arc::new(RwLock::new(persisted.as_ref().map(|settings| settings.engine_options).unwrap_or_default())),
        failed_devices: Arc::new(RwLock::new(Vec::new())),
        active_device: Arc::new(RwLock::new(None)),
        engine_runtime: Arc::new(engine_runtime::EngineRuntime::new(&engine_bundle_root())),
        assistant: Arc::new(RwLock::new(persisted.as_ref().map(|settings| settings.assistant.clone()).unwrap_or_default())),
        assistant_runtime: Arc::new(assistant_runtime::AssistantRuntime::new(
            &studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")),
        )),
        lyrics_sync: Arc::new(lyrics_sync::LyricsSync::new(
            &studio_data_root().unwrap_or_else(|| std::path::PathBuf::from(".")),
        )),
        lyrics_sync_config: Arc::new(RwLock::new(
            persisted.as_ref().map(|settings| settings.lyrics_sync.clone()).unwrap_or_default(),
        )),
    };
    processing::clear_workspace(state.library.media_dir());
    state.training.recover();
    prepare::resume(&state);

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/configuration", get(configuration).put(update_configuration))
        .route("/engine/presets", get(engine_presets))
        .route("/engine/preset", post(apply_engine_preset))
        .route("/engine/options", get(engine_options).put(update_engine_options))
        .route("/engine/restart", post(restart_local_engine))
        .route("/v1/resources/rescan", post(rescan_resources_route))
        .route("/v1/engine/logs", get(engine_logs))
        .route("/v1/system/resources", get(system_resources))
        .route("/v1/proxy/image", get(proxy_image))
        .route("/v1/openrouter/settings", get(openrouter_settings).put(update_openrouter_settings))
        .route("/v1/openrouter/logs", get(openrouter_logs))
        .route("/v1/assistant/status", get(assistant_status).put(update_assistant_settings))
        .route("/v1/assistant/local-models", get(assistant_local_models))
        .route("/v1/assistant/write", post(assistant_write))
        .route("/v1/assistant/write/stream", post(assistant_write_stream))
        .route("/v1/assistant/sections", post(assistant_sections))
        .route("/v1/assistant/runtime", get(assistant_runtime_status))
        .route("/v1/assistant/runtime/install", post(assistant_runtime_install))
        .route("/v1/assistant/runtime/cancel", post(cancel_assistant_download))
        .route("/v1/assistant/runtime/remove", post(assistant_runtime_remove))
        .route("/v1/assistant/runtime/start", post(assistant_runtime_start))
        .route("/v1/assistant/runtime/stop", post(assistant_runtime_stop))
        .route("/v1/karaoke/status", get(karaoke_status).put(update_karaoke_settings))
        .route("/v1/karaoke/install", post(karaoke_install))
        .route("/v1/karaoke/cancel", post(cancel_karaoke_download))
        .route("/v1/karaoke/remove", post(karaoke_remove))
        .route("/v1/library/songs/{id}/karaoke", post(create_song_karaoke).delete(delete_song_karaoke))
        .route("/v1/openrouter/catalog", get(openrouter_catalog))
        .route("/v1/openrouter/catalog/refresh", post(refresh_openrouter_catalog))
        .route("/v1/openrouter/transcriptions", post(create_openrouter_transcription))
        .route("/v1/openrouter/covers", post(create_openrouter_cover))
        .route("/editor", get(|| async { axum::response::Redirect::permanent("/editor/index.html") }))
        .route("/editor/{*path}", get(editor_asset))
        .route("/v1/separation/runtime", get(separation_assets))
        .route("/v1/separation/runtime/install", post(install_separation_asset))
        .route("/v1/separation/runtime/cancel", post(cancel_separation_download))
        .route("/v1/separation/status", get(separation_status))
        .route("/v1/separation/settings", get(read_separation_settings).put(write_separation_settings))
        .route("/v1/separation/install", post(install_separation_model))
        .route("/v1/separation/remove", post(remove_separation_model))
        .route("/v1/library/songs/{id}/process", post(start_processing))
        .route("/v1/library/songs/{id}/version", axum::routing::put(select_song_version))
        .route("/v1/library/songs/{id}/versions/{version}", axum::routing::delete(remove_song_version))
        .route("/v1/processing", get(read_processing))
        .route("/v1/processing/vst", get(read_vst))
        .route("/v1/processing/vst/scan", post(scan_vst))
        .route("/v1/processing/vst/editor", post(open_vst_editor))
        .route("/v1/adapters", get(list_adapters))
        .route("/v1/adapters/import", post(import_adapter))
        .route("/v1/adapters/cancel", post(cancel_adapter_download))
        .route("/v1/adapters/install", post(install_catalog_adapters))
        .route("/v1/adapters/hub", get(search_hub_adapters))
        .route("/v1/adapters/hub/files", get(list_hub_files))
        .route("/v1/adapters/hub/install", post(install_hub_adapters))
        .route("/v1/adapters/{id}", axum::routing::patch(update_adapter).delete(delete_adapter))
        .route("/v1/training", get(read_training))
        .route("/v1/training/pack/install", post(install_training_pack))
        .route("/v1/training/pack/cancel", post(cancel_training_pack))
        .route("/v1/training/datasets", post(create_training_dataset))
        // A dataset is gigabytes of lossless audio, far over the studio's usual body limit.
        .route("/v1/training/datasets/import", post(import_training_dataset).layer(DefaultBodyLimit::max(TRAINING_UPLOAD_LIMIT)))
        .route("/v1/training/datasets/{id}/reveal", post(reveal_training_dataset))
        .route("/v1/training/datasets/{id}", axum::routing::patch(update_training_dataset).delete(delete_training_dataset))
        .route("/v1/training/datasets/{id}/songs", post(add_training_songs))
        .route("/v1/training/datasets/{id}/files", post(upload_training_files).layer(DefaultBodyLimit::max(TRAINING_UPLOAD_LIMIT)))
        .route("/v1/training/datasets/{id}/items/{item}", axum::routing::patch(update_training_item).delete(delete_training_item))
        .route("/v1/training/datasets/{id}/items/{item}/describe", post(describe_training_item))
        .route("/v1/training/datasets/{id}/items/{item}/audio", get(training_item_audio))
        .route("/v1/training/datasets/{id}/prepare", post(prepare::start))
        .route("/v1/lyrics/find", post(find_lyrics))
        // a clip of a long song is more than the router's 256 MB
        .route("/v1/videos", post(store_video).layer(DefaultBodyLimit::max(TRAINING_UPLOAD_LIMIT)))
        .route("/v1/writing/guide", get(writing_guide))
        .route("/v1/writing/examples", get(writing_examples))
        .route("/v1/library/songs/{id}/files", get(library_song_files))
        .route("/v1/midi", get(midi_status))
        .route("/v1/midi/notes", get(midi_live_notes))
        .route("/v1/midi/install", post(install_midi))
        .route("/v1/midi/remove", post(remove_midi_model))
        .route("/v1/midi/cancel", post(cancel_midi))
        .route("/v1/midi/transcribe", post(start_midi))
        .route("/v1/library/songs/{id}/midi", get(read_song_midi).delete(delete_song_midi))
        .route("/v1/library/songs/{id}/midi/file", get(song_midi_file))
        .route("/v1/training/datasets/{id}/items/{item}/files", get(dataset_song_files))
        .route("/v1/training/prepare/cancel", post(prepare::cancel))
        .route("/v1/training/prepare/train-after", post(prepare::set_train_after))
        .route("/v1/training/listen/install", post(install_listen_pack))
        .route("/v1/training/runs", post(start_training))
        .route("/v1/training/runs/{id}/cancel", post(cancel_training))
        .route("/v1/training/runs/{id}/continue", post(continue_training))
        .route("/v1/training/runs/{id}", axum::routing::delete(delete_training_run))
        .route("/v1/training/runs/{id}/checkpoints/{step}/install", post(install_training_checkpoint))
        .route("/v1/processing/preview", get(processing_preview))
        .route("/v1/processing/keep", post(keep_processing))
        .route("/v1/processing/discard", post(discard_processing))
        .route("/v1/processing/reference", post(upload_processing_reference))
        .route("/v1/library/songs/{id}/stems", get(read_stems).post(start_separation))
        .route("/v1/library/songs/{id}/stems/{stem}", get(read_stem_audio))
        .route("/v1/library/songs/{id}/cover/auto", post(draw_cover_now))
        .route("/v1/activity", get(read_activity))
        .route("/v1/cover-templates", get(read_cover_templates).put(write_cover_templates))
        .route("/v1/cover-templates/render", post(render_cover_template))
        .route("/v1/openrouter/completions", post(create_openrouter_completion))
        .route("/v1/library/songs", get(library_songs).post(create_library_song))
        .route("/v1/library/import", post(import_library_audio))
        .route("/v1/library/songs/{id}", get(library_song).put(update_library_song).delete(delete_library_song))
        .route("/v1/library/media/{song_id}", get(library_media))
        .route("/v1/library/songs/{id}/cover", get(library_cover).put(store_library_cover))
        .route("/v1/library/playlists", get(library_playlists).post(create_library_playlist))
        .route("/v1/library/playlists/{id}", get(library_playlist).put(update_library_playlist).delete(delete_library_playlist))
        .route("/setup/status", get(setup_status))
        .route("/setup/catalog", get(setup_catalog))
        .route("/setup/download", post(setup_download))
        .route("/setup/remove", post(setup_remove))
        .route("/setup/adopt", post(setup_adopt))
        .route("/v1/open-data-directory", post(open_data_directory))
        .route("/setup/select", post(setup_select))
        .route("/setup/cancel", post(setup_cancel))
        .route("/v1/local-models/music", get(local_music_model_catalog))
        .route("/v1/music/jobs", post(create_music_job).get(list_active_music_jobs))
        .route("/v1/music/replay", post(replay_music_job))
        .route("/v1/ace/plan", post(ace_plan))
        .route("/v1/ace/understand", post(ace_understand))
        .route(
            "/v1/music/jobs/{job_id}",
            get(music_job_status).post(cancel_music_job),
        )
        .with_state(state.clone())
        // Covers and imported audio are megabytes, not kilobytes. The default
        // two-megabyte cap rejected a generated cover by dropping the
        // connection, which reaches the interface as "Failed to fetch".
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024));
    // the MCP tools call the same routes, inside the process
    mcp::install(app.clone());
    let app = app
        // POST only: a GET is answered 405, as the stateless revision asks
        .route("/mcp", post(mcp::handle))
        .route("/mcp/status", get(mcp::status))
        .route("/mcp/window", get(mcp::window_events))
        .route("/mcp/window/result", post(mcp::window_result))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    // The provider catalog is public and small; reading it once at startup
    // means the settings panel is right the first time it is opened, instead
    // of after the user presses a refresh button.
    // Start the engine as soon as a complete set is installed. It takes about
    // three seconds; making the user press a button for it - or worse, wait
    // without knowing what for - is the studio being lazy on their time.
    // The engine is supervised, not started once and forgotten. It used to be
    // launched a single time at startup, and only if a complete set of weights
    // was already on disk - so a first installation downloaded its models,
    // nothing started them, and the window waited on "loading the models into
    // memory" until the studio was restarted by hand. The same gap swallowed a
    // crashed engine. This watches instead: whenever a complete set is on disk
    // and nothing is answering on the engine port, it brings the engine up.
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut complained = false;
            // Whether the engine was up on the last look. Only a fall from up to
            // down is a crash worth a line; an engine that has never started is
            // a startup that is failing, and repeating "stopped answering" every
            // two seconds is what filled a whole log with one sentence.
            let mut was_running = false;
            // The last finished model download seen; the one a previous session
            // left behind is not news.
            let mut last_download: Option<Option<String>> = None;
            loop {
                let status = state.model_manager.status(effective_install_target(&state).await).await;
                let ready = status.ready;
                let running = state.music_server.health().await;
                let finished = status.active.as_ref().filter(|job| matches!(job.status, DownloadStatus::Completed)).map(|job| job.id.clone());
                if last_download.as_ref().is_some_and(|seen| *seen != finished) && finished.is_some() && running {
                    // a model that came down after the engine started is found by a rescan
                    let state = state.clone();
                    tokio::spawn(async move {
                        if let Err(error) = rescan_resources(&state).await {
                            eprintln!("[ERROR] finding the downloaded model: {error}");
                        }
                    });
                }
                last_download = Some(finished);
                // the card has one owner: a preparation or a training run
                // holding it is left alone, and the engine comes back after
                let preparing = state.prepare.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().is_some_and(|job| !job.finished);
                let card_taken = preparing || state.training.active_run().await.is_some();
                // a rescan restarting the engine, or songs still on it, are left alone
                let quiet = state.engine_use.try_write().ok();
                if ready && !running && !card_taken && quiet.is_some() && !state.music_server.health().await {
                    if was_running {
                        // It was answering and now it is not: the one line that
                        // explains a log which suddenly starts again from
                        // "Listening on". Written once, not once a cycle.
                        music_engine::server::note_in_log("the engine stopped answering; restarting it");
                        // Under Auto a device that died of its own fault is
                        // not tried again: the restart moves down the chain.
                        let auto = state.engine_options.read().await.backend == music_engine::server::ComputeBackend::Auto;
                        let active = *state.active_device.read().await;
                        if let Some(device) = active.filter(|device| auto && *device != music_engine::server::ComputeBackend::Cpu) {
                            let log = last_run_log().to_lowercase();
                            if describes_device_failure(&log) || died_silently(&log) {
                                music_engine::server::note_in_log(&format!("{} failed on this machine; leaving it for this session", device_name(device)));
                                state.failed_devices.write().await.push(device);
                            }
                        }
                    }
                    match restart_engine(&state).await {
                        Ok(()) => complained = false,
                        Err(error) => {
                            // Say it once per failure, not once every few
                            // seconds: a card with too little memory would
                            // otherwise fill the log with the same line.
                            if !complained {
                                eprintln!("the local engine did not start: {error}");
                                complained = true;
                            }
                        }
                    }
                }
                drop(quiet);
                was_running = running;
                tokio::time::sleep(std::time::Duration::from_secs(if running { 5 } else { 2 })).await;
            }
        });
    }

    let address = SocketAddr::from(([127, 0, 0, 1], listen_port()));
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("music-server listening on http://{address}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn library_songs(State(state): State<AppState>) -> Result<Json<Vec<library::Song>>, (StatusCode, Json<ApiError>)> { state.library.list_songs().map(Json).map_err(|e|api_error(StatusCode::INTERNAL_SERVER_ERROR,e.to_string())) }
async fn library_song(State(state): State<AppState>,Path(id):Path<String>)->Result<Json<library::Song>,(StatusCode,Json<ApiError>)>{state.library.get_song(&id).map_err(|e|api_error(StatusCode::INTERNAL_SERVER_ERROR,e.to_string()))?.map(Json).ok_or_else(||api_error(StatusCode::NOT_FOUND,"Song not found".into()))}
async fn library_media(State(state): State<AppState>, Path(song_id): Path<String>, headers: HeaderMap) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let song = state.library.get_song(&song_id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?.ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let path = state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song audio is not available in the studio media library".into()))?;
    // Tracks made before the studio tagged anything have no ID3 at all, and a
    // download of one lands in a player as an untitled file. Tag it on the way
    // out, once: the check is three bytes.
    if path.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase).as_deref() == Some("mp3")
        && tokio::fs::read(&path).await.map(|bytes| bytes.get(..3) != Some(b"ID3")).unwrap_or(false)
    {
        tag_stored_song(&state, &song_id).await;
    }
    serve_audio_file(&path, &headers).await
}

/// A dataset song's recording, to listen to while its style is checked.
async fn training_item_audio(State(state): State<AppState>, Path((id, item)): Path<(String, String)>, headers: HeaderMap) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let path = state.training.item_audio(&id, &item).map_err(training_error)?;
    serve_audio_file(&path, &headers).await
}

/// An audio file with single byte-range support, which `<audio>` seeking needs.
async fn serve_audio_file(path: &std::path::Path, headers: &HeaderMap) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let bytes = tokio::fs::read(path).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("read song audio: {error}")))?;
    let content_type = match path.extension().and_then(|extension| extension.to_str()).map(|extension| extension.to_ascii_lowercase()).as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("flac") => "audio/flac",
        _ => return Err(api_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Stored song has an unsupported audio extension".into())),
    };
    let total = bytes.len();
    let range = headers.get(header::RANGE).and_then(|value| value.to_str().ok()).and_then(|value| parse_single_byte_range(value, total));
    let response = if let Some((start, end)) = range {
        axum::response::Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}"))
            .header(header::CONTENT_LENGTH, end - start + 1)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(bytes[start..=end].to_vec()))
    } else if headers.contains_key(header::RANGE) {
        axum::response::Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{total}"))
            .body(Body::empty())
    } else {
        axum::response::Response::builder()
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CONTENT_LENGTH, total)
            .header(header::ACCEPT_RANGES, "bytes")
            .body(Body::from(bytes))
    };
    Ok(response.expect("valid audio response"))
}

/// Parses the one byte-range form used by HTMLAudioElement. Multiple ranges are
/// intentionally declined; a single 206 keeps native seeking interoperable.
fn parse_single_byte_range(value: &str, total: usize) -> Option<(usize, usize)> {
    let value = value.strip_prefix("bytes=")?;
    if value.contains(',') || total == 0 { return None; }
    let (start, end) = value.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<usize>().ok()?;
        if suffix == 0 { return None; }
        return Some((total.saturating_sub(suffix), total - 1));
    }
    let start = start.parse::<usize>().ok()?;
    if start >= total { return None; }
    let end = if end.is_empty() { total - 1 } else { end.parse::<usize>().ok()?.min(total - 1) };
    (end >= start).then_some((start, end))
}
#[derive(Debug, Deserialize)]
struct StoreCoverRequest {
    /// Raw base64 image bytes, without a data-URL prefix.
    image_base64: String,
    media_type: String,
}

/// Cover art is Studio-side metadata: a track keeps working without one, so a
/// missing cover is a 404 the UI answers with its generated placeholder art
/// rather than an error state.
#[derive(Debug, Deserialize)]
struct CoverTemplatesRequest {
    templates: Vec<cover_prompt::CoverTemplate>,
    /// Draw a cover as soon as a track finishes.
    #[serde(default)]
    auto: Option<bool>,
    /// Which of them a new cover starts from. `None` leaves it as it was.
    #[serde(default)]
    default_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RenderCoverPromptRequest {
    template: String,
    /// The track the prompt is for. Without it the placeholders have nothing
    /// to stand in for, which is only useful for previewing the wording.
    song_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    lyrics: Option<String>,
}


/// The waveform editor, carried inside the binary.
///
/// It is a static web application; embedding it keeps the promise that the
/// studio is one executable, and serving it over the studio's own port means
/// the browser can open it with the track already loaded.
static EDITOR: include_dir::Dir<'_> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../app/public/editor");

async fn editor_asset(Path(path): Path<String>) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let file = EDITOR
        .get_file(path.trim_start_matches('/'))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("the editor has no file {path}")))?;
    let media_type = match std::path::Path::new(&path).extension().and_then(|value| value.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("mp3") => "audio/mpeg",
        Some("mp4") => "video/mp4",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    };
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, media_type)
        .header(header::CONTENT_LENGTH, file.contents().len())
        .body(Body::from(file.contents().to_vec()))
        .expect("valid editor response"))
}

/// One separation in progress, as the interface sees it.
#[derive(Debug, Clone, Serialize)]
struct SeparationRun {
    song_id: String,
    /// Between 0 and 1.
    progress: f64,
    done: bool,
    error: Option<String>,
    stems: Vec<String>,
    /// Whether the graphics card did the work, once the run is over.
    used_gpu: Option<bool>,
    /// The library tracks the stems became.
    library_songs: Vec<String>,
}

/// What a track made by a tool says about where it came from: the track it
/// was made from, the tool, and the settings the tool ran with.
fn derivation(from: &library::Song, tool: &str, settings: Value) -> Value {
    serde_json::json!({ "from": from.id, "from_title": from.title, "tool": tool, "settings": settings })
}

/// The track a derived track was made from, and by which tool.
fn derived_from(song: &library::Song) -> Option<(&str, &str)> {
    let derived = song.metadata.get("derived")?;
    Some((derived.get("from")?.as_str()?, derived.get("tool")?.as_str()?))
}

/// Gives a track made by a tool the cover of the one it was made from.
fn cover_like(state: &AppState, from: &library::Song, to: &str) -> anyhow::Result<()> {
    if let Some((path, media_type)) = state.library.cover_path_for_song(from) {
        let image = std::fs::read(&path).with_context(|| format!("read the cover {}", path.display()))?;
        state.library.store_song_cover(to, &image, &media_type)?;
    }
    Ok(())
}

/// Every stem of a song becomes a track of the library made from it: it
/// plays, goes to the tools, makes a clip or a cover like any track, and
/// wears the original's cover. Separating the song again replaces its stems
/// in the library instead of adding more: the new ones are in before the old
/// ones go, so a stem file held open by a player costs nothing but itself.
fn stems_into_library(state: &AppState, song_id: &str, stems: &[String], overlap: f64) -> anyhow::Result<(Vec<String>, Option<String>)> {
    let original = state.library.get_song(song_id)?.ok_or_else(|| anyhow::anyhow!("the song {song_id} is gone"))?;
    let replaced: Vec<library::Song> = state
        .library
        .list_songs()?
        .into_iter()
        .filter(|old| {
            let stem = old.metadata.pointer("/derived/settings/stem").and_then(Value::as_str).unwrap_or_default();
            derived_from(old) == Some((song_id, "stems")) && stems.iter().any(|name| name == stem)
        })
        .collect();
    let mut added = Vec::new();
    for stem in stems {
        let audio = std::fs::read(stem_path(state, song_id, stem)).with_context(|| format!("read the {stem} stem"))?;
        let duration = library::audio_duration_seconds(&audio, "wav", None);
        let song = state.library.import_audio_song(library::AudioImportInput {
            title: format!("{} · {stem}", original.title),
            caption: original.caption.clone(),
            // the words belong to the voice, not to the drums
            lyrics: if stem == "vocals" { original.lyrics.clone() } else { String::new() },
            metadata: serde_json::json!({
                "derived": derivation(&original, "stems", serde_json::json!({ "stem": stem, "model": "HT-Demucs", "overlap": overlap })),
                "duration_seconds": duration,
            }),
            generation_settings: Value::Null,
            engine_id: "stems".into(),
            profile_id: None,
            source: "stems".into(),
            audio_extension: "wav".into(),
            audio,
        })?.song;
        cover_like(state, &original, &song.id)?;
        added.push(song.id);
    }
    let mut kept = Vec::new();
    for old in replaced {
        let files = song_files(state, &old);
        state.library.delete_song(&old.id)?;
        for path in files {
            if let Err(error) = std::fs::remove_file(&path) {
                kept.push(format!("{}: {error}", path.display()));
            }
        }
    }
    let problem = (!kept.is_empty()).then(|| format!("the new stems are in the library, but files of the old ones could not be removed: {}", kept.join("; ")));
    Ok((added, problem))
}

/// Where a song's stems live: beside the track, named after it.
fn stem_path(state: &AppState, song_id: &str, stem: &str) -> PathBuf {
    state.library.media_dir().join(format!("{song_id}-{stem}.wav"))
}

fn stems_on_disk(state: &AppState, song_id: &str) -> Vec<String> {
    separation::STEMS
        .iter()
        .filter(|stem| stem_path(state, song_id, stem).is_file())
        .map(|stem| (*stem).to_string())
        .collect()
}

/// The separator as an optional module: its files, and whichever download is
/// running. The same envelope the assistant and karaoke use, because the models
/// page lists all three the same way.
async fn separation_assets(State(state): State<AppState>) -> Json<Value> {
    let runtime_installed = state.lyrics_sync.onnxruntime_library().is_some();
    let assets = serde_json::json!([
        {
            "id": separation::MODEL.id,
            "label": separation::MODEL.label,
            "bytes": separation::MODEL.bytes,
            "note": separation::MODEL.note,
            "installed": state.separator.is_installed(),
        },
        {
            "id": "onnxruntime-cuda",
            "label": "ONNX Runtime 1.30.0 · CUDA",
            "bytes": 379_723_801u64,
            "note": "The CUDA build of the runtime.",
            "installed": state.lyrics_sync.has_cuda_runtime(),
        },
        {
            "id": "cuda-cublas",
            "label": "NVIDIA cuBLAS 12.9",
            "bytes": 549_731_131u64,
            "note": "The linear algebra the CUDA provider is built on.",
            "installed": state.lyrics_sync.downloader().runtime_dir("onnx-cuda").join("cublasLt64_12.dll").is_file(),
        },
        {
            "id": "cuda-cudart",
            "label": "NVIDIA CUDA runtime 12.9",
            "bytes": 3_521_238u64,
            "note": "The CUDA runtime itself.",
            "installed": state.lyrics_sync.downloader().runtime_dir("onnx-cuda").join("cudart64_12.dll").is_file(),
        },
        {
            "id": "cuda-cudnn",
            "label": "NVIDIA cuDNN 9.25",
            "bytes": 1_904_452_100u64,
            "note": "The convolution kernels the separator spends its time in.",
            "installed": state.lyrics_sync.downloader().runtime_dir("onnx-cuda").join("cudnn64_9.dll").is_file(),
        },
        {
            "id": "onnxruntime",
            "label": "ONNX Runtime 1.30.0",
            "bytes": 82_645_522,
            "note": "Runs the separator and the karaoke recogniser; shared between them.",
            "installed": runtime_installed,
        }
    ]);
    let config = state.separation_config.read().await.clone();
    let mut set: Vec<&'static lyrics_sync::Asset> = Vec::new();
    if let Some(asset) = lyrics_sync::asset("onnxruntime") { set.push(asset); }
    if !matches!(config.runtime, lyrics_sync::OnnxFlavour::Cpu) {
        set.extend(CARD_ASSETS.iter().filter_map(|id| lyrics_sync::asset(id)));
    }
    let runtime_progress = set_progress(state.lyrics_sync.downloader(), &set);
    let model_installed = state.separator.is_installed();
    let bytes = runtime_progress["bytes"].as_u64().unwrap_or(0) + separation::MODEL.bytes;
    let installed_bytes = runtime_progress["installed_bytes"].as_u64().unwrap_or(0)
        + if model_installed { separation::MODEL.bytes } else { 0 };
    Json(serde_json::json!({
        "assets": assets,
        "settings": { "runtime": config.runtime },
        "set": {
            "bytes": bytes,
            "installed_bytes": installed_bytes,
            "ready": installed_bytes == bytes,
            "files": set.len() + 1,
        },
        // Only this panel's own download. The recogniser shares this
        // downloader, and its gigabytes are not the separator's business.
        "active_download": state.separator.downloader().active_for("separation").await.or(state.lyrics_sync.downloader().active_for("separation").await),
        // Not an error and not a stall: the file server is asking us to wait.
        "waiting_for_server": crate::chunked::waiting_for_server(),
    }))
}

#[derive(Debug, Deserialize)]
struct InstallSeparationAssetRequest {
    asset_id: String,
}

/// Everything the card path needs, in the order it is used. `karaoke_set`
/// builds a recogniser out of these plus its own model files, so this is the
/// one place the CUDA provider's parts are named.
const CARD_ASSETS: [&str; 5] = ["onnxruntime-cuda", "cuda-cudart", "cuda-cublas", "cuda-cufft", "cuda-cudnn"];

/// Stops a download. What arrived stays on disk: pressing this again later
/// carries on from the last finished piece rather than starting the file over.
///
/// Two panels can be downloading at once, and until this existed the only way
/// to stop one of them was to close the studio.
/// Points `ort` at the ONNX Runtime the studio installed, once per process.
///
/// It has to happen before anything touches `ort`, or the library binds to
/// whatever `onnxruntime.dll` the system happens to have - and on Windows a
/// DLL's own dependencies are resolved through the process search path, not
/// through the folder it came from, which is why the directory joins PATH too.
/// Every caller needs this, not only the separator: reading a track through the
/// same runtime without it left the request waiting on a library that was never
/// located.
fn point_ort_at(runtime: &std::path::Path) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let runtime = runtime.to_path_buf();
    ONCE.call_once(|| unsafe {
        std::env::set_var("ORT_DYLIB_PATH", &runtime);
        if let Some(directory) = runtime.parent() {
            let existing = std::env::var("PATH").unwrap_or_default();
            std::env::set_var("PATH", format!("{};{existing}", directory.display()));
        }
    });
}

async fn cancel_separation_download(State(state): State<AppState>) -> Json<Value> {
    state.separator.downloader().cancel();
    state.lyrics_sync.downloader().cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

async fn cancel_karaoke_download(State(state): State<AppState>) -> Json<Value> {
    state.lyrics_sync.downloader().cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

/// Frees the disk an assistant takes: the model, the runtime and any half of
/// either. Every other capability could be removed from its panel; this one
/// could only be added.
async fn assistant_runtime_remove(
    State(state): State<AppState>,
    Json(request): Json<AssistantAssetRequest>,
) -> Result<Json<assistant_runtime::RuntimeStatus>, (StatusCode, Json<ApiError>)> {
    // "managed" from the panel means the whole thing: whichever llama.cpp build
    // is on disk, and the model that was chosen with it.
    let ids: Vec<String> = if request.asset_id == "managed" || request.asset_id == "cuda" || request.asset_id == "cpu" {
        let chosen = state.assistant.read().await.managed_model.clone();
        ["llama-cuda", "llama-cuda-runtime", "llama-cpu"].iter().map(|id| id.to_string()).chain(chosen).collect()
    } else {
        vec![request.asset_id.clone()]
    };
    for id in &ids {
        state
            .assistant_runtime
            .remove(id)
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    }
    {
        let mut assistant = state.assistant.write().await;
        assistant.managed_model = None;
    }
    let _ = persist_studio_settings(&state).await;
    Ok(Json(state.assistant_runtime.status().await))
}

async fn cancel_assistant_download(State(state): State<AppState>) -> Json<Value> {
    state.assistant_runtime.cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

async fn install_separation_asset(
    State(state): State<AppState>,
    Json(request): Json<InstallSeparationAssetRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    // "card" means whatever is still missing for the graphics card, one after
    // another: asking someone to press four buttons in the right order is not a
    // setup, it is a quiz.
    // The separator as one thing: its model, the runtime that loads it, and -
    // for the card - the CUDA provider. Six rows of file names asked the user
    // to work out which of them belong together.
    if matches!(request.asset_id.as_str(), "auto" | "cuda" | "cpu") {
        let card = !matches!(request.asset_id.as_str(), "cpu");
        let separator = state.separator.clone();
        let sync = state.lyrics_sync.clone();
        let mut runtime: Vec<&'static lyrics_sync::Asset> = Vec::new();
        if let Some(asset) = lyrics_sync::asset("onnxruntime") { runtime.push(asset); }
        if card {
            runtime.extend(CARD_ASSETS.iter().filter_map(|id| lyrics_sync::asset(id)));
        }
        tokio::spawn(async move {
            if let Err(error) = separator.downloader().install_all("separation", &[&separation::MODEL]).await {
                eprintln!("the separator model could not be installed: {error}");
                return;
            }
            if let Err(error) = sync.downloader().install_all("separation", &runtime).await {
                eprintln!("the separator runtime could not be installed: {error}");
            }
        });
        return Ok(Json(serde_json::json!({ "started": true })));
    }
    if request.asset_id == "card" {
        let sync = state.lyrics_sync.clone();
        let card: Vec<&'static lyrics_sync::Asset> = CARD_ASSETS.iter().filter_map(|id| lyrics_sync::asset(id)).collect();
        tokio::spawn(async move {
            if let Err(error) = sync.downloader().install_all("separation", &card).await {
                eprintln!("the card path could not be installed: {error}");
            }
        });
        return Ok(Json(serde_json::json!({ "started": true })));
    }
    // Anything in the catalogue may be installed by name; listing the ids here
    // by hand is how cuFFT ended up silently rejected.
    // `install` starts a background task and returns immediately; its Result
    // says whether the download was accepted at all. Discarding it inside a
    // spawn - which is what this did - meant a refusal ("another download is
    // already running") was thrown away while the endpoint answered
    // "started: true". The button then did nothing, twice, silently, with a
    // successful reply, and no amount of error handling in the interface could
    // have shown it.
    if request.asset_id == separation::MODEL.id {
        state
            .separator
            .downloader()
            .install(&separation::MODEL)
            .await
            .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    } else if let Some(asset) = lyrics_sync::asset(&request.asset_id) {
        state
            .lyrics_sync
            .downloader()
            .install(asset)
            .await
            .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    } else {
        return Err(api_error(StatusCode::BAD_REQUEST, format!("unknown asset {}", request.asset_id)));
    }
    Ok(Json(serde_json::json!({ "started": true })))
}

async fn read_separation_settings(State(state): State<AppState>) -> Json<separation::SeparationConfig> {
    Json(state.separation_config.read().await.clone())
}

async fn write_separation_settings(
    State(state): State<AppState>,
    Json(config): Json<separation::SeparationConfig>,
) -> Result<Json<separation::SeparationConfig>, (StatusCode, Json<ApiError>)> {
    // A run that writes nothing is a run nobody wanted; an empty choice means
    // everything, which is also what the studio starts with.
    let stems: Vec<String> = if config.stems.is_empty() {
        separation::STEMS.iter().map(|stem| (*stem).to_string()).collect()
    } else {
        config.stems.iter().filter(|stem| separation::STEMS.contains(&stem.as_str())).cloned().collect()
    };
    let stored = separation::SeparationConfig { runtime: config.runtime, stems, overlap: config.sane_overlap() };
    *state.separation_config.write().await = stored.clone();
    persist_studio_settings(&state)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(stored))
}

async fn separation_status(State(state): State<AppState>) -> Json<Value> {
    let runtime = state.lyrics_sync.onnxruntime_library();
    Json(serde_json::json!({
        "model": {
            "id": separation::MODEL.id,
            "label": separation::MODEL.label,
            "bytes": separation::MODEL.bytes,
            "note": separation::MODEL.note,
            "installed": state.separator.is_installed(),
        },
        "runtime_installed": runtime.is_some(),
        "cuda_runtime_installed": state.lyrics_sync.has_cuda_libraries(),
        "card_missing_bytes": CARD_ASSETS
            .iter()
            .filter_map(|id| lyrics_sync::asset(id))
            .filter(|asset| !state.lyrics_sync.downloader().is_installed(asset))
            .map(|asset| asset.bytes)
            .sum::<u64>(),
        "ready": state.separator.ready(runtime.as_deref()),
        "stems": separation::STEMS,
        // Either downloader may be the busy one: the model has its own, the
        // card libraries come through karaoke's. Reporting only the first is
        // what made a running download look like a dead button.
        "download": match state.separator.downloader().active().await {
            Some(active) if !active.done => Some(active),
            other => match state.lyrics_sync.downloader().active().await {
                Some(active) if !active.done => Some(active),
                fallback => fallback.or(other),
            },
        },
        "settings": state.separation_config.read().await.clone(),
        "run": state.separation_run.read().await.clone(),
    }))
}

/// Fetches the separation model. Nothing here downloads on its own; this is the
/// button, and it also brings the runtime if karaoke has not already.
async fn install_separation_model(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let separator = state.separator.clone();
    let sync = state.lyrics_sync.clone();
    tokio::spawn(async move {
        if sync.onnxruntime_library().is_none() {
            if let Some(runtime) = lyrics_sync::asset("onnxruntime") {
                let _ = sync.downloader().install(runtime).await;
            }
        }
        let _ = separator.downloader().install(&separation::MODEL).await;
    });
    Ok(Json(serde_json::json!({ "started": true })))
}

/// The adapter page: the parts of the model an adapter can change, the
/// installed adapters with what the engine found in each, the catalogue, and
/// the download in progress. The engine is asked only when it is already up;
/// a stopped engine leaves the slots the studio remembered.
async fn list_adapters(State(state): State<AppState>) -> Json<Value> {
    let slot_ids: Vec<&str> = music_engine::model::ADAPTER_SLOTS.iter().map(|slot| slot.id).collect();
    let views = match tokio::time::timeout(std::time::Duration::from_secs(3), state.music_server.props()).await {
        Ok(Ok(props)) => Some(adapters::engine_views(&props, &slot_ids)),
        _ => None,
    };
    Json(serde_json::json!({
        "slots": music_engine::model::ADAPTER_SLOTS,
        "installed": state.adapters.installed(views.as_ref()),
        "catalog": state.adapters.offered(),
        "model": selected_model_files(&state).await.ok().and_then(|files| music_engine::model::model_family(&files.synth_model)),
        "engine_checked": views.is_some(),
        "download": state.adapters.downloader().active_for(adapters::SCOPE).await,
        "installing": state.adapters.installing(),
    }))
}

#[derive(Debug, Deserialize)]
struct InstallAdaptersRequest {
    /// Catalogue entries to fetch as one download.
    ids: Vec<String>,
}

async fn install_catalog_adapters(
    State(state): State<AppState>,
    Json(input): Json<InstallAdaptersRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    state.adapters.begin_install(&input.ids).map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(serde_json::json!({ "started": true })))
}

#[derive(Debug, Deserialize)]
struct HubQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    repo: String,
}

async fn search_hub_adapters(State(state): State<AppState>, Query(query): Query<HubQuery>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let found = state.adapters.hub_search(&sizes::client(), &query.q).await.map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "repos": found })))
}

/// The weight files of a repository; `repo` may be an id or any link into it,
/// and a link to one file comes back with that file named.
async fn list_hub_files(State(state): State<AppState>, Query(query): Query<HubQuery>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let (repo, file) = adapters::hub_reference(&query.repo)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "that is not a Hugging Face repository or file link".into()))?;
    let listing = state.adapters.hub_files(&sizes::client(), &repo).await.map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "listing": listing, "file": file })))
}

#[derive(Debug, Deserialize)]
struct InstallHubRequest {
    repo: String,
    paths: Vec<String>,
}

async fn install_hub_adapters(State(state): State<AppState>, Json(input): Json<InstallHubRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    state
        .adapters
        .begin_hub_install(&sizes::client(), &input.repo, &input.paths)
        .await
        .map_err(|error| api_error(StatusCode::CONFLICT, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "started": true })))
}

async fn cancel_adapter_download(State(state): State<AppState>) -> Json<Value> {
    state.adapters.downloader().cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

/// Stores uploaded adapter files: one or more `.safetensors`, and the
/// `adapter_config.json` or `lora.json` that came with them.
async fn import_adapter(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<adapters::AdapterMeta>), (StatusCode, Json<ApiError>)> {
    let mut name = String::new();
    let mut files = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read adapter form: {e}")))? {
        match (field.name().unwrap_or_default().to_owned(), field.file_name().map(str::to_owned)) {
            (key, Some(file)) if key == "files" => {
                let bytes = field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read {file}: {e}")))?;
                files.push((file, bytes.to_vec()));
            }
            (key, _) if key == "name" => {
                name = field.text().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read adapter name: {e}")))?;
            }
            _ => {}
        }
    }
    if name.trim().is_empty() {
        name = files
            .iter()
            .find(|(file, _)| file.ends_with(".safetensors"))
            .and_then(|(file, _)| std::path::Path::new(file).file_stem().and_then(|stem| stem.to_str()).map(str::to_owned))
            .unwrap_or_else(|| "Adapter".into());
    }
    let meta = state
        .adapters
        .import(&name, files, adapters::Origin::Imported)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok((StatusCode::CREATED, Json(meta)))
}

async fn update_adapter(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<adapters::Patch>,
) -> Result<Json<adapters::AdapterMeta>, (StatusCode, Json<ApiError>)> {
    state.adapters.update(&id, patch).map(Json).map_err(|e| api_error(StatusCode::NOT_FOUND, e.to_string()))
}

async fn delete_adapter(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    state.adapters.remove(&id).map_err(|e| api_error(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Starts processing a track into a preview. One run at a time: the stages are
/// quick, and a second request would only race the first for the preview.
async fn start_processing(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<processing::ProcessRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    if request.stages().is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "choose at least one kind of processing".into()));
    }
    let song = state
        .library
        .get_song(&id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let source = state
        .library
        .media_path_for_song(&song)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "this track has no stored audio".into()))?;
    let media = state.library.media_dir().to_path_buf();
    let reference = match &request.master {
        None => None,
        Some(processing::MasterSource::Song { song_id }) => {
            let reference_song = state
                .library
                .get_song(song_id)
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the reference track is not in the library".into()))?;
            Some(
                state
                    .library
                    .media_path_for_song(&reference_song)
                    .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "the reference track has no stored audio".into()))?,
            )
        }
        Some(processing::MasterSource::Upload { upload_id }) => Some(
            processing::workspace_file(&media, upload_id)
                .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "the uploaded reference is gone; upload it again".into()))?,
        ),
    };

    let run_id = uuid::Uuid::now_v7().simple().to_string();
    {
        // checked and claimed under one lock, so two requests cannot both start
        let mut current = state.processing_run.write().await;
        if current.as_ref().is_some_and(|run| !run.done) {
            return Err(api_error(StatusCode::CONFLICT, "a track is already being processed".into()));
        }
        // a new run replaces the last preview nobody kept; its reference stays
        // when this run masters to the same upload
        if let Some(previous) = current.take() {
            let keep = reference.clone();
            for file in previous.leftovers(&media).into_iter().filter(|file| Some(file) != keep.as_ref()) {
                let _ = std::fs::remove_file(file);
            }
        }
        *current = Some(processing::ProcessRun {
            id: run_id.clone(),
            song_id: id.clone(),
            stages: request.stages(),
            stage: None,
            done: false,
            error: None,
            preview: None,
            preview_ready: false,
            request: request.clone(),
        });
    }

    let background = state.clone();
    tokio::task::spawn_blocking(move || {
        let handle = tokio::runtime::Handle::current();
        let current = |run: &Option<processing::ProcessRun>| run.as_ref().is_some_and(|run| run.id == run_id);
        let outcome = (|| -> anyhow::Result<std::path::PathBuf> {
            let vst = studio_data_root().and_then(|root| vst::VstHost::locate(&root));
            let audio = processing::run(&source, reference.as_deref(), &request, vst.as_ref(), |stage| {
                let state = background.clone();
                let run_id = run_id.clone();
                handle.spawn(async move {
                    if let Some(run) = state.processing_run.write().await.as_mut().filter(|run| run.id == run_id) {
                        run.stage = Some(stage);
                    }
                });
            })?;
            let folder = processing::workspace(&media);
            std::fs::create_dir_all(&folder)?;
            let path = folder.join(format!("{id}-{}.wav", &run_id[run_id.len() - 8..]));
            audio_pcm::write_wav24(&path, &audio)?;
            Ok(path)
        })();
        handle.block_on(async {
            let mut guard = background.processing_run.write().await;
            if !current(&guard) {
                // discarded while it worked: nothing will ever ask for the preview
                if let Ok(path) = &outcome {
                    let _ = std::fs::remove_file(path);
                }
                return;
            }
            let run = guard.as_mut().expect("checked above");
            run.done = true;
            match outcome {
                Ok(path) => {
                    run.preview = path.file_name().and_then(|name| name.to_str()).map(str::to_owned);
                    run.preview_ready = run.preview.is_some();
                }
                Err(error) => run.error = Some(format!("{error:#}")),
            }
        });
    });
    Ok(Json(serde_json::json!({ "started": true })))
}

/// The VST host and the plugins its last scan found; `available` is false
/// when the host did not ship with this build.
async fn read_vst() -> Json<Value> {
    let host = studio_data_root().and_then(|root| vst::VstHost::locate(&root));
    Json(serde_json::json!({ "available": host.is_some(), "plugins": host.and_then(|host| host.plugins()) }))
}

async fn scan_vst() -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let host = studio_data_root()
        .and_then(|root| vst::VstHost::locate(&root))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the VST host is not installed with this studio".into()))?;
    let plugins = tokio::task::spawn_blocking(move || host.scan())
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "available": true, "plugins": plugins })))
}

#[derive(Debug, Deserialize)]
struct VstEditorRequest {
    path: String,
    #[serde(default)]
    state_id: Option<String>,
}

/// Opens a plugin's window; the settings save into the slot's state when it closes.
async fn open_vst_editor(Json(input): Json<VstEditorRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let host = studio_data_root()
        .and_then(|root| vst::VstHost::locate(&root))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the VST host is not installed with this studio".into()))?;
    let state_id = host.open_editor(&input.path, input.state_id).map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "state_id": state_id })))
}

async fn read_processing(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({ "run": state.processing_run.read().await.clone() }))
}

async fn processing_preview(State(state): State<AppState>, headers: HeaderMap) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let name = state.processing_run.read().await.as_ref().and_then(|run| run.preview.clone());
    let path = name
        .and_then(|name| processing::workspace_file(state.library.media_dir(), &name))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "there is no processed preview".into()))?;
    serve_audio_file(&path, &headers).await
}

#[derive(Debug, Deserialize)]
struct KeepProcessingRequest {
    /// What the processing is called on the new track; its stages when left out.
    #[serde(default)]
    label: String,
}

/// Keeps the preview as a version of its track, playing from now on.
async fn keep_processing(
    State(state): State<AppState>,
    Json(input): Json<KeepProcessingRequest>,
) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    let run = state.processing_run.read().await.clone().filter(|run| run.preview_ready);
    let run = run.ok_or_else(|| api_error(StatusCode::NOT_FOUND, "there is no processed preview to keep".into()))?;
    let media = state.library.media_dir().to_path_buf();
    let preview = run
        .preview
        .as_deref()
        .and_then(|name| processing::workspace_file(&media, name))
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "the preview file is gone".into()))?;
    let filename = format!("{}-v{}-{}.wav", run.song_id, &uuid::Uuid::now_v7().simple().to_string()[..8], run.stages.join("-"));
    let stored = media.join(&filename);
    std::fs::rename(&preview, &stored).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("store the version: {error}")))?;
    let reference_title = match &run.request.master {
        Some(processing::MasterSource::Song { song_id }) => state.library.get_song(song_id).ok().flatten().map(|song| song.title),
        _ => None,
    };
    let settings = processing::settings_record(&run.request, reference_title.as_deref());
    // the result is a track of its own, made from the one it processed
    let recorded = (|| -> anyhow::Result<library::Song> {
        let original = state.library.get_song(&run.song_id)?.ok_or_else(|| anyhow::anyhow!("Song not found"))?;
        let audio = std::fs::read(&stored).with_context(|| format!("read {}", stored.display()))?;
        let label = if input.label.trim().is_empty() { run.stages.join(" + ") } else { input.label.trim().to_string() };
        let mut settings = settings;
        settings["label"] = Value::from(label.clone());
        let song = state.library.create_song(library::SongInput {
            title: format!("{} · {label}", original.title),
            audio_path: Some(stored.display().to_string()),
            caption: original.caption.clone(),
            lyrics: original.lyrics.clone(),
            metadata: serde_json::json!({
                "derived": derivation(&original, "processing", settings),
                "duration_seconds": library::audio_duration_seconds(&audio, "wav", None),
            }),
            generation_settings: Value::Null,
            engine_id: "processing".into(),
            profile_id: None,
            replay_request: None,
            audio_codes: None,
            source: "processing".into(),
        })?;
        cover_like(&state, &original, &song.id)?;
        Ok(song)
    })()
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")));
    let song = match recorded {
        Ok(song) => song,
        Err(problem) => {
            // the preview goes back where it was, so the run can still be kept or discarded
            if let Err(error) = std::fs::rename(&stored, &preview) {
                eprintln!("[ERROR] processing: return {} to the workspace: {error}", stored.display());
            }
            return Err(problem);
        }
    };
    let mut current = state.processing_run.write().await;
    if current.as_ref().is_some_and(|now| now.id == run.id) {
        for file in current.take().map(|run| run.leftovers(&media)).unwrap_or_default() {
            let _ = std::fs::remove_file(file);
        }
    }
    Ok(Json(song))
}

/// Forgets the run, finished or not. A worker still going sees it is no longer
/// current and removes its own output.
async fn discard_processing(State(state): State<AppState>) -> Json<Value> {
    if let Some(run) = state.processing_run.write().await.take() {
        for file in run.leftovers(state.library.media_dir()) {
            let _ = std::fs::remove_file(file);
        }
    }
    Json(serde_json::json!({ "discarded": true }))
}

/// Stores a reference recording for mastering. It waits in the processing
/// folder, not the library: a reference is a tool, not a song.
async fn upload_processing_reference(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    while let Some(field) = multipart.next_field().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read the upload: {e}")))? {
        if field.name() != Some("audio") {
            continue;
        }
        let original = field.file_name().unwrap_or("reference").to_owned();
        let extension = std::path::Path::new(&original)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|value| matches!(value.as_str(), "mp3" | "wav" | "flac" | "ogg" | "m4a"))
            .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "the reference must be MP3, WAV, FLAC, OGG or M4A".into()))?;
        let bytes = field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read the upload: {e}")))?;
        let folder = processing::workspace(state.library.media_dir());
        std::fs::create_dir_all(&folder).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let upload_id = format!("reference-{}.{extension}", uuid::Uuid::now_v7().simple());
        std::fs::write(folder.join(&upload_id), &bytes).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        return Ok(Json(serde_json::json!({ "upload_id": upload_id, "name": original })));
    }
    Err(api_error(StatusCode::BAD_REQUEST, "no audio part in the upload".into()))
}

#[derive(Debug, Deserialize)]
struct SelectVersionRequest {
    /// `original`, or the id of one of the track's versions.
    version: String,
}

async fn select_song_version(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<SelectVersionRequest>,
) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    let song = state
        .library
        .select_song_version(&id, &input.version)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    tag_stored_song(&state, &id).await;
    Ok(Json(song))
}

async fn remove_song_version(
    State(state): State<AppState>,
    Path((id, version)): Path<(String, String)>,
) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    let (song, file) = state
        .library
        .remove_song_version(&id, &version)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    if let Some(file) = file {
        if let Err(error) = std::fs::remove_file(&file) {
            eprintln!("[ERROR] remove version {version} of {id}: {error}");
        }
    }
    Ok(Json(song))
}

/// The studio's separator, for the vocals lyric recognition hears.
struct StudioSeparator {
    model: PathBuf,
    runtime: PathBuf,
    overlap: f64,
    on_gpu: bool,
    /// Loaded on the first song and kept for the rest: one separator is made
    /// per run of songs and dropped with it, taking the card's memory along.
    loaded: std::sync::Mutex<Option<separation::Loaded>>,
}

impl StudioSeparator {
    fn separate(&self, mix: &std::path::Path, out: &std::path::Path) -> anyhow::Result<()> {
        point_ort_at(&self.runtime);
        let audio = audio_pcm::decode_stereo_44k(mix)?;
        let mut loaded = self.loaded.lock().map_err(|_| anyhow::anyhow!("the separator failed on an earlier song"))?;
        if loaded.is_none() {
            *loaded = Some(separation::load(&self.model, self.on_gpu)?);
        }
        let model = loaded.as_mut().expect("loaded above");
        let separated = separation::separate_with(model, &audio, separation::STEMS.len(), self.overlap, |_| {})?;
        let vocals = separated.stems.into_iter().find(|stem| stem.name == "vocals").context("the separator returned no vocals")?;
        separation::write_wav_stereo(out, &vocals.samples)
    }
}

/// The ONNX Runtime build for the card when its libraries are there. The
/// process binds one build on first use, so work that wants the card points
/// at this one before anything else runs.
fn preferred_onnx_runtime(state: &AppState) -> Option<PathBuf> {
    state
        .lyrics_sync
        .onnxruntime_library_of(if state.lyrics_sync.has_cuda_libraries() { lyrics_sync::OnnxFlavour::Cuda } else { lyrics_sync::OnnxFlavour::Cpu })
        .or_else(|| state.lyrics_sync.onnxruntime_library())
}

/// The separator when its model and runtime are installed; the runtime is
/// chosen the way a song's separation chooses it.
async fn vocal_separator(state: &AppState) -> Option<Arc<StudioSeparator>> {
    let runtime = preferred_onnx_runtime(state)?;
    if !state.separator.is_installed() {
        return None;
    }
    let config = state.separation_config.read().await.clone();
    Some(Arc::new(StudioSeparator {
        model: state.separator.model_path(),
        runtime,
        overlap: config.sane_overlap(),
        on_gpu: !matches!(config.runtime, lyrics_sync::OnnxFlavour::Cpu) && state.lyrics_sync.has_cuda_libraries(),
        loaded: std::sync::Mutex::new(None),
    }))
}

/// What one upload of songs to a dataset may weigh.
const TRAINING_UPLOAD_LIMIT: usize = 16 * 1024 * 1024 * 1024;

fn training_error(error: anyhow::Error) -> (StatusCode, Json<ApiError>) {
    api_error(StatusCode::BAD_REQUEST, format!("{error:#}"))
}

/// The training page: what is installed, the datasets, the runs, and what the
/// run in progress is doing.
async fn read_training(State(state): State<AppState>) -> Json<Value> {
    let training = &state.training;
    let active = training.active_run().await;
    let runs: Vec<Value> = training
        .runs()
        .into_iter()
        .map(|run| {
            let checkpoints = training.checkpoints(&run.id);
            let mut value = serde_json::to_value(&run).unwrap_or(Value::Null);
            value["checkpoints"] = serde_json::json!(checkpoints.iter().map(|checkpoint| checkpoint.step).collect::<Vec<_>>());
            // where "train further" starts, or why it cannot; a run going now has neither
            if run.status != training::RunStatus::Running {
                match training.resume_point(&run.id) {
                    Ok((step, _)) => value["resume_step"] = serde_json::json!(step),
                    Err(reason) => value["resume_refused"] = serde_json::json!(reason),
                }
            }
            if active.as_deref() == Some(run.id.as_str()) || run.status == training::RunStatus::Failed {
                value["log"] = serde_json::json!(training.log_tail(&run.id, 12));
            }
            value
        })
        .collect();
    let pack = training.pack_status();
    let training_download = training.downloader().active_for(training::SCOPE).await;
    let listen_download = training.downloader().active_for(training::LISTEN_SCOPE).await.filter(|active| !active.done);
    Json(serde_json::json!({
        "pack": pack,
        "pack_ready": training.pack_ready(),
        "recipe_defaults": training::Recipe::default(),
        "recipe_fields": music_engine::train::recipe_fields(),
        "min_vram_gb": music_engine::train::MIN_VRAM_GB,
        "progress_unit": music_engine::train::PROGRESS_UNIT,
        // the DiT a new run trains on: the BF16 of the one the studio renders with
        "base": training_weights(&state).await.ok().map(|weights| serde_json::json!({
            "dit": weights.base.dit,
            "installed": weights.installed,
            "bytes": weights.bytes,
            "download": weights.download,
        })),
        "item_style": "caption",
        "download": training_download,
        "listen": {
            "pack": training.listen_status(),
            "ready": training.listen_ready() && state.lyrics_sync.onnxruntime_library().is_some(),
            "download": listen_download,
        },
        "datasets": training.datasets(),
        "runs": runs,
        "active": active,
        "prepare": state.prepare.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone(),
    }))
}

async fn install_training_pack(State(state): State<AppState>) -> Json<Value> {
    let background = state.clone();
    tokio::spawn(async move {
        if let Err(error) = background.training.install_pack().await {
            eprintln!("[ERROR] training pack: {error:#}");
        }
    });
    Json(serde_json::json!({ "started": true }))
}

/// The optional listening pack, and the ONNX Runtime its tempo and key
/// models run on when nothing else has brought it yet.
async fn install_listen_pack(State(state): State<AppState>) -> Json<Value> {
    let background = state.clone();
    tokio::spawn(async move {
        if let Err(error) = background.training.install_listen().await {
            eprintln!("[ERROR] listening pack: {error:#}");
            return;
        }
        if background.lyrics_sync.onnxruntime_library().is_none() {
            let runtime: Vec<&'static lyrics_sync::Asset> = lyrics_sync::asset("onnxruntime").into_iter().collect();
            if let Err(error) = background.lyrics_sync.downloader().install_all("listen", &runtime).await {
                eprintln!("[ERROR] ONNX Runtime for the listening pack: {error:#}");
            }
        }
    });
    Json(serde_json::json!({ "started": true }))
}

async fn cancel_training_pack(State(state): State<AppState>) -> Json<Value> {
    state.training.downloader().cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

#[derive(Debug, Deserialize)]
struct DatasetInput {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    trigger: Option<String>,
}

async fn create_training_dataset(State(state): State<AppState>, Json(input): Json<DatasetInput>) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    state.training.create_dataset(input.name.as_deref().unwrap_or_default(), input.trigger.as_deref().unwrap_or_default()).map(Json).map_err(training_error)
}

/// Takes a dataset folder uploaded from another studio: its dataset.json and
/// the audio beside it.
/// A folder of uploaded files, removed however the request ends.
struct UploadFolder(std::path::PathBuf);

impl UploadFolder {
    fn new() -> Result<Self, (StatusCode, Json<ApiError>)> {
        let folder = std::env::temp_dir().join(format!("training-upload-{}", uuid::Uuid::now_v7().simple()));
        std::fs::create_dir_all(&folder).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        Ok(Self(folder))
    }
}

impl Drop for UploadFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Writes one uploaded file to disk as it arrives, so a dataset of several
/// gigabytes never sits in memory.
async fn save_upload(mut field: axum::extract::multipart::Field<'_>, path: &std::path::Path) -> Result<(), (StatusCode, Json<ApiError>)> {
    use tokio::io::AsyncWriteExt;
    let name = field.file_name().unwrap_or_default().to_owned();
    let mut file = tokio::fs::File::create(path).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    while let Some(chunk) = field.chunk().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read {name}: {e}")))? {
        file.write_all(&chunk).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    }
    file.flush().await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(())
}

async fn import_training_dataset(State(state): State<AppState>, mut multipart: Multipart) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    let folder = UploadFolder::new()?;
    let mut manifest = None;
    let mut files = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read the upload: {e}")))? {
        let Some(name) = field.file_name().map(str::to_owned) else { continue };
        let base = name.rsplit(['/', '\\']).next().unwrap_or_default().to_owned();
        if base == "dataset.json" {
            let bytes = field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read {name}: {e}")))?;
            manifest = Some(bytes.to_vec());
        } else if base.to_ascii_lowercase().ends_with(".wav") {
            let path = folder.0.join(format!("{}.wav", files.len()));
            save_upload(field, &path).await?;
            files.push((base, path));
        }
    }
    let manifest = manifest.ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "the folder has no dataset.json".into()))?;
    let training = state.training.clone();
    let outcome = tokio::task::spawn_blocking(move || training.import_dataset(&manifest, &files)).await;
    drop(folder);
    outcome.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?.map(Json).map_err(training_error)
}

/// Opens a dataset's folder in the file manager, to copy it to another studio.
async fn reveal_training_dataset(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let folder = state.training.dataset_folder(&id).map_err(training_error)?;
    #[cfg(windows)]
    let opened = std::process::Command::new("explorer.exe").arg(&folder).spawn();
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("open").arg(&folder).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let opened = std::process::Command::new("xdg-open").arg(&folder).spawn();
    opened.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({ "opened": folder.display().to_string() })))
}

async fn update_training_dataset(State(state): State<AppState>, Path(id): Path<String>, Json(input): Json<DatasetInput>) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    state.training.update_dataset(&id, input.name, input.trigger).map(Json).map_err(training_error)
}

async fn delete_training_dataset(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    if state.training.dataset_in_use(&id).await {
        return Err(api_error(StatusCode::CONFLICT, "a LoRA is training on this dataset; change it once the run ends".into()));
    }
    state.training.remove_dataset(&id).map_err(training_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct DatasetSongs {
    song_ids: Vec<String>,
}

/// Adds library songs with their style and lyrics; decoding and resampling is
/// real work, so it runs off the request threads.
async fn add_training_songs(State(state): State<AppState>, Path(id): Path<String>, Json(input): Json<DatasetSongs>) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    let mut sources = Vec::new();
    for song_id in &input.song_ids {
        let song = state
            .library
            .get_song(song_id)
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("song {song_id} is not in the library")))?;
        let audio = state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::BAD_REQUEST, format!("{} has no stored audio", song.title)))?;
        // what the song was made with is what it is: tempo, key, metre, language
        let meta = |key: &str| song.metadata.get(key).cloned().unwrap_or(Value::Null);
        let text = |key: &str| meta(key).as_str().map(str::to_owned);
        let patch = training::ItemPatch {
            bpm: meta("bpm").as_u64().map(|bpm| bpm as u32),
            keyscale: text("keyscale"),
            timesignature: text("timesignature"),
            language: text("vocal_language").filter(|language| language != "unknown"),
            ..Default::default()
        };
        sources.push((audio, song.title, song.caption, song.lyrics, song.id, patch));
    }
    let training = state.training.clone();
    tokio::task::spawn_blocking(move || {
        let mut dataset = training.dataset(&id)?;
        for (audio, title, style, lyrics, song_id, patch) in sources {
            let instrumental = lyrics.trim().is_empty() || lyrics.trim() == "[Instrumental]";
            dataset = training.add_item(&id, &audio, &title, "", &style, if instrumental { "" } else { &lyrics }, instrumental, &format!("song:{song_id}"))?;
            if let Some(item) = dataset.items.last().map(|item| item.id.clone()) {
                dataset = training.update_item(&id, &item, patch)?;
            }
        }
        Ok::<_, anyhow::Error>(dataset)
    })
    .await
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
    .map_err(training_error)
}

/// Adds audio files from the user's disk; a same-named `.txt` or `.lrc` part
/// is taken as that song's lyrics.
async fn upload_training_files(State(state): State<AppState>, Path(id): Path<String>, mut multipart: Multipart) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    let folder = UploadFolder::new()?;
    let mut audio = Vec::new();
    let mut texts = std::collections::HashMap::new();
    let mut cues: Vec<(String, Option<String>, Vec<training::CueTrack>)> = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read the upload: {e}")))? {
        // the page sends the path inside what was dropped; its folders name the artist
        let Some(relative) = field.file_name().map(|name| name.replace('\\', "/")) else { continue };
        let name = relative.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or("song").to_owned();
        let lower = name.to_lowercase();
        let stem = std::path::Path::new(&name).file_stem().and_then(|stem| stem.to_str()).unwrap_or(&name).to_owned();
        if lower.ends_with(".txt") || lower.ends_with(".lrc") {
            let bytes = field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read {name}: {e}")))?;
            texts.insert(stem, String::from_utf8_lossy(&bytes).into_owned());
        } else if lower.ends_with(".cue") {
            let bytes = field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read {name}: {e}")))?;
            let (file, tracks) = training::cue_sheet(&bytes);
            cues.push((stem, file, tracks));
        } else if [".wav", ".mp3", ".flac", ".ogg", ".m4a"].iter().any(|extension| lower.ends_with(extension)) {
            let path = folder.0.join(format!("{}-{name}", audio.len()));
            save_upload(field, &path).await?;
            audio.push((path, stem, name, relative));
        }
    }
    if audio.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "no audio in the upload: WAV, MP3, FLAC, OGG or M4A".into()));
    }
    let training = state.training.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut dataset = training.dataset(&id)?;
        for (path, stem, name, relative) in audio {
            // an album in one file comes with the cue sheet that cuts it
            let sheet = cues.iter().find(|(cue_stem, file, tracks)| tracks.len() > 1 && (file.as_deref() == Some(name.as_str()) || *cue_stem == stem));
            if let Some((_, _, tracks)) = sheet {
                let album_artist = audio_pcm::tags(&path).artist;
                dataset = training.add_album(&id, &path, tracks, &album_artist, &format!("file:{relative}"))?;
                continue;
            }
            // a text beside the file, else the lyrics the file carries in its own tags
            let tags = audio_pcm::tags(&path);
            let lyrics = texts.get(&stem).map(|text| training::plain_lyrics(text)).filter(|text| !text.trim().is_empty()).unwrap_or_else(|| training::plain_lyrics(&tags.lyrics));
            let (artist, title) = training::identify(&path, &relative);
            dataset = training.add_item(&id, &path, &title, &artist, "", &lyrics, false, &format!("file:{relative}"))?;
            let details = training::tag_details(&tags);
            if let (Some(item), Some(details)) = (dataset.items.last().map(|item| item.id.clone()), details) {
                dataset = training.edit_item(&id, &item, details)?;
            }
        }
        Ok::<_, anyhow::Error>(dataset)
    })
    .await;
    drop(folder);
    outcome.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?.map(Json).map_err(training_error)
}

async fn update_training_item(
    State(state): State<AppState>,
    Path((id, item)): Path<(String, String)>,
    Json(patch): Json<training::ItemPatch>,
) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    state.training.edit_item(&id, &item, patch).map(Json).map_err(training_error)
}

async fn delete_training_item(State(state): State<AppState>, Path((id, item)): Path<(String, String)>) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    if state.training.dataset_in_use(&id).await {
        return Err(api_error(StatusCode::CONFLICT, "a LoRA is training on this dataset; change it once the run ends".into()));
    }
    state.training.remove_item(&id, &item).map(Json).map_err(training_error)
}

#[derive(Debug, Deserialize)]
struct StartTraining {
    dataset_id: String,
    #[serde(default)]
    name: String,
    recipe: training::Recipe,
}

/// Starts a run. It wants the whole card: refused while a song renders, and
/// the writing assistant is let go first.
async fn start_training(State(state): State<AppState>, Json(input): Json<StartTraining>) -> Result<Json<training::Run>, (StatusCode, Json<ApiError>)> {
    let preparing = state.prepare.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().is_some_and(|job| !job.finished);
    if preparing {
        return Err(api_error(StatusCode::CONFLICT, "songs are being prepared; train once that is done".into()));
    }
    start_training_run(&state, &input.dataset_id, &input.name, input.recipe)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::CONFLICT, error))
}

/// The model files the studio renders with now: the set chosen in the model
/// manager, or the one chosen on the create page.
async fn selected_model_files(state: &AppState) -> Result<model_manager::ProfileModelFiles, String> {
    let selected_profile = state.selected_profile_id.read().await.clone();
    let selected_components = state.selected_component_ids.read().await.clone();
    match (selected_components, selected_profile) {
        (Some(components), _) => state.model_manager.installed_component_files(&components),
        (None, Some(profile)) => state.model_manager.installed_profile_files(&profile),
        (None, None) => return Err("no model set is installed yet; install one in Settings - Models".into()),
    }
    .map_err(|error| error.to_string())
}

/// The weights a run trains on: the unquantised (BF16) DiT of the variant the
/// studio renders with - the trainer refuses a quantised base, and the adapter
/// it makes fits every quantisation of that variant - with the studio's VAE
/// and text encoder.
struct TrainingWeights {
    base: music_engine::train::TrainingBase,
    installed: bool,
    bytes: u64,
    /// The studio's set with the DiT swapped, for the model manager to fetch
    /// the one missing file of.
    download: Vec<String>,
}

async fn training_weights(state: &AppState) -> Result<TrainingWeights, String> {
    let files = selected_model_files(state).await?;
    let catalog = state.model_manager.catalog();
    let current = catalog.components.iter().find(|component| component.filename == files.synth_model).ok_or_else(|| format!("{} is not a DiT of the catalogue", files.synth_model))?;
    let variant = current.id.rsplit_once('-').map(|(variant, _)| variant).unwrap_or(current.id);
    let bf16_id = format!("{variant}-bf16");
    let bf16 = catalog.components.iter().find(|component| component.id == bf16_id).ok_or_else(|| format!("{variant} has no BF16 weights to train on"))?;
    let set: Vec<String> = catalog
        .components
        .iter()
        .filter(|component| [files.lm_model.as_str(), files.text_encoder.as_str(), files.vae.as_str()].contains(&component.filename))
        .map(|component| component.id.to_string())
        .chain(std::iter::once(bf16.id.to_string()))
        .collect();
    let root = state.model_manager.models_directory().to_path_buf();
    Ok(TrainingWeights {
        installed: root.join(bf16.filename).is_file(),
        bytes: bf16.bytes,
        download: set,
        base: music_engine::train::TrainingBase { models: root, dit: bf16.filename.to_string(), vae: files.vae, text_encoder: files.text_encoder },
    })
}

async fn training_base(state: &AppState) -> Result<music_engine::train::TrainingBase, String> {
    let weights = training_weights(state).await?;
    if !weights.installed {
        return Err(format!("the trainer needs the unquantised {} ({:.1} GB); download it on the training page", weights.base.dit, weights.bytes as f64 / 1e9));
    }
    Ok(weights.base)
}

/// A run of `dataset`, refused while a song renders.
async fn start_training_run(state: &AppState, dataset: &str, name: &str, recipe: training::Recipe) -> Result<training::Run, String> {
    let rendering = state.jobs.read().await.values().any(|job| matches!(job.status, MusicJobStatus::Queued | MusicJobStatus::Running));
    if rendering {
        return Err("a song is being made; train once it is done".into());
    }
    let base = training_base(state).await?;
    state
        .training
        .start(Some(engine_bundle_root()), dataset, name, recipe, base, card_hooks(state).await)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// Frees the card for a run and gives it back after: the assistant is stopped,
/// and an engine kept loaded between songs is stopped and started again.
async fn card_hooks(state: &AppState) -> training::CardHooks {
    let keep_loaded = state.engine_options.read().await.keep_loaded;
    let (take_state, back_state) = (state.clone(), state.clone());
    training::CardHooks {
        take: Box::new(move || {
            Box::pin(async move {
                free_the_card_for_the_engine(&take_state).await;
                if keep_loaded {
                    if let Some(engine) = take_state.engine.lock().await.as_mut() {
                        if let Err(error) = tokio::task::block_in_place(|| engine.stop(std::time::Duration::from_secs(10))) {
                            eprintln!("[ERROR] stopping the engine for training: {error}");
                        }
                    }
                }
            })
        }),
        give_back: Box::new(move || {
            Box::pin(async move {
                if keep_loaded {
                    if let Err(error) = restart_engine(&back_state).await {
                        eprintln!("[ERROR] starting the engine after training: {error}");
                    }
                }
            })
        }),
    }
}

/// Refuses work that needs the graphics card while a LoRA trains on it.
async fn card_free_of_training(state: &AppState, what: &str) -> Result<(), (StatusCode, Json<ApiError>)> {
    if state.training.active_run().await.is_some() {
        return Err(api_error(StatusCode::CONFLICT, format!("a LoRA is training on the card; {what} once it finishes")));
    }
    Ok(())
}

#[derive(Deserialize)]
struct ContinueTraining {
    /// The steps the run is to reach in all.
    steps: u32,
}

/// Trains a finished or stopped run further from its latest checkpoint.
async fn continue_training(State(state): State<AppState>, Path(id): Path<String>, Json(input): Json<ContinueTraining>) -> Result<Json<training::Run>, (StatusCode, Json<ApiError>)> {
    let preparing = state.prepare.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().is_some_and(|job| !job.finished);
    if preparing {
        return Err(api_error(StatusCode::CONFLICT, "songs are being prepared; train once that is done".into()));
    }
    let rendering = state.jobs.read().await.values().any(|job| matches!(job.status, MusicJobStatus::Queued | MusicJobStatus::Running));
    if rendering {
        return Err(api_error(StatusCode::CONFLICT, "a song is being made; train once it is done".into()));
    }
    state
        .training
        .continue_run(Some(engine_bundle_root()), &id, input.steps, training_base(&state).await.map_err(|error| api_error(StatusCode::CONFLICT, error))?, card_hooks(&state).await)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::CONFLICT, format!("{error:#}")))
}

async fn cancel_training(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    state.training.cancel(&id).await.map_err(training_error)?;
    Ok(Json(serde_json::json!({ "cancelled": true })))
}

async fn delete_training_run(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    state.training.remove_run(&id).await.map_err(training_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct InstallCheckpoint {
    #[serde(default)]
    name: Option<String>,
}

/// Adds a checkpoint to the adapter library, where the create page finds it.
async fn install_training_checkpoint(
    State(state): State<AppState>,
    Path((id, step)): Path<(String, u32)>,
    Json(input): Json<InstallCheckpoint>,
) -> Result<Json<adapters::AdapterMeta>, (StatusCode, Json<ApiError>)> {
    let run = state.training.run(&id).map_err(training_error)?;
    let checkpoint = state
        .training
        .checkpoints(&id)
        .into_iter()
        .find(|checkpoint| checkpoint.step == step)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("the run has no checkpoint at step {step}")))?;
    let name = input.name.filter(|name| !name.trim().is_empty()).unwrap_or_else(|| format!("{} · {step}", run.name));
    let trigger = Some(run.trigger.clone());
    let meta = state
        .adapters
        .import_trained(&name, trigger, run.base.as_deref().and_then(music_engine::model::model_family).map(str::to_owned), &checkpoint.files, adapters::Origin::Trained { run: id.clone(), step })
        .map_err(training_error)?;
    state.training.mark_installed(&id, step).map_err(training_error)?;
    Ok(Json(meta))
}

/// Writes a dataset song's structured caption with the writing assistant,
/// from what the song already says about itself - its style line and its
/// lyrics - the way the create form writes one. The trainer learns the song
/// from this caption, and a two-line style is not what generation will send.
async fn describe_training_item(
    State(state): State<AppState>,
    Path((id, item)): Path<(String, String)>,
) -> Result<Json<training::Dataset>, (StatusCode, Json<ApiError>)> {
    if matches!(state.assistant.read().await.provider, AssistantProvider::Managed) {
        card_free_of_training(&state, "describe songs").await?;
    }
    let dataset = state.training.dataset(&id).map_err(training_error)?;
    let song = dataset
        .items
        .iter()
        .find(|entry| entry.id == item)
        .cloned()
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("no song {item} in the dataset")))?;
    let request = assistant::AssistRequest {
        target: assistant::AssistTarget::Prompt,
        description: String::new(),
        instruction: format!("{}. Describe this finished recording: {}", song.title.trim(), song.style.trim()),
        lyrics: if song.instrumental { String::new() } else { song.lyrics.clone() },
        caption: String::new(),
        duration_seconds: song.seconds,
        instrumental: song.instrumental,
        vocal_language: String::new(),
        recording: true,
    };
    // A small local model now and then answers without the caption; it is
    // asked once more, then the failure is the user's to see.
    let mut caption = String::new();
    for attempt in 0..2 {
        let draft = match assistant_write(State(state.clone()), Json(request.clone())).await {
            Ok(Json(draft)) => draft,
            Err(error) if attempt == 0 => {
                eprintln!("[WARN] describing {}: {}", song.title, error.1.0.error);
                continue;
            }
            Err(error) => return Err(error),
        };
        caption = draft.get("caption").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();
        if !caption.is_empty() {
            break;
        }
    }
    if caption.is_empty() {
        return Err(api_error(StatusCode::BAD_GATEWAY, "the assistant left the caption empty twice; try again or write the caption".into()));
    }
    state
        .training
        .update_item(&id, &item, training::ItemPatch { style: Some(caption), style_state: Some(training::StyleState::Done), heard: Some(None), ..Default::default() })
        .map(Json)
        .map_err(training_error)
}

async fn read_stems(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let songs = state.library.list_songs().map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let in_library: Vec<Value> = songs
        .iter()
        .filter(|song| derived_from(song) == Some((id.as_str(), "stems")))
        .map(|song| serde_json::json!({ "stem": song.metadata.pointer("/derived/settings/stem"), "song_id": song.id, "title": song.title }))
        .collect();
    Ok(Json(serde_json::json!({
        "song_id": id,
        "stems": stems_on_disk(&state, &id),
        "library_songs": in_library,
        "run": state.separation_run.read().await.clone(),
    })))
}

async fn read_stem_audio(
    State(state): State<AppState>,
    Path((id, stem)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    if !separation::STEMS.contains(&stem.as_str()) {
        return Err(api_error(StatusCode::BAD_REQUEST, format!("unknown stem {stem}")));
    }
    let path = stem_path(&state, &id, &stem);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| api_error(StatusCode::NOT_FOUND, "this track has no such stem yet".into()))?;
    // Without range support a player cannot seek: it can only start at zero and
    // wait. The library's own audio has answered ranges from the beginning;
    // stems were served whole, which is why dragging their position did
    // nothing.
    let total = bytes.len();
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_single_byte_range(value, total));
    let response = if let Some((start, end)) = range {
        axum::response::Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_TYPE, "audio/wav")
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}"))
            .header(header::CONTENT_LENGTH, end - start + 1)
            .body(Body::from(bytes[start..=end].to_vec()))
    } else {
        axum::response::Response::builder()
            .header(header::CONTENT_TYPE, "audio/wav")
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, total)
            .body(Body::from(bytes))
    };
    Ok(response.expect("valid stem response"))
}

/// Separates one track into stems, in the background, reporting progress.
async fn start_separation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    card_free_of_training(&state, "separate tracks").await?;
    if state.separation_run.read().await.as_ref().is_some_and(|run| !run.done) {
        return Err(api_error(StatusCode::CONFLICT, "a track is already being separated".into()));
    }
    let wanted_runtime = state.separation_config.read().await.runtime;
    // Which library is loaded is decided once per process - `ort` binds it on
    // first use - so always take the CUDA build when it is complete: it carries
    // the processor provider too, and the setting below decides which of them
    // actually runs. Choosing by the setting meant a studio that had run once
    // on the processor could never reach the card without a restart.
    let runtime = state
        .lyrics_sync
        .onnxruntime_library_of(if state.lyrics_sync.has_cuda_libraries() {
            lyrics_sync::OnnxFlavour::Cuda
        } else {
            lyrics_sync::OnnxFlavour::Cpu
        })
        .or_else(|| state.lyrics_sync.onnxruntime_library())
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "the ONNX Runtime is not installed yet".into()))?;
    // The card is only really available when every CUDA library the provider
    // links against is beside it.
    let on_gpu = !matches!(wanted_runtime, lyrics_sync::OnnxFlavour::Cpu)
        && state.lyrics_sync.has_cuda_libraries();
    if !state.separator.is_installed() {
        return Err(api_error(StatusCode::BAD_REQUEST, "the separation model is not installed yet".into()));
    }
    let song = state
        .library
        .get_song(&id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let audio_path = state
        .library
        .media_path_for_song(&song)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "this track has no stored audio".into()))?;

    *state.separation_run.write().await =
        Some(SeparationRun { song_id: id.clone(), progress: 0.0, done: false, error: None, stems: vec![], used_gpu: None, library_songs: vec![] });

    let model = state.separator.model_path();
    let config = state.separation_config.read().await.clone();
    let overlap = config.sane_overlap();
    let wanted = config.stems.clone();
    let background = state.clone();
    let song_id = id.clone();
    tokio::task::spawn_blocking(move || {
        point_ort_at(&runtime);

        let outcome = (|| -> anyhow::Result<(Vec<String>, bool)> {
            let audio = audio_pcm::decode_stereo_44k(&audio_path)?;
            let handle = tokio::runtime::Handle::current();
            let separated = separation::separate(&model, &audio, separation::STEMS.len(), overlap, on_gpu, |fraction| {
                let state = background.clone();
                handle.spawn(async move {
                    if let Some(run) = state.separation_run.write().await.as_mut() {
                        run.progress = fraction;
                    }
                });
            })?;
            let mut written = Vec::new();
            let ran_on_gpu = separated.used_gpu;
            for stem in separated.stems {
                if !wanted.iter().any(|name| name == stem.name) {
                    continue;
                }
                let path = stem_path(&background, &song_id, stem.name);
                separation::write_wav_stereo(&path, &stem.samples)?;
                written.push(stem.name.to_string());
            }
            Ok((written, ran_on_gpu))
        })();
        // still on this thread: the stems are hundreds of megabytes to copy
        let library = outcome.as_ref().ok().map(|(stems, _)| stems_into_library(&background, &song_id, stems, overlap));

        let handle = tokio::runtime::Handle::current();
        handle.spawn(async move {
            if let Some(run) = background.separation_run.write().await.as_mut() {
                run.done = true;
                match outcome {
                    Ok((stems, ran_on_gpu)) => {
                        run.progress = 1.0;
                        run.used_gpu = Some(ran_on_gpu);
                        match library {
                            Some(Ok((songs, problem))) => {
                                run.library_songs = songs;
                                run.error = problem;
                            }
                            Some(Err(error)) => run.error = Some(format!("the stems are separated but did not reach the library: {error:#}")),
                            None => {}
                        }
                        run.stems = stems;
                    }
                    Err(error) => run.error = Some(format!("{error:#}")),
                }
            }
        });
    });

    Ok(Json(serde_json::json!({ "started": true, "song_id": id })))
}


/// Draws a cover for a finished track, if the studio was told to.
///
/// The same pieces the cover window uses: the default template, filled in from
/// this track, and the image model chosen on the provider page. Nothing happens
/// without a key, without a model, or when the user turned this off - and a
/// failure is written to the log rather than shown as a broken track.
/// Draws the cover for one track now, and says what went wrong if it did not.
async fn draw_cover_now(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    match draw_cover(&state, &id).await {
        Ok(()) => Ok(Json(serde_json::json!({ "drawn": true }))),
        Err(error) => Err(api_error(StatusCode::BAD_GATEWAY, error.to_string())),
    }
}


/// Times the lyrics of a finished track, if karaoke is switched on.
///
/// The switch said "on" and nothing happened: the timings were only ever made
/// by the button in the track menu. A track arrives with its words already
/// known, so this is the moment to time them.

/// One background piece of work on a finished track.
#[derive(Debug, Clone, Serialize)]
struct Activity {
    song_id: String,
    title: String,
    /// "cover" or "karaoke".
    kind: &'static str,
    /// "running", "done" or "failed".
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Notes what is happening, keeping only the recent past.
async fn note_activity(state: &AppState, song_id: &str, title: &str, kind: &'static str, phase: &'static str, detail: Option<String>) {
    let mut activity = state.activity.write().await;
    if let Some(existing) = activity.iter_mut().find(|entry| entry.song_id == song_id && entry.kind == kind) {
        existing.state = phase;
        existing.detail = detail;
        existing.title = title.to_string();
    } else {
        activity.push(Activity {
            song_id: song_id.to_string(),
            title: title.to_string(),
            kind,
            state: phase,
            detail,
        });
    }
    let overflow = activity.len().saturating_sub(20);
    if overflow > 0 {
        activity.drain(..overflow);
    }
}

async fn read_activity(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({ "activity": state.activity.read().await.clone() }))
}

/// Downloads whatever the chosen local recogniser is missing, then waits for it.
///
/// Choosing Parakeet or Whisper in the settings is the instruction to use it;
/// making the user then find a download button for it is a second instruction
/// nobody asked for. The first track that needs timings fetches the model and
/// carries on.
async fn ensure_local_recogniser(state: &AppState, config: &lyrics_sync::LyricsSyncConfig, song_id: &str) -> bool {
    let ready = |state: &AppState| match config.provider {
        lyrics_sync::AsrProvider::Parakeet => state.lyrics_sync.parakeet_ready(),
        lyrics_sync::AsrProvider::Whisper => {
            state.lyrics_sync.whisper_binary().is_some() && state.lyrics_sync.whisper_model_ready(config)
        }
        _ => true,
    };
    if ready(state) {
        return true;
    }

    let missing: Vec<&'static lyrics_sync::Asset> = match config.provider {
        lyrics_sync::AsrProvider::Parakeet => lyrics_sync::PARAKEET_ASSET_IDS
            .iter()
            .filter_map(|id| lyrics_sync::asset(id))
            .filter(|asset| !state.lyrics_sync.downloader().is_installed(asset))
            .collect(),
        // Whatever the chosen recogniser is made of, by the same reckoning the
        // panel uses. Naming the files here by hand is how this went on asking
        // for `whisper-cuda` after that asset had ceased to exist, and then
        // concluded that nothing was missing and nothing was ready.
        lyrics_sync::AsrProvider::Whisper => karaoke_set("whisper", config.runtime, config.whisper_model.as_deref())
            .into_iter()
            .filter(|asset| !state.lyrics_sync.downloader().is_installed(asset))
            .collect(),
        _ => Vec::new(),
    };
    if missing.is_empty() {
        return ready(state);
    }

    let title = state.library.get_song(song_id).ok().flatten().map(|song| song.title).unwrap_or_default();
    note_activity(state, song_id, &title, "karaoke", "running", Some("karaoke.downloading".into())).await;
    for asset in missing {
        if let Err(error) = state.lyrics_sync.downloader().install(asset).await {
            eprintln!("could not fetch the karaoke model {}: {error}", asset.id);
            note_activity(state, song_id, &title, "karaoke", "failed", Some(format!("{error:#}"))).await;
            return false;
        }
        // The downloader runs one file at a time in the background; the timings
        // wait for it rather than starting against half a model. The wait is
        // reported with real numbers: a spinner that says "downloading" for ten
        // minutes without moving is indistinguishable from one that is stuck.
        loop {
            let Some(progress) = state.lyrics_sync.downloader().active().await else { break };
            if progress.done {
                break;
            }
            let percent = if progress.total_bytes > 0 {
                (progress.downloaded_bytes * 100 / progress.total_bytes).min(100)
            } else {
                0
            };
            note_activity(state, song_id, &title, "karaoke", "running", Some(format!("karaoke.downloading {percent}%"))).await;
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        }
    }
    ready(state)
}

async fn time_lyrics_for(state: AppState, song_id: String) {
    let config = state.lyrics_sync_config.read().await.clone();
    if !config.enabled || config.provider == lyrics_sync::AsrProvider::None {
        return;
    }
    // The cloud recogniser is the one case that cannot be fixed from here: a key
    // is the user's to add, and announcing a failure they cannot act on is
    // noise. A local recogniser is different - if its model is not on disk yet,
    // choosing it is the instruction to fetch it, so the first use downloads it
    // and then does the work.
    if config.provider == lyrics_sync::AsrProvider::OpenRouter && credentials::openrouter_api_key().is_none() {
        return;
    }
    if !ensure_local_recogniser(&state, &config, &song_id).await {
        return;
    }
    let Ok(Some(song)) = state.library.get_song(&song_id) else { return };
    // An instrumental has section markers and no words. Timing it means asking
    // the recogniser to find lyrics that were never sung.
    if !auto_title::has_sung_lines(&song.lyrics) {
        return;
    }
    let Some(audio) = state.library.media_path_for_song(&song) else { return };
    let audio = audio.display().to_string();

    note_activity(&state, &song_id, &song.title, "karaoke", "running", None).await;
    let words = match config.provider {
        lyrics_sync::AsrProvider::None => return,
        lyrics_sync::AsrProvider::Parakeet => {
            let sync = state.lyrics_sync.clone();
            let path = std::path::PathBuf::from(&audio);
            match tokio::task::spawn_blocking(move || sync.parakeet_words(&path)).await {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("no karaoke for {song_id}: {error}");
                    return;
                }
            }
        }
        lyrics_sync::AsrProvider::Whisper => {
            let sync = state.lyrics_sync.clone();
            let config = config.clone();
            let path = std::path::PathBuf::from(&audio);
            let lyrics = song.lyrics.clone();
            match tokio::task::spawn_blocking(move || sync.whisper_words(&config, &path, None, &lyrics)).await {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("no karaoke for {song_id}: {error}");
                    return;
                }
            }
        }
        lyrics_sync::AsrProvider::OpenRouter => {
            karaoke_words_from_openrouter(&state, &config, &audio, None).await
        }
    };
    let words = match words {
        Ok(words) => words,
        Err(error) => {
            eprintln!("no karaoke for {song_id}: {error}");
            note_activity(&state, &song_id, &song.title, "karaoke", "failed", Some(format!("{error:#}"))).await;
            return;
        }
    };
    let lines = lyrics_sync::align_lyrics_words(&words, &song.lyrics);
    if lines.is_empty() {
        let reason = "karaoke.no-match";
        eprintln!("no karaoke for {song_id}: {reason}");
        note_activity(&state, &song_id, &song.title, "karaoke", "failed", Some(reason.to_string())).await;
        return;
    }
    match state.library.set_song_lrc(&song_id, &lyrics_sync::enhanced_lrc(&lines)) {
        Ok(_) => note_activity(&state, &song_id, &song.title, "karaoke", "done", None).await,
        Err(error) => {
            eprintln!("could not store karaoke for {song_id}: {error}");
            note_activity(&state, &song_id, &song.title, "karaoke", "failed", Some(format!("{error:#}"))).await;
        }
    }
}

async fn draw_cover_for(state: AppState, song_id: String) {
    if !*state.cover_auto.read().await {
        return;
    }
    // Drawing a cover needs a cloud key. Without one there is nothing to try,
    // and announcing a failure the user cannot act on is noise: the track keeps
    // the placeholder artwork the library already shows.
    if credentials::openrouter_api_key().is_none() {
        return;
    }
    let title = state.library.get_song(&song_id).ok().flatten().map(|song| song.title).unwrap_or_default();
    note_activity(&state, &song_id, &title, "cover", "running", None).await;
    match draw_cover(&state, &song_id).await {
        Ok(()) => note_activity(&state, &song_id, &title, "cover", "done", None).await,
        Err(error) => {
            eprintln!("no cover for {song_id}: {error}");
            note_activity(&state, &song_id, &title, "cover", "failed", Some(format!("{error:#}"))).await;
        }
    }
}

/// The work itself, with its reasons kept rather than printed.
async fn draw_cover(state: &AppState, song_id: &str) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let song = state
        .library
        .get_song(song_id)?
        .context("the track is not in the library")?;
    if song.metadata.get("cover_filename").is_some() {
        return Ok(());
    }
    let model = {
        let configuration = state.configuration.read().await;
        configuration
            .selections
            .iter()
            .find(|selection| selection.capability == Capability::CoverArt)
            .filter(|selection| selection.mode == ExecutionMode::OpenRouter)
            .and_then(|selection| selection.cloud_model.clone())
            .filter(|model| !model.trim().is_empty())
    };
    let catalog = catalog_for(state).await.map_err(|error| anyhow::anyhow!(error))?;
    let model = match model {
        Some(model) => model,
        None => providers::openrouter::suggested_model(&catalog, Capability::CoverArt)
            .context("no image model is chosen for covers")?,
    };
    let templates = state.cover_templates.read().await.clone();
    let default_id = state.cover_template_default.read().await.clone();
    let template = templates
        .iter()
        .find(|entry| Some(&entry.id) == default_id.as_ref())
        .or_else(|| templates.first())
        .map(|entry| entry.template.clone())
        .context("there are no cover templates")?;
    let facts = cover_prompt::TrackFacts {
        title: song.title.clone(),
        style: song.caption.clone(),
        lyrics: song.lyrics.clone(),
        duration_seconds: song.metadata.get("duration_seconds").and_then(Value::as_f64).unwrap_or(0.0),
    };
    let prompt = match song.metadata.get("cover_prompt").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()) {
        Some(written) => written.to_string(),
        None => cover_prompt::render(&template, &facts),
    };
    let request = providers::openrouter::request_for(&catalog, Capability::CoverArt, &model, &prompt)?;
    let answered = execute_openrouter_json(request).await.map_err(|error| anyhow::anyhow!(error))?;
    let first = answered
        .body
        .get("data")
        .and_then(|data| data.get(0))
        .context("the model returned no image")?;
    let image = first
        .get("b64_json")
        .and_then(Value::as_str)
        .context("the model returned no image")?;
    // The answer states its own format, and it is not always PNG.
    let media_type = first.get("media_type").and_then(Value::as_str).unwrap_or("image/png").to_string();
    use base64::{engine::general_purpose::STANDARD, Engine};
    let bytes = STANDARD.decode(image.trim()).context("the image was not valid base64")?;
    state.library.store_song_cover(song_id, &bytes, &media_type)?;
    tag_stored_song(state, song_id).await;
    Ok(())
}

#[allow(dead_code)]
async fn draw_cover_unused(state: AppState, song_id: String) {
    let Ok(Some(song)) = state.library.get_song(&song_id) else { return };
    if song.metadata.get("cover_filename").is_some() {
        return;
    }

    let model = {
        let configuration = state.configuration.read().await;
        configuration
            .selections
            .iter()
            .find(|selection| selection.capability == Capability::CoverArt)
            .filter(|selection| selection.mode == ExecutionMode::OpenRouter)
            .and_then(|selection| selection.cloud_model.clone())
    };
    let model = match model {
        Some(model) if !model.trim().is_empty() => model,
        _ => match catalog_for(&state).await.ok().and_then(|catalog| {
            providers::openrouter::suggested_model(&catalog, Capability::CoverArt)
        }) {
            Some(model) => model,
            None => return,
        },
    };

    let template = {
        let templates = state.cover_templates.read().await;
        let default_id = state.cover_template_default.read().await.clone();
        templates
            .iter()
            .find(|entry| Some(&entry.id) == default_id.as_ref())
            .or_else(|| templates.first())
            .map(|entry| entry.template.clone())
    };
    let Some(template) = template else { return };

    let facts = cover_prompt::TrackFacts {
        title: song.title.clone(),
        style: song.caption.clone(),
        lyrics: song.lyrics.clone(),
        duration_seconds: song.metadata.get("duration_seconds").and_then(Value::as_f64).unwrap_or(0.0),
    };
    // What the assistant wrote for this track beats the generic template: it
    // was written with the lyrics in front of it.
    let prompt = match song.metadata.get("cover_prompt").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()) {
        Some(written) => written.to_string(),
        None => cover_prompt::render(&template, &facts),
    };

    let catalog = match catalog_for(&state).await {
        Ok(catalog) => catalog,
        Err(_) => return,
    };
    let request = match providers::openrouter::request_for(&catalog, Capability::CoverArt, &model, &prompt) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("no cover for {song_id}: {error}");
            return;
        }
    };
    let answered = match execute_openrouter_json(request).await {
        Ok(answered) => answered,
        Err(error) => {
            eprintln!("no cover for {song_id}: {error}");
            return;
        }
    };
    let image = answered
        .body
        .get("data")
        .and_then(|data| data.get(0))
        .and_then(|first| first.get("b64_json"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let Some(image) = image else {
        eprintln!("no cover for {song_id}: the model returned no image");
        return;
    };
    use base64::{engine::general_purpose::STANDARD, Engine};
    let Ok(bytes) = STANDARD.decode(image.trim()) else { return };
    if let Err(error) = state.library.store_song_cover(&song_id, &bytes, "image/png") {
        eprintln!("could not store the cover for {song_id}: {error}");
        return;
    }
    tag_stored_song(&state, &song_id).await;
}

async fn read_cover_templates(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({
        "auto": *state.cover_auto.read().await,
        "templates": state.cover_templates.read().await.clone(),
        "default_id": state.cover_template_default.read().await.clone(),
        "placeholders": ["title", "style", "lyrics", "excerpt", "duration"],
    }))
}

async fn write_cover_templates(
    State(state): State<AppState>,
    Json(request): Json<CoverTemplatesRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let templates = if request.templates.is_empty() { cover_prompt::default_templates() } else { request.templates };
    *state.cover_templates.write().await = templates.clone();
    // A default that names a template nobody kept is worse than none.
    let default_id = request
        .default_id
        .filter(|id| !id.trim().is_empty() && templates.iter().any(|entry| entry.id == *id));
    *state.cover_template_default.write().await = default_id.clone();
    if let Some(auto) = request.auto {
        *state.cover_auto.write().await = auto;
    }
    persist_studio_settings(&state)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({
        "templates": templates,
        "default_id": default_id,
        "auto": *state.cover_auto.read().await,
    })))
}

/// The prompt a template turns into for one track, exactly as it would be sent.
async fn render_cover_template(
    State(state): State<AppState>,
    Json(request): Json<RenderCoverPromptRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let mut facts = cover_prompt::TrackFacts {
        title: request.title.unwrap_or_default(),
        style: request.style.unwrap_or_default(),
        lyrics: request.lyrics.unwrap_or_default(),
        duration_seconds: 0.0,
    };
    if let Some(song_id) = request.song_id.as_deref().filter(|value| !value.trim().is_empty()) {
        let song = state
            .library
            .get_song(song_id)
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
        facts.title = song.title.clone();
        facts.style = song.caption.clone();
        facts.lyrics = song.lyrics.clone();
        facts.duration_seconds = song
            .metadata
            .get("duration_seconds")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
    }
    Ok(Json(serde_json::json!({ "prompt": cover_prompt::render(&request.template, &facts) })))
}

async fn library_cover(State(state): State<AppState>, Path(id): Path<String>) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let song = state.library.get_song(&id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let (path, media_type) = state.library.cover_path_for_song(&song).ok_or_else(|| api_error(StatusCode::NOT_FOUND, "This song has no stored cover image".into()))?;
    let bytes = tokio::fs::read(&path).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("read cover: {error}")))?;
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, media_type)
        .header(header::CONTENT_LENGTH, bytes.len())
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .expect("valid cover response"))
}


/// Writes ID3 tags onto a stored MP3 from what the library knows about it.
///
/// Called after a track is stored, after its cover changes and after it is
/// renamed. Failure is logged and never fails the request: an untagged track
/// still plays, a lost one does not.
async fn tag_stored_song(state: &AppState, song_id: &str) {
    let Ok(Some(song)) = state.library.get_song(song_id) else { return };
    // `audio_path` is a full path, not a filename: resolve it the way playback
    // does, or tagging silently skips every track.
    let Some(audio_path) = state.library.media_path_for_song(&song) else { return };
    if audio_path.extension().and_then(|value| value.to_str()).map(str::to_lowercase).as_deref() != Some("mp3") {
        return;
    }
    let cover = state
        .library
        .cover_path_for_song(&song)
        .and_then(|(path, media_type)| std::fs::read(path).ok().map(|bytes| (media_type.to_string(), bytes)));
    let tags = tagging::TrackTags {
        title: song.title.clone(),
        album: music_core::studio().name.clone(),
        // The engine is the performer here; the studio is the label.
        artist: music_core::studio().artist.clone(),
        genre: tagging::genre_from_caption(&song.caption),
        lyrics: Some(song.lyrics.clone()).filter(|value| !value.trim().is_empty()),
        bpm: tagging::bpm_from_caption(&song.caption),
        cover,
    };
    if let Err(error) = tagging::write_mp3_tags(&audio_path, &tags) {
        eprintln!("could not tag {}: {error}", audio_path.display());
    }
}

async fn store_library_cover(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<StoreCoverRequest>,
) -> Result<Json<library::Song>, (StatusCode, Json<ApiError>)> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let image = STANDARD
        .decode(request.image_base64.trim())
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("cover image is not valid base64: {error}")))?;
    let song = state
        .library
        .store_song_cover(&id, &image, &request.media_type)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    // The cover belongs in the file too, not only beside it.
    tag_stored_song(&state, &id).await;
    Ok(Json(song))
}

async fn create_library_song(State(state):State<AppState>,Json(input):Json<library::SongInput>)->Result<(StatusCode,Json<library::Song>),(StatusCode,Json<ApiError>)>{state.library.create_song(input).map(|s|(StatusCode::CREATED,Json(s))).map_err(|e|api_error(StatusCode::BAD_REQUEST,e.to_string()))}
async fn import_library_audio(State(state): State<AppState>, mut multipart: Multipart) -> Result<(StatusCode, Json<library::Song>), (StatusCode, Json<ApiError>)> {
    let mut title = None; let mut caption = String::new(); let mut lyrics = String::new(); let mut audio = None; let mut filename = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read import form: {e}")))? {
        let name = field.name().unwrap_or_default().to_owned();
        if name == "audio" { filename = field.file_name().map(str::to_owned); audio = Some(field.bytes().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read audio upload: {e}")))?.to_vec()); }
        else { let value = field.text().await.map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("read import field: {e}")))?; match name.as_str() { "title" => title = Some(value), "caption" => caption = value, "lyrics" => lyrics = value, _ => {} } }
    }
    let filename = filename.ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "audio file is required".into()))?;
    let extension = std::path::Path::new(&filename).extension().and_then(|value| value.to_str()).unwrap_or_default().to_owned();
    let title = title.filter(|value| !value.trim().is_empty()).unwrap_or_else(|| std::path::Path::new(&filename).file_stem().and_then(|value| value.to_str()).unwrap_or("Imported audio").to_owned());
    let audio = audio.ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "audio file is required".into()))?;
    let duration = library::audio_duration_seconds(&audio, &extension.to_ascii_lowercase(), None);
    let song = state.library.import_audio_song(library::AudioImportInput { title, caption, lyrics, metadata: serde_json::json!({"imported_filename": filename, "duration_seconds": duration}), generation_settings: Value::Null, engine_id: "imported-audio".into(), profile_id: None, source: "audio_import".into(), audio_extension: extension, audio }).map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?.song;
    Ok((StatusCode::CREATED, Json(song)))
}
async fn update_library_song(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<library::SongInput>)->Result<Json<library::Song>,(StatusCode,Json<ApiError>)>{
    let song = state.library.update_song(&id,input).map_err(|e|api_error(StatusCode::BAD_REQUEST,e.to_string()))?.ok_or_else(||api_error(StatusCode::NOT_FOUND,"Song not found".into()))?;
    // A rename is a title change, and the file carries the title.
    tag_stored_song(&state, &id).await;
    Ok(Json(song))
}
/// Removes a song and its files; a file that cannot go (a player holds it)
/// is named in the answer instead of only in the log.
async fn delete_library_song(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let song = state.library.get_song(&id).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !state.library.delete_song(&id).map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))? {
        return Err(api_error(StatusCode::NOT_FOUND, "Song not found".into()));
    }
    let mut left = Vec::new();
    if let Some(song) = song {
        for path in song_files(&state, &song) {
            if let Err(error) = std::fs::remove_file(&path) {
                eprintln!("[ERROR] delete song {id}: could not remove {}: {error}", path.display());
                left.push(format!("{}: {error}", plain_path(&path)));
            }
        }
    }
    Ok(Json(serde_json::json!({ "deleted": true, "files_left": left })))
}

/// The files in the media folder that belong to one song: its audio, its
/// stems and its cover. Anything outside the media folder is not ours to remove.
fn song_files(state: &AppState, song: &library::Song) -> Vec<PathBuf> {
    let media = state.library.media_dir();
    let mut files: Vec<PathBuf> = separation::STEMS.iter().map(|stem| stem_path(state, &song.id, stem)).collect();
    if let Some(audio) = state.library.media_path_for_song(song) {
        files.push(audio);
    }
    // a processed track owns its original and every version besides the one playing
    if let Some(original) = song.metadata.get("original_audio_path").and_then(Value::as_str).and_then(|path| state.library.resolve_media(path)) {
        files.push(original);
    }
    for version in song.metadata.get("audio_versions").and_then(Value::as_array).into_iter().flatten() {
        if let Some(file) = version.get("file").and_then(Value::as_str) {
            files.push(media.join(file));
        }
    }
    files.push(midi_path(state, &song.id));
    files.push(midi_sidecar(&midi_path(state, &song.id)));
    // the engine's latent, kept for an exact re-render
    files.push(latent_path(state, &song.id));
    if let Some((cover, _)) = state.library.cover_path_for_song(song) {
        files.push(cover);
    }
    files.retain(|path| in_media_folder(media, path));
    files.sort();
    files.dedup();
    files
}

/// Whether a file sits directly in the media folder, however either path is
/// written: the library resolves its paths to `\\?\F:\…` while the folder
/// is kept as `F:\…`, and comparing them as written removed nothing.
fn in_media_folder(media: &std::path::Path, path: &std::path::Path) -> bool {
    let (Ok(root), Ok(file)) = (media.canonicalize(), path.canonicalize()) else { return false };
    file.is_file() && file.parent() == Some(root.as_path())
}
async fn library_playlists(State(state):State<AppState>)->Result<Json<Vec<library::Playlist>>,(StatusCode,Json<ApiError>)>{state.library.list_playlists().map(Json).map_err(|e|api_error(StatusCode::INTERNAL_SERVER_ERROR,e.to_string()))}
async fn create_library_playlist(State(state):State<AppState>,Json(input):Json<library::PlaylistInput>)->Result<(StatusCode,Json<library::Playlist>),(StatusCode,Json<ApiError>)>{state.library.create_playlist(input).map(|p|(StatusCode::CREATED,Json(p))).map_err(|e|api_error(StatusCode::BAD_REQUEST,e.to_string()))}
async fn library_playlist(State(state):State<AppState>,Path(id):Path<String>)->Result<Json<library::Playlist>,(StatusCode,Json<ApiError>)>{state.library.get_playlist(&id).map_err(|e|api_error(StatusCode::INTERNAL_SERVER_ERROR,e.to_string()))?.map(Json).ok_or_else(||api_error(StatusCode::NOT_FOUND,"Playlist not found".into()))}
async fn update_library_playlist(State(state):State<AppState>,Path(id):Path<String>,Json(input):Json<library::PlaylistInput>)->Result<Json<library::Playlist>,(StatusCode,Json<ApiError>)>{state.library.update_playlist(&id,input).map_err(|e|api_error(StatusCode::BAD_REQUEST,e.to_string()))?.map(Json).ok_or_else(||api_error(StatusCode::NOT_FOUND,"Playlist not found".into()))}
async fn delete_library_playlist(State(state):State<AppState>,Path(id):Path<String>)->Result<StatusCode,(StatusCode,Json<ApiError>)>{if state.library.delete_playlist(&id).map_err(|e|api_error(StatusCode::INTERNAL_SERVER_ERROR,e.to_string()))?{Ok(StatusCode::NO_CONTENT)}else{Err(api_error(StatusCode::NOT_FOUND,"Playlist not found".into()))}}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    let engine_ready = state.music_server.health().await;
    // Which program is actually answering here, and whether it can start an
    // engine at all. A second copy of the studio - a development build, say -
    // takes this port and the window then waits forever on an engine that
    // copy has no bundle for. Saying whose service this is turns a hang into
    // a sentence.
    let executable = std::env::current_exe().ok().map(|path| path.display().to_string());
    let engine_available = engine_location(*state.engine_options.read().await).bundle_root.is_dir();
    Json(serde_json::json!({
        "status": "ok",
        "runtime": "native",
        "service_executable": executable,
        "engine_bundle_present": engine_available,
        "music_engine": {
            "id": PRIMARY_MUSIC_ENGINE_ID,
            "base_url": state.music_server.base_url(),
            "reachable": engine_ready,
        }
    }))
}

async fn configuration(State(state): State<AppState>) -> Json<StudioConfiguration> {
    Json(state.configuration.read().await.clone())
}

async fn update_configuration(
    State(state): State<AppState>,
    Json(update): Json<StudioConfiguration>,
) -> Json<StudioConfiguration> {
    // A page that changes one capability sends one selection. Storing the
    // request verbatim then erased every other choice - which is how a studio
    // with a downloaded engine started answering "the local music engine is not
    // configured" after the assistant was pointed at a local model.
    let configuration = {
        let mut stored = state.configuration.write().await;
        for selection in update.selections {
            match stored.selections.iter_mut().find(|existing| existing.capability == selection.capability) {
                Some(existing) => *existing = selection,
                None => stored.selections.push(selection),
            }
        }
        stored.clone()
    };

    // The choice has to reach the code that does the work, or the button is
    // decoration. Speech-to-text is done by the karaoke stack, and the writing
    // assistant has its own provider; both follow this page now.
    for selection in &configuration.selections {
        match selection.capability {
            Capability::SpeechToText => {
                let mut sync = state.lyrics_sync_config.write().await;
                sync.provider = match selection.mode {
                    ExecutionMode::OpenRouter => lyrics_sync::AsrProvider::OpenRouter,
                    ExecutionMode::Local => match selection.local_engine.as_deref() {
                        Some("whisper") => lyrics_sync::AsrProvider::Whisper,
                        Some("parakeet") => lyrics_sync::AsrProvider::Parakeet,
                        _ if state.lyrics_sync.parakeet_ready() => lyrics_sync::AsrProvider::Parakeet,
                        _ if state.lyrics_sync.whisper_binary().is_some() => lyrics_sync::AsrProvider::Whisper,
                        _ => sync.provider,
                    },
                };
                if selection.mode == ExecutionMode::OpenRouter {
                    sync.openrouter_model = selection.cloud_model.clone();
                }
            }
            Capability::PromptEnhancement => {
                let mut assistant = state.assistant.write().await;
                match selection.mode {
                    ExecutionMode::OpenRouter => {
                        assistant.provider = AssistantProvider::OpenRouter;
                        if let Some(model) = selection.cloud_model.clone() {
                            assistant.openrouter_model = Some(model);
                        }
                    }
                    ExecutionMode::Local if assistant.provider == AssistantProvider::Agent => {}
                    ExecutionMode::Local => {
                        // Whichever local shape is set up: a managed model the
                        // studio downloaded, or a server the user runs.
                        assistant.provider = if assistant.managed_model.is_some() || assistant.managed_path.is_some() {
                            AssistantProvider::Managed
                        } else {
                            AssistantProvider::Local
                        };
                    }
                }
            }
            _ => {}
        }
    }

    let _ = persist_studio_settings(&state).await;
    Json(configuration)
}

async fn engine_options(State(state): State<AppState>) -> Json<Value> {
    let options = *state.engine_options.read().await;
    Json(serde_json::json!({
        "options": options,
        "effective_max_batch": options.effective_max_batch(),
        "restart_required_to_apply": true,
    }))
}

/// Stores the launch flags and restarts the engine if it is running, because
/// upstream reads them once at startup.
async fn update_engine_options(
    State(state): State<AppState>,
    Json(request): Json<EngineOptions>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    if request.max_batch.is_some_and(|value| value == 0 || value > 8) {
        return Err(api_error(StatusCode::BAD_REQUEST, "max_batch must be between 1 and 8".into()));
    }
    if request.max_seq.is_some_and(|value| value < 512) {
        return Err(api_error(StatusCode::BAD_REQUEST, "max_seq must be at least 512".into()));
    }
    let changed = {
        let mut options = state.engine_options.write().await;
        let changed = *options != request;
        *options = request;
        changed
    };
    let _ = persist_studio_settings(&state).await;

    let mut restarted = false;
    if changed && state.music_server.health().await {
        let _restart = state.engine_use.write().await;
        restart_engine(&state).await.map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error))?;
        restarted = true;
    }
    Ok(Json(serde_json::json!({
        "options": request,
        "effective_max_batch": request.effective_max_batch(),
        "engine_restarted": restarted,
    })))
}

/// Makes the engine find every model and LoRA on disk again. It reads its
/// folders once, when it starts, so a file that arrived later - a download, a
/// file put there by hand - is found by restarting it, after the work in
/// flight is done; new work waits for it.
async fn rescan_resources(state: &AppState) -> Result<Value, String> {
    let _rescan = state.engine_use.write().await;
    // a training run or a dataset preparation holds the card; the engine finds
    // everything when it comes back after them
    let preparing = state.prepare.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().is_some_and(|job| !job.finished);
    if preparing || state.training.active_run().await.is_some() || !state.music_server.health().await {
        return Ok(serde_json::json!({ "restarted": false, "message": "The engine is not running; it finds every model and LoRA when it starts." }));
    }
    restart_engine(state).await?;
    mcp::announce("resources_rescanned");
    let props = state.music_server.props().await.map_err(|error| format!("the engine did not answer after the rescan: {error}"))?;
    Ok(serde_json::json!({ "restarted": true, "models": props["models"], "adapters": props["adapters"] }))
}

async fn rescan_resources_route(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    rescan_resources(&state).await.map(Json).map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error))
}

async fn restart_local_engine(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let _restart = state.engine_use.write().await;
    restart_engine(&state).await.map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error))?;
    Ok(Json(serde_json::json!({ "engine_id": PRIMARY_MUSIC_ENGINE_ID, "restarted": true })))
}

async fn restart_engine(state: &AppState) -> Result<(), String> {
    let mut supervisor = state.engine.lock().await;
    let owned = supervisor.is_some();
    if let Some(engine) = supervisor.as_mut() {
        tokio::task::block_in_place(|| engine.stop(std::time::Duration::from_secs(10)))
            .map_err(|error| format!("stopping the local engine failed: {error}"))?;
    }
    // Launch flags are read once at engine startup. If something this service
    // does not own is still listening, starting again would silently reuse it
    // and the new flags would never take effect — report that instead of
    // claiming a restart that did not happen.
    if !owned && state.music_server.health().await {
        return Err(
            "An engine that this application did not start is already running on the engine port.              Close it and try again, otherwise the new options cannot be applied."
                .into(),
        );
    }
    // Nothing can start until the libraries the engine binary imports are on
    // disk: Windows resolves them before the process runs, so a missing cuBLAS
    // is not a slow start, it is no start at all. This is the path the studio
    // actually takes on launch, so the fetch belongs here rather than only in
    // the endpoint nothing calls.
    let options = *state.engine_options.read().await;
    if options.backend == music_engine::server::ComputeBackend::Cuda && cuda_build::current().is_none() {
        return Err(cuda_build::UNSUPPORTED.into());
    }
    let chain = options.device_chain(&state.failed_devices.read().await);
    let mut last_error = String::new();
    for (index, device) in chain.iter().copied().enumerate() {
        let attempt = EngineOptions { backend: device, ..options };
        match attempt.cuda_build() {
            Some(build) if !state.engine_runtime.is_ready(build) => {
                state
                    .engine_runtime
                    .install_missing(build)
                    .await
                    .map_err(|error| format!("the engine's CUDA libraries could not be downloaded: {error}"))?;
            }
            Some(_) => {}
            None => {
                let absent = state.engine_runtime.vc_runtime_missing();
                if !absent.is_empty() {
                    return Err(format!("the engine bundle is incomplete: {} missing beside the engine; reinstall the studio", absent.join(", ")));
                }
            }
        }
        // The engine loads eleven gigabytes of weights the moment it starts.
        // If the writing assistant is still holding the card, it does not finish.
        free_the_card_for_the_engine(state).await;
        let config = engine_location(attempt)
            .resolve()
            .map_err(|error| format!("the local engine runtime was not found: {error}"))?;
        let mut engine = music_engine::server::ServerSupervisor::new(config).map_err(|error| error.to_string())?;
        match tokio::task::block_in_place(|| engine.ensure_started(std::time::Duration::from_secs(60))) {
            Ok(_) => {
                *supervisor = Some(engine);
                *state.active_device.write().await = Some(device);
                if options.backend == music_engine::server::ComputeBackend::Auto {
                    music_engine::server::note_in_log(&format!("computing on {}", device_name(device)));
                }
                return Ok(());
            }
            Err(error) => {
                last_error = format!("the local engine did not start: {error}");
                // Only Auto moves on, and never past a card that ran out of
                // memory: Vulkan has no more of it, and the reason is shown.
                let next = chain.get(index + 1).copied();
                let out_of_memory = describes_exhausted_memory(&last_run_log().to_lowercase());
                match next {
                    Some(next) if options.backend == music_engine::server::ComputeBackend::Auto && !out_of_memory => {
                        music_engine::server::note_in_log(&format!(
                            "{} did not start ({error}); trying {}",
                            device_name(device),
                            device_name(next)
                        ));
                        state.failed_devices.write().await.push(device);
                    }
                    _ => return Err(last_error),
                }
            }
        }
    }
    Err(last_error)
}

/// The device's name as the settings show it.
fn device_name(device: music_engine::server::ComputeBackend) -> &'static str {
    use music_engine::server::ComputeBackend;
    match device {
        ComputeBackend::Auto => "Auto",
        ComputeBackend::Cuda => "CUDA",
        ComputeBackend::Vulkan => "Vulkan",
        ComputeBackend::Cpu => "the processor",
    }
}

/// The engine log since the last start, where the reason a run ended is.
fn last_run_log() -> String {
    let tail = music_engine::server::startup_log_tail(400);
    let start = tail.iter().rposition(|line| line.contains("---- starting")).unwrap_or(0);
    tail[start..].join("\n")
}

/// Whether the engine ended without a word about why. Its own failures -
/// an assertion, an error, running out of memory - are written before it
/// goes; a device lost under the driver takes the process with nothing said.
fn died_silently(log: &str) -> bool {
    !["error", "assert", "fatal", "exception", "abort", "failed", "out of memory"]
        .iter()
        .any(|marker| log.contains(marker))
}

/// Whether a lowercased engine log says the compute device itself failed -
/// a CUDA or Vulkan error, a device lost, the engine's self-test - as opposed
/// to running out of memory, which another device would not cure.
fn describes_device_failure(log: &str) -> bool {
    if describes_exhausted_memory(log) {
        return false;
    }
    [
        "cuda error",
        "no kernel image",
        "unsupported toolchain",
        "ggml_cuda_compute_forward",
        "fatal: self-test",
        "_cuda_backend=",
        "devicelost",
        "device lost",
        "vk::",
        "ggml_vulkan: error",
    ]
    .iter()
    .any(|marker| log.contains(marker))
}

/// Where the packaged or developer-built `ace-server` lives. Every value is an
/// explicit override or a documented default; nothing is downloaded here.
fn engine_bundle_root() -> PathBuf {
    env::var_os("STUDIO_ENGINE_ROOT")
        .map(PathBuf::from)
        .or_else(|| env::var_os("STUDIO_ENGINE_BIN").map(PathBuf::from).and_then(|path| path.parent().map(std::path::Path::to_path_buf)))
        .or_else(|| std::env::current_exe().ok().and_then(|path| path.parent().map(|parent| parent.join("resources").join("engine"))))
        .unwrap_or_else(|| PathBuf::from("resources/engine"))
}

fn engine_location(options: EngineOptions) -> music_engine::server::ServerLocation {
    let configured_executable = env::var_os("STUDIO_ENGINE_BIN").map(PathBuf::from);
    let bundle_root = engine_bundle_root();
    music_engine::server::ServerLocation {
        bundle_root,
        configured_executable,
        // The GGUFs live where the model manager put them, which is not
        // inside the engine bundle: pointing the service at a developer build
        // of the engine used to leave it looking for models next to the binary
        // and failing with "models root is not a directory".
        configured_models_root: env::var_os("STUDIO_MODELS_ROOT")
            .map(PathBuf::from)
            .or_else(|| {
                let managed = studio_data_root()?.join("models").join(model_manager::ENGINE_ID);
                managed.is_dir().then_some(managed)
            }),
        adapters_root: studio_data_root().map(|root| root.join("adapters")).filter(|folder| match std::fs::create_dir_all(folder) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("[ERROR] the adapter folder {} cannot be created, the engine starts without adapters: {error}", folder.display());
                false
            }
        }),
        host: env::var("STUDIO_ENGINE_HOST").ok(),
        port: env::var("STUDIO_ENGINE_PORT").ok().and_then(|value| value.parse().ok()),
        options: options.to_engine(),
    }
}

async fn engine_presets(State(state): State<AppState>) -> Json<Value> {
    let catalog = state.openrouter_catalog.read().await;
    let music_available = catalog.catalog.as_ref().is_some_and(|catalog| catalog.models_for(Capability::MusicGeneration).next().is_some());
    Json(serde_json::json!({
        "presets": presets::list(),
        "hardware": presets::hardware(),
        "selected_profile_id": state.selected_profile_id.read().await.clone(),
        "openrouter_music": { "available": music_available, "refreshed_at": catalog.refreshed_at, "requires_live_catalog_resolution": !music_available }
    }))
}

async fn apply_engine_preset(
    State(state): State<AppState>,
    Json(request): Json<ApplyPresetRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let mut configuration = state.configuration.write().await;
    let music_available = state.openrouter_catalog.read().await.catalog.as_ref().is_some_and(|catalog| catalog.models_for(Capability::MusicGeneration).next().is_some());
    let application = presets::apply(&request.id, &mut configuration, music_available)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error))?;
    if let Some(profile_id) = &application.profile_id {
        if !state.model_manager.catalog().profiles.iter().any(|profile| profile.id == profile_id && profile.installable) {
            return Err(api_error(StatusCode::BAD_REQUEST, format!("preset profile '{profile_id}' is not installable")));
        }
    }
    let applied_configuration = configuration.clone();
    drop(configuration);
    if application.selected_profile_changed {
        *state.selected_profile_id.write().await = application.profile_id;
        *state.selected_component_ids.write().await = None;
    }
    let selected_profile_id = state.selected_profile_id.read().await.clone();
    let selected_component_ids = state.selected_component_ids.read().await.clone();
    persist_studio_settings(&state)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "id": request.id,
        "profile_id": selected_profile_id,
        "component_ids": selected_component_ids,
        "configuration": applied_configuration,
        "openrouter_music_requires_live_catalog_resolution": request.id == "full-openrouter" && !music_available,
    })))
}

async fn openrouter_catalog(State(state): State<AppState>) -> Json<Value> {
    // Fetch it if this process has not yet: an empty answer here made every
    // capability read "no model in the refreshed catalog", which is a lie -
    // the catalog had simply never been read.
    let catalog = catalog_for(&state).await.ok();
    let refreshed_at = state.openrouter_catalog.read().await.refreshed_at.clone();
    // What the studio would pick for each capability if the user picks
    // nothing. The panel shows these as the selection, so adding a key is
    // enough to start rather than the beginning of a shopping trip.
    let suggested = catalog.as_ref().map(|catalog| {
        serde_json::json!({
            "speech_to_text": providers::openrouter::suggested_model(catalog, Capability::SpeechToText),
            "prompt_enhancement": providers::openrouter::suggested_model(catalog, Capability::PromptEnhancement),
            "cover_art": providers::openrouter::suggested_model(catalog, Capability::CoverArt),
            "music_generation": providers::openrouter::suggested_model(catalog, Capability::MusicGeneration),
        })
    });
    Json(serde_json::json!({
        "models": catalog.map(|catalog| catalog.models),
        "refreshed_at": refreshed_at,
        "suggested": suggested,
    }))
}



/// Where the provider catalog is kept between runs.
fn openrouter_catalog_path() -> Option<PathBuf> {
    studio_data_root().map(|root| root.join("openrouter-catalog.json"))
}

/// Reads the catalog saved by the last refresh. Nothing here touches the
/// network: the studio refreshes when a key is connected and when the user
/// asks, and lives off this file the rest of the time.
fn load_cached_catalog() -> Option<providers::openrouter::CapabilityCatalog> {
    let path = openrouter_catalog_path()?;
    let body = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&body).ok()
}

fn save_cached_catalog(catalog: &providers::openrouter::CapabilityCatalog) {
    let Some(path) = openrouter_catalog_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(body) = serde_json::to_string(catalog) {
        let _ = std::fs::write(path, body);
    }
}

/// The capability catalog, fetched if this process has not got it yet.
///
/// The catalog lives in memory, so it is empty after every restart. Telling
/// the user to "refresh the catalog" at the moment they press a button is
/// asking them to do the program's job - and the refresh button lives on
/// another screen entirely.
/// The catalogue, and a fresh one when the model asked about is not in it.
///
/// The record is what the request is built from - the model's own parameters,
/// the efforts it accepts - so a catalogue that predates the model would have
/// the studio guessing about a model OpenRouter can describe exactly.
async fn catalog_describing(state: &AppState, model: &str) -> Result<providers::openrouter::CapabilityCatalog, String> {
    let catalog = catalog_for(state).await?;
    if model.is_empty() || catalog.models.iter().any(|entry| entry.id == model) {
        return Ok(catalog);
    }
    {
        let mut cached = state.openrouter_catalog.write().await;
        cached.catalog = None;
    }
    catalog_for(state).await
}

async fn catalog_for(state: &AppState) -> Result<providers::openrouter::CapabilityCatalog, String> {
    if let Some(catalog) = state.openrouter_catalog.read().await.catalog.clone() {
        return Ok(catalog);
    }
    // The last refresh, read from disk. A restart should not cost a request.
    if let Some(catalog) = load_cached_catalog() {
        let mut cached = state.openrouter_catalog.write().await;
        cached.catalog = Some(catalog.clone());
        return Ok(catalog);
    }
    let client = reqwest::Client::new();
    let fetch = |path: &'static str| {
        let client = client.clone();
        async move {
            client
                .get(format!("{}{}", providers::openrouter::API_BASE_URL, path))
                .send()
                .await?
                .error_for_status()?
                .text()
                .await
        }
    };
    let general = fetch(providers::openrouter::MODELS_PATH)
        .await
        .map_err(|error| format!("OpenRouter catalog request failed: {error}"))?;
    let transcription = fetch(providers::openrouter::TRANSCRIPTION_MODELS_PATH).await.unwrap_or_default();
    let images = fetch(providers::openrouter::IMAGE_MODELS_PATH).await.unwrap_or_default();
    let parsed = providers::openrouter::CapabilityCatalog::parse_merged(&general, &[&transcription, &images])
    .map_err(|error| format!("OpenRouter catalog parse failed: {error}"))?;
    save_cached_catalog(&parsed);
    let mut cached = state.openrouter_catalog.write().await;
    cached.catalog = Some(parsed.clone());
    cached.refreshed_at = Some(chrono_like_timestamp());
    Ok(parsed)
}

async fn refresh_openrouter_catalog(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let client = reqwest::Client::new();
    let fetch = |path: &'static str| {
        let client = client.clone();
        async move {
            client
                .get(format!("{}{}", providers::openrouter::API_BASE_URL, path))
                .send()
                .await?
                .error_for_status()?
                .text()
                .await
        }
    };
    let general = fetch(providers::openrouter::MODELS_PATH)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter catalog request failed: {error}")))?;
    // The recognisers live behind their own filter; without this second call
    // the catalog contains no model that can return timings.
    let transcription = fetch(providers::openrouter::TRANSCRIPTION_MODELS_PATH).await.unwrap_or_default();
    let images = fetch(providers::openrouter::IMAGE_MODELS_PATH).await.unwrap_or_default();
    let parsed = providers::openrouter::CapabilityCatalog::parse_merged(&general, &[&transcription, &images])
    .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter catalog parse failed: {error}")))?;
    let refreshed_at = chrono_like_timestamp();
    let models = parsed.models.clone();
    save_cached_catalog(&parsed);
    let mut cached = state.openrouter_catalog.write().await;
    cached.catalog = Some(parsed);
    cached.refreshed_at = Some(refreshed_at.clone());
    Ok(Json(serde_json::json!({ "models": models, "refreshed_at": refreshed_at })))
}

/// Sends an already catalog-validated OpenRouter JSON request. The frontend
/// supplies a model selection and input data, never an API key or endpoint.
async fn execute_openrouter_json(
    request: providers::openrouter::OpenRouterRequest,
) -> anyhow::Result<OpenRouterResponse> {
    let authenticated = providers::openrouter::authenticated_request_for(request)?;
    // Every cloud request passes through here, so this is where they are all
    // written down: which model, how long, and what came back when it was not
    // a success.
    let what = authenticated.request.path.trim_matches('/');
    let model = authenticated
        .request
        .body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string();
    request_log::asked(what, &model, authenticated.request.body.to_string().chars().count());
    let started = std::time::Instant::now();
    let response = reqwest::Client::new()
        .post(format!("{}{}", providers::openrouter::API_BASE_URL, authenticated.request.path))
        .bearer_auth(authenticated.api_key)
        .json(&authenticated.request.body)
        .send()
        .await
        .inspect_err(|error| request_log::failed(what, &model, &error.to_string()))?;
    let status = response.status();
    let response = match response.error_for_status_ref() {
        Ok(_) => response,
        Err(error) => {
            let text = response.text().await.unwrap_or_default();
            request_log::answered(what, &model, status.as_u16(), started.elapsed().as_secs_f64(), text.chars().count());
            request_log::unusable(what, &model, &error.to_string(), &text);
            return Err(error.into());
        }
    };
    let generation_id = response
        .headers()
        .get("X-Generation-Id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response.json::<Value>().await?;
    request_log::answered(what, &model, status.as_u16(), started.elapsed().as_secs_f64(), body.to_string().chars().count());
    Ok(OpenRouterResponse { body, generation_id })
}

async fn create_openrouter_transcription(
    State(state): State<AppState>,
    Json(input): Json<OpenRouterTranscriptionRequest>,
) -> Result<Json<OpenRouterResponse>, (StatusCode, Json<ApiError>)> {
    let catalog = catalog_for(&state)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))?;
    let request = providers::openrouter::stt_request_for(
        &catalog,
        &input.model_id,
        providers::openrouter::Base64AudioInput {
            timestamps: false,
            data: &input.audio_base64,
            format: &input.audio_format,
            language: input.language.as_deref(),
        },
    )
    .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    execute_openrouter_json(request)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter transcription failed: {error}")))
}

async fn create_openrouter_cover(
    State(state): State<AppState>,
    Json(input): Json<OpenRouterCoverRequest>,
) -> Result<Json<OpenRouterResponse>, (StatusCode, Json<ApiError>)> {
    let catalog = catalog_for(&state)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))?;
    let request = providers::openrouter::request_for(
        &catalog,
        Capability::CoverArt,
        &input.model_id,
        &input.prompt,
    )
    .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    execute_openrouter_json(request)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter cover generation failed: {error}")))
}

/// Text assistance (caption and lyric drafting). The model must declare the
/// prompt-enhancement capability in the refreshed catalog, so the studio can
/// never send this to an image or audio-only endpoint.
async fn create_openrouter_completion(
    State(state): State<AppState>,
    Json(input): Json<OpenRouterCompletionRequest>,
) -> Result<Json<OpenRouterResponse>, (StatusCode, Json<ApiError>)> {
    let catalog = catalog_for(&state)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))?;
    let request = providers::openrouter::request_for(
        &catalog,
        Capability::PromptEnhancement,
        &input.model_id,
        &input.prompt,
    )
    .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    execute_openrouter_json(request)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter completion failed: {error}")))
}

/// The loopback port the studio serves on. Overridable for development, so a
/// second instance can run beside a released one.
fn listen_port() -> u16 {
    env::var("STUDIO_PORT").ok().and_then(|value| value.parse().ok()).unwrap_or(music_core::studio().port)
}

fn chrono_like_timestamp() -> String { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_secs().to_string()).unwrap_or_default() }

fn studio_settings_path() -> PathBuf {
    env::var_os("STUDIO_SETTINGS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_studio_settings_path)
}

fn default_studio_settings_path() -> PathBuf {
    studio_data_root()
        .unwrap_or_else(|| env::temp_dir().join(&music_core::studio().slug))
        .join("studio-settings.json")
}

/// Single per-user directory for every piece of Studio runtime data: settings,
/// library, media and locally stored provider credentials.
static STUDIO_VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The studio's own version, set by the desktop shell before the service
/// starts; a service run on its own is a development build of its crate.
pub fn set_studio_version(version: String) {
    let _ = STUDIO_VERSION.set(version);
}

pub(crate) fn studio_version() -> &'static str {
    STUDIO_VERSION.get().map(String::as_str).unwrap_or(env!("CARGO_PKG_VERSION"))
}

pub fn studio_data_root() -> Option<PathBuf> {
    if let Some(root) = env::var_os("STUDIO_DATA_ROOT") {
        return Some(PathBuf::from(root));
    }

    #[cfg(windows)]
    {
        if let Some(root) = env::var_os("LOCALAPPDATA").or_else(|| env::var_os("APPDATA")) {
            return Some(PathBuf::from(root).join(&music_core::studio().name));
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(root) = env::var_os("XDG_DATA_HOME") {
            return Some(PathBuf::from(root).join(&music_core::studio().slug));
        }
        if let Some(home) = env::var_os("HOME") {
            return Some(PathBuf::from(home).join(".local/share").join(&music_core::studio().slug));
        }
    }

    None
}

fn load_studio_settings(path: &PathBuf) -> Option<PersistedStudioSettings> {
    fs::read_to_string(path).ok().and_then(|body| serde_json::from_str(&body).ok())
}

async fn persist_studio_settings(state: &AppState) -> anyhow::Result<()> {
    let settings = PersistedStudioSettings {
        engine_options: *state.engine_options.read().await,
        assistant: state.assistant.read().await.clone(),
        lyrics_sync: state.lyrics_sync_config.read().await.clone(),
        configuration: state.configuration.read().await.clone(),
        selected_profile_id: state.selected_profile_id.read().await.clone(),
        selected_component_ids: state.selected_component_ids.read().await.clone(),
        cover_templates: Some(state.cover_templates.read().await.clone()),
        cover_template_default: state.cover_template_default.read().await.clone(),
        separation: Some(state.separation_config.read().await.clone()),
        cover_auto: Some(*state.cover_auto.read().await),
    };
    if let Some(parent) = state.settings_path.parent() { fs::create_dir_all(parent)?; }
    let temporary = state.settings_path.with_extension("json.part");
    fs::write(&temporary, serde_json::to_vec_pretty(&settings)?)?;
    fs::rename(temporary, &state.settings_path)?;
    Ok(())
}

/// The set Studio will actually load. `None` means nothing has been selected
/// yet, so the manager falls back to the hardware recommendation for progress
/// reporting only — it still never downloads anything on its own.
async fn effective_install_target(state: &AppState) -> Option<InstallRequest> {
    if let Some(component_ids) = state.selected_component_ids.read().await.clone() {
        return Some(InstallRequest { profile_id: None, component_ids });
    }
    state
        .selected_profile_id
        .read()
        .await
        .clone()
        .map(|profile_id| InstallRequest { profile_id: Some(profile_id), component_ids: vec![] })
}

async fn compose_setup_status(state: &AppState, manager_status: model_manager::ManagerStatus) -> Value {
    let mut status = serde_json::to_value(manager_status).unwrap_or_else(|_| serde_json::json!({}));
    let selected_profile_id = state.selected_profile_id.read().await.clone();
    let selected_component_ids = state.selected_component_ids.read().await.clone();
    let selected_set_ready = match (&selected_profile_id, &selected_component_ids) {
        (_, Some(component_ids)) => state.model_manager.installed_component_files(component_ids).is_ok(),
        (Some(profile_id), None) => state.model_manager.installed_profile_files(profile_id).is_ok(),
        (None, None) => false,
    };
    if let Value::Object(ref mut fields) = status {
        fields.insert(
            "engine_ready".into(),
            Value::Bool(state.music_server.health().await),
        );
        fields.insert("engine_id".into(), Value::String(PRIMARY_MUSIC_ENGINE_ID.into()));
        fields.insert("selected_profile_id".into(), serde_json::to_value(selected_profile_id).unwrap_or(Value::Null));
        fields.insert("selected_component_ids".into(), serde_json::to_value(selected_component_ids).unwrap_or(Value::Null));
        fields.insert("hardware".into(), serde_json::to_value(presets::hardware()).unwrap_or(Value::Null));
        fields.insert("engine_options".into(), serde_json::to_value(*state.engine_options.read().await).unwrap_or(Value::Null));
        fields.insert("effective_max_batch".into(), Value::from(state.engine_options.read().await.effective_max_batch()));
        // Where everything the studio owns actually lives. People complained
        // they could not find the ten gigabytes afterwards, let alone delete
        // them; the model root is already reported, this is the folder that
        // holds it along with the library, the media and the logs.
        fields.insert(
            "data_directory".into(),
            studio_data_root().map(|root| Value::String(root.display().to_string())).unwrap_or(Value::Null),
        );
        fields.insert("portable".into(), Value::Bool(is_portable_installation()));
        // Half a gigabyte of CUDA libraries arriving is the difference between
        // an engine that starts in three seconds and one that starts in ten
        // minutes. A spinner that says nothing for ten minutes is the same
        // screen as a spinner that is stuck.
        let runtime_build = state.engine_options.read().await.cuda_build();
        let runtime_total = runtime_build.map(|build| engine_runtime::cublas_asset(build).bytes).unwrap_or(0);
        let runtime_active = state.engine_runtime.downloader().active().await;
        fields.insert(
            "engine_runtime".into(),
            serde_json::json!({
                "ready": match runtime_build {
                    Some(build) => state.engine_runtime.is_ready(build),
                    None => state.engine_runtime.vc_runtime_missing().is_empty(),
                },
                "downloading": runtime_active.is_some(),
                "downloaded_bytes": runtime_active.as_ref().map(|progress| progress.downloaded_bytes).unwrap_or(0),
                "total_bytes": runtime_total,
                "error": runtime_active.and_then(|progress| progress.error),
            }),
        );
        fields.insert("ready".into(), Value::Bool(selected_set_ready));
        fields.insert("first_run".into(), Value::Bool(!selected_set_ready));
        if selected_set_ready { fields.insert("download_pending".into(), Value::from(0_u64)); }
    }
    status
}

/// Whether this copy keeps everything beside its own executable.
///
/// The desktop shell decides it by the marker file next to the binary and then
/// hands the service the data root; the service reports it so the interface can
/// say "this folder is the whole studio" rather than sending people hunting
/// through AppData.
fn is_portable_installation() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|directory| directory.join("portable.flag")))
        .is_some_and(|marker| marker.is_file())
}

/// Recent native engine output. This is the only progress detail upstream
/// exposes: `/job` reports a phase, and everything finer lives in the log ring.
async fn engine_logs(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    // While the engine is starting it has no HTTP log yet, so the file it
    // writes from its first line is the only thing that can be shown - and it
    // is exactly what the first-run screen needs.
    match state.music_server.logs_snapshot(std::time::Duration::from_millis(700)).await {
        Ok(lines) => Ok(Json(serde_json::json!({ "engine_id": PRIMARY_MUSIC_ENGINE_ID, "lines": lines }))),
        Err(error) => {
            let lines = music_engine::server::startup_log_tail(60);
            if lines.is_empty() {
                return Err(api_error(StatusCode::SERVICE_UNAVAILABLE, format!("the engine's logs are unavailable: {error}")));
            }
            Ok(Json(serde_json::json!({ "engine_id": PRIMARY_MUSIC_ENGINE_ID, "lines": lines, "source": "startup" })))
        }
    }
}

/// Live machine resources. ACE Studio's resource readout is kept, but every
/// value now comes from a real measurement on this machine.
async fn system_resources() -> Json<Value> {
    let snapshot = tokio::task::spawn_blocking(resources::snapshot)
        .await
        .unwrap_or_else(|_| resources::snapshot());
    Json(serde_json::json!({
        "poll_interval_ms": resources::SUGGESTED_INTERVAL.as_millis() as u64,
        "resources": snapshot,
    }))
}

/// Fetches a remote image on behalf of the video composer.
///
/// The canvas has to stay untainted to read frames back, which a cross-origin
/// image without CORS headers prevents. Only http(s) is accepted and the
/// response must actually be an image, so this cannot be used to reach local
/// services or to pull arbitrary files.
async fn proxy_image(
    State(state): State<AppState>,
    axum::extract::Query(request): axum::extract::Query<ProxyImageRequest>,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let url = reqwest::Url::parse(&request.url)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("invalid image url: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(api_error(StatusCode::BAD_REQUEST, "only http and https images can be proxied".into()));
    }
    if url.host_str().is_some_and(|host| host == "localhost" || host.starts_with("127.") || host == "0.0.0.0" || host == "[::1]") {
        return Err(api_error(StatusCode::BAD_REQUEST, "loopback addresses cannot be proxied".into()));
    }
    let response = state
        .music_server
        .http()
        .get(url)
        .send()
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("image request failed: {error}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(api_error(StatusCode::BAD_GATEWAY, format!("image request returned {status}")));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    if !content_type.starts_with("image/") {
        return Err(api_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "the proxied url is not an image".into()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("reading the image failed: {error}")))?;
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes.to_vec()))
        .expect("valid image response"))
}

async fn assistant_status(State(state): State<AppState>) -> Json<Value> {
    let config = state.assistant.read().await.clone();
    let runtime = state.assistant_runtime.status().await;
    // A managed model is only usable once its file and the runtime are on disk.
    let available = match config.provider {
        AssistantProvider::Managed => {
            let has_runtime = runtime.server_path.is_some();
            let downloaded = config
                .managed_model
                .as_deref()
                .is_some_and(|model| runtime.installed_models.iter().any(|id| id == model));
            let own_file = config
                .managed_path
                .as_deref()
                .is_some_and(|path| !path.trim().is_empty() && std::path::Path::new(path.trim()).is_file());
            has_runtime && (downloaded || own_file)
        }
        _ => config.available(),
    };
    Json(serde_json::json!({
        "available": available,
        "managed_model": config.managed_model,
        "managed_path": config.managed_path,
        "reasoning_effort": config.reasoning_effort,
        "runtime_ready": runtime.ready,
        "provider": config.provider,
        "local_base_url": config.local_base_url,
        "local_model": config.local_model,
        "openrouter_model": config.openrouter_model,
    }))
}

/// Every field optional, so the download page can set the provider without
/// blanking the model, the path and the reasoning effort it knows nothing of.
#[derive(Debug, Deserialize)]
struct AssistantSettingsRequest {
    provider: Option<AssistantProvider>,
    local_base_url: Option<Option<String>>,
    local_model: Option<Option<String>>,
    openrouter_model: Option<Option<String>>,
    managed_model: Option<Option<String>>,
    managed_path: Option<Option<String>>,
    reasoning_effort: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct LocalModelsQuery {
    base: String,
}

/// The models an OpenAI-compatible server the user runs offers, fetched through
/// the studio so the browser is never asked to reach another origin itself.
///
/// LM Studio, llama-server, Ollama's OpenAI shim - all answer `GET <base>/models`
/// with `{ "data": [ { "id": ... } ] }`. Typing the model name by hand, which
/// is what this replaces, meant a typo read as a server that answered nothing.
async fn assistant_local_models(
    axum::extract::Query(query): axum::extract::Query<LocalModelsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let base = query.base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "no server address".into()));
    }
    let url = format!("{base}/models");
    let response = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("the server at {base} did not answer: {error}")))?;
    if !response.status().is_success() {
        return Err(api_error(StatusCode::BAD_GATEWAY, format!("{url} answered {}", response.status())));
    }
    let body = response
        .json::<Value>()
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("{url} did not return JSON: {error}")))?;
    let models: Vec<String> = body
        .get("data")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string)).collect())
        .unwrap_or_default();
    Ok(Json(serde_json::json!({ "models": models })))
}

async fn update_assistant_settings(
    State(state): State<AppState>,
    Json(incoming): Json<AssistantSettingsRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let request = {
        let current = state.assistant.read().await.clone();
        AssistantConfig {
            provider: incoming.provider.unwrap_or(current.provider),
            local_base_url: incoming.local_base_url.unwrap_or(current.local_base_url),
            local_model: incoming.local_model.unwrap_or(current.local_model),
            openrouter_model: incoming.openrouter_model.unwrap_or(current.openrouter_model),
            managed_model: incoming.managed_model.unwrap_or(current.managed_model),
            managed_path: incoming.managed_path.unwrap_or(current.managed_path),
            reasoning_effort: incoming.reasoning_effort.unwrap_or(current.reasoning_effort),
        }
    };
    if request.provider == AssistantProvider::Local {
        let base = request.local_base_url.as_deref().unwrap_or_default();
        let url = reqwest::Url::parse(base)
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, format!("invalid assistant URL: {error}")))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(api_error(StatusCode::BAD_REQUEST, "the assistant URL must be http or https".into()));
        }
    }
    *state.assistant.write().await = request.clone();
    let _ = persist_studio_settings(&state).await;
    Ok(Json(serde_json::json!({ "available": request.available(), "provider": request.provider })))
}

#[derive(Debug, Deserialize)]
struct AssistantAssetRequest {
    asset_id: String,
    /// The model to install along with the runtime, when the capability has a
    /// choice of them. A runtime with no model does nothing.
    #[serde(default)]
    model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AssistantModelRequest {
    #[serde(default)]
    model_id: String,
    /// A GGUF already on this machine, used instead of a downloaded one.
    #[serde(default)]
    model_path: Option<String>,
}

/// The runtime, and the choice it belongs to.
///
/// The panel reads this one address to draw itself, and the chosen engine is
/// kept with the assistant's settings rather than with its files - so without
/// it here the tabs came back to the first one on every open, whatever the user
/// had picked.
async fn assistant_runtime_status(State(state): State<AppState>) -> Json<Value> {
    let status = state.assistant_runtime.status().await;
    let provider = state.assistant.read().await.provider;
    let mut value = serde_json::to_value(&status).unwrap_or(Value::Null);
    let chosen = state.assistant.read().await.managed_model.clone();
    if let Value::Object(ref mut fields) = value {
        fields.insert("provider".into(), serde_json::to_value(provider).unwrap_or(Value::Null));
        // And which model, so the dropdown reopens on the one that was picked
        // rather than on whichever happens to be installed first.
        fields.insert("chosen_model".into(), serde_json::to_value(chosen).unwrap_or(Value::Null));
    }
    Json(value)
}

/// Starts one download. Nothing is fetched until this is called, and an
/// interrupted file resumes where it stopped.
/// The llama.cpp build for a device, with the CUDA libraries it needs.
///
/// The card build is useless without its runtime companion - two downloads
/// that are one decision, the same way a recogniser is.
fn assistant_set(device: &str) -> Vec<&'static str> {
    match device {
        "cpu" => vec!["llama-cpu"],
        _ => vec!["llama-cuda", "llama-cuda-runtime"],
    }
}

async fn assistant_runtime_install(
    State(state): State<AppState>,
    Json(request): Json<AssistantAssetRequest>,
) -> Result<Json<assistant_runtime::RuntimeStatus>, (StatusCode, Json<ApiError>)> {
    let set = match request.asset_id.as_str() {
        "auto" | "cuda" | "cpu" => assistant_set(&request.asset_id),
        _ => Vec::new(),
    };
    if !set.is_empty() {
        // Downloading a model is choosing it. Nothing else recorded which one,
        // so a freshly installed Gemma left the studio reporting that no
        // assistant was set up - with the model sitting on the disk.
        if let Some(model) = request.model_id.clone() {
            let mut assistant = state.assistant.write().await;
            assistant.managed_model = Some(model);
            if assistant.provider == AssistantProvider::None {
                assistant.provider = AssistantProvider::Managed;
            }
        }
        let _ = persist_studio_settings(&state).await;
        let runtime = state.assistant_runtime.clone();
        // The whole thing - runtime, CUDA libraries, model - as one download.
        // Starting them one after another only looked like a queue: each call
        // returned before its file had arrived, so the next one was refused and
        // the model, always last, was never fetched at all.
        let ids: Vec<String> = set
            .into_iter()
            .map(str::to_string)
            .chain(request.model_id.clone())
            .collect();
        tokio::spawn(async move {
            if let Err(error) = runtime.install_all(&ids).await {
                eprintln!("the assistant could not be installed: {error}");
            }
        });
        return Ok(Json(state.assistant_runtime.status().await));
    }
    state
        .assistant_runtime
        .install(&request.asset_id)
        .await
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(state.assistant_runtime.status().await))
}

/// How much room the local model gets.
///
/// The prompt alone is around 3200 tokens - the caption contract plus the three
/// official examples the writing guide selects - and a full answer is another
/// 700 to 1200. Eight thousand left almost no headroom for a long lyric, and a
/// model that runs out mid-JSON produces an answer nothing can parse.
const ASSISTANT_CONTEXT: u32 = 16384;

async fn assistant_runtime_start(
    State(state): State<AppState>,
    Json(request): Json<AssistantModelRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let own_file = request.model_path.clone().unwrap_or_default();
    let reasoning = state.assistant.read().await.reasoning_effort.clone();
    let base_url = if own_file.trim().is_empty() {
        state.assistant_runtime.start(&request.model_id, ASSISTANT_CONTEXT, reasoning.as_deref()).await
    } else {
        state.assistant_runtime.start_path(std::path::Path::new(own_file.trim()), ASSISTANT_CONTEXT, reasoning.as_deref()).await
    }
    .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))?;
    Ok(Json(serde_json::json!({ "base_url": base_url, "model_id": request.model_id, "model_path": own_file })))
}

async fn assistant_runtime_stop(State(state): State<AppState>) -> Json<Value> {
    state.assistant_runtime.stop().await;
    Json(serde_json::json!({ "running": false }))
}

#[derive(Debug, Deserialize)]
struct KaraokeRequest {
    /// Overrides the language guess for this one track.
    #[serde(default)]
    language: Option<String>,
}

/// What the chosen engine would install: its files, their weight, and how much
/// of it is already here.
fn set_progress(downloader: &crate::downloads::Downloader, set: &[&'static lyrics_sync::Asset]) -> Value {
    let total: u64 = set.iter().map(|asset| asset.bytes).sum();
    let installed: u64 = set.iter().filter(|asset| downloader.is_installed(asset)).map(|asset| asset.bytes).sum();
    serde_json::json!({
        "bytes": total,
        "installed_bytes": installed,
        "ready": !set.is_empty() && installed == total,
        "files": set.len(),
    })
}

async fn karaoke_status(State(state): State<AppState>) -> Json<Value> {
    let config = state.lyrics_sync_config.read().await.clone();
    let status = state.lyrics_sync.status(&config).await;
    let name = match config.provider {
        lyrics_sync::AsrProvider::Whisper => "whisper",
        _ => "parakeet",
    };
    let set = karaoke_set(name, config.runtime, config.whisper_model.as_deref());
    let mut value = serde_json::to_value(&status).unwrap_or(Value::Null);
    if let Value::Object(ref mut fields) = value {
        fields.insert("set".into(), set_progress(state.lyrics_sync.downloader(), &set));
    }
    Json(value)
}

/// Every field optional, so a panel that changes one thing changes one thing.
///
/// This took the whole configuration before: the download page, which knows
/// only which recogniser and which device were picked, would have blanked the
/// switch and both model choices by sending them absent.
#[derive(Debug, Deserialize)]
struct KaraokeSettingsRequest {
    enabled: Option<bool>,
    provider: Option<lyrics_sync::AsrProvider>,
    whisper_model: Option<Option<String>>,
    openrouter_model: Option<Option<String>>,
    runtime: Option<lyrics_sync::OnnxFlavour>,
}

async fn update_karaoke_settings(
    State(state): State<AppState>,
    Json(request): Json<KaraokeSettingsRequest>,
) -> Result<Json<lyrics_sync::SyncStatus>, (StatusCode, Json<ApiError>)> {
    let merged = {
        let mut config = state.lyrics_sync_config.write().await;
        if let Some(enabled) = request.enabled { config.enabled = enabled; }
        if let Some(provider) = request.provider { config.provider = provider; }
        if let Some(model) = request.whisper_model { config.whisper_model = model; }
        if let Some(model) = request.openrouter_model { config.openrouter_model = model; }
        if let Some(runtime) = request.runtime { config.runtime = runtime; }
        config.clone()
    };
    let _ = persist_studio_settings(&state).await;
    Ok(Json(state.lyrics_sync.status(&merged).await))
}

/// Frees the disk a karaoke recogniser takes.
/// Removes a recogniser the same way it was installed: whole.
async fn karaoke_remove(
    State(state): State<AppState>,
    Json(request): Json<AssistantAssetRequest>,
) -> Result<Json<lyrics_sync::SyncStatus>, (StatusCode, Json<ApiError>)> {
    let config = state.lyrics_sync_config.read().await.clone();
    let set = karaoke_set(&request.asset_id, config.runtime, config.whisper_model.as_deref());
    if !set.is_empty() {
        for asset in set {
            let _ = state.lyrics_sync.downloader().remove(asset);
        }
        return Ok(Json(state.lyrics_sync.status(&config).await));
    }
    let asset = lyrics_sync::asset(&request.asset_id)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, format!("unknown karaoke asset: {}", request.asset_id)))?;
    state
        .lyrics_sync
        .downloader()
        .remove(asset)
        .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(state.lyrics_sync.status(&config).await))
}

/// Frees the disk the stem separation model takes.
async fn remove_separation_model(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let freed = state
        .separator
        .downloader()
        .remove(&separation::MODEL)
        .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(serde_json::json!({ "freed_bytes": freed })))
}

/// Installs a recogniser, not a file.
///
/// Parakeet is six downloads and Whisper is two, and which two depends on the
/// card. Asking a person to work that out from a list of file names - and to
/// notice that one of them is a runtime - is not a setup screen, it is a quiz.
/// The whole set is named here, in the order it is used.
fn karaoke_set(name: &str, device: lyrics_sync::OnnxFlavour, whisper_model: Option<&str>) -> Vec<&'static lyrics_sync::Asset> {
    let mut wanted: Vec<String> = Vec::new();
    match name {
        "parakeet" => {
            wanted.push("onnxruntime".into());
            if !matches!(device, lyrics_sync::OnnxFlavour::Cpu) {
                wanted.extend(CARD_ASSETS.map(String::from));
            }
            // The precision is chosen the same way a Whisper model is: through
            // the dropdown, which names one of the two encoders.
            if whisper_model.is_some_and(|id| id.contains("fp32")) {
                wanted.extend(lyrics_sync::PARAKEET_FP32_ASSET_IDS.map(String::from));
            } else {
                wanted.extend(lyrics_sync::PARAKEET_ASSET_IDS.map(String::from));
            }
        }
        "whisper" => {
            // One binary whichever device is chosen; the card needs CUDA 11's
            // libraries beside it, and without them CTranslate2 silently uses
            // the processor instead of saying so.
            wanted.push("whisper-engine".into());
            if !matches!(device, lyrics_sync::OnnxFlavour::Cpu) {
                wanted.push("whisper-cublas".into());
                wanted.push("whisper-cudnn".into());
            }
            // A model is a directory of files, and it is useless one file
            // short, so the whole set goes together.
            let chosen = whisper_model.unwrap_or("whisper-large-v3-turbo");
            if let Some(size) = chosen.strip_prefix("whisper-") {
                let prefix = format!("models/whisper/faster-whisper-{size}/");
                wanted.extend(
                    lyrics_sync::ASSETS
                        .iter()
                        .filter(|asset| asset.relative_path.starts_with(&prefix))
                        .map(|asset| asset.id.to_string()),
                );
            }
        }
        _ => {}
    }
    wanted.iter().filter_map(|id| lyrics_sync::asset(id)).collect()
}

async fn karaoke_install(
    State(state): State<AppState>,
    Json(request): Json<AssistantAssetRequest>,
) -> Result<Json<lyrics_sync::SyncStatus>, (StatusCode, Json<ApiError>)> {
    let config = state.lyrics_sync_config.read().await.clone();
    let set = karaoke_set(
        &request.asset_id,
        config.runtime,
        request.model_id.as_deref().or(config.whisper_model.as_deref()),
    );
    if !set.is_empty() {
        // In the background, so the panel keeps answering while half a
        // gigabyte arrives; the whole set is one button, not eight.
        let sync = state.lyrics_sync.clone();
        tokio::spawn(async move {
            if let Err(error) = sync.downloader().install_all("karaoke", &set).await {
                eprintln!("the karaoke recogniser could not be installed: {error}");
            }
        });
        return Ok(Json(state.lyrics_sync.status(&config).await));
    }

    let asset = lyrics_sync::asset(&request.asset_id)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, format!("unknown karaoke asset: {}", request.asset_id)))?;
    state
        .lyrics_sync
        .downloader()
        .install(asset)
        .await
        .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(state.lyrics_sync.status(&config).await))
}

/// Times one track's own lyrics and stores the result with it.
///
/// Recognition is CPU or GPU bound and takes tens of seconds, so it runs on a
/// blocking thread rather than holding an async worker hostage.
async fn create_song_karaoke(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<KaraokeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let config = state.lyrics_sync_config.read().await.clone();
    if matches!(config.provider, lyrics_sync::AsrProvider::Whisper | lyrics_sync::AsrProvider::Parakeet) {
        card_free_of_training(&state, "make karaoke").await?;
    }
    if !config.available() {
        return Err(api_error(StatusCode::CONFLICT, "karaoke.off".into()));
    }
    // Pressing the button on a track is the instruction to time it. If the
    // chosen local recogniser is not on disk yet, that is a download to start,
    // not a refusal to hand back.
    if !ensure_local_recogniser(&state, &config, &id).await {
        return Err(api_error(StatusCode::CONFLICT, "karaoke.model-missing".into()));
    }
    let song = state
        .library
        .get_song(&id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "no such song".into()))?;
    let audio = state
        .library
        .media_path_for_song(&song)
        .map(|path| path.to_string_lossy().into_owned())
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "this track has no audio to listen to".into()))?;
    if !auto_title::has_sung_lines(&song.lyrics) {
        return Err(api_error(StatusCode::BAD_REQUEST, "karaoke.instrumental".into()));
    }

    let words = match config.provider {
        lyrics_sync::AsrProvider::None => {
            return Err(api_error(StatusCode::CONFLICT, "karaoke.no-recogniser".into()))
        }
        lyrics_sync::AsrProvider::Parakeet => {
            let sync = state.lyrics_sync.clone();
            let path = std::path::PathBuf::from(&audio);
            tokio::task::spawn_blocking(move || sync.parakeet_words(&path))
                .await
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        }
        lyrics_sync::AsrProvider::Whisper => {
            let sync = state.lyrics_sync.clone();
            let config = config.clone();
            let path = std::path::PathBuf::from(&audio);
            let language = request.language.clone();
            let lyrics = song.lyrics.clone();
            tokio::task::spawn_blocking(move || sync.whisper_words(&config, &path, language.as_deref(), &lyrics))
                .await
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        }
        lyrics_sync::AsrProvider::OpenRouter => {
            karaoke_words_from_openrouter(&state, &config, &audio, request.language.as_deref()).await
        }
    }
    .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))?;

    // Word by word, because that is what karaoke means: a line time alone
    // leaves a player sweeping the highlight linearly through the line, which
    // drifts off the singing immediately.
    let lines = lyrics_sync::align_lyrics_words(&words, &song.lyrics);
    if lines.is_empty() {
        return Err(api_error(StatusCode::BAD_GATEWAY, "karaoke.no-match".into()));
    }
    let lrc = lyrics_sync::enhanced_lrc(&lines);
    state
        .library
        .set_song_lrc(&id, &lrc)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({ "lrc": lrc, "lines": lines.len(), "provider": config.provider })))
}

async fn delete_song_karaoke(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    state
        .library
        .set_song_lrc(&id, "")
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({ "lrc": Value::Null })))
}

/// The cloud path: base64 the audio, ask for verbose output, read the times.
async fn karaoke_words_from_openrouter(
    state: &AppState,
    config: &lyrics_sync::LyricsSyncConfig,
    audio: &str,
    language: Option<&str>,
) -> anyhow::Result<Vec<(f64, String)>> {
    use base64::Engine as _;
    let catalog = catalog_for(state).await.map_err(|error| anyhow::anyhow!(error))?;
    let model = config
        .openrouter_model
        .clone()
        .filter(|value| !value.trim().is_empty())
        // Only the Whisper family returns timings, and that is what karaoke is.
        .or_else(|| providers::openrouter::suggested_model(&catalog, Capability::SpeechToText))
        .unwrap_or_default();
    let bytes = std::fs::read(audio)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let format = std::path::Path::new(audio)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp3")
        .to_ascii_lowercase();
    let request = providers::openrouter::stt_request_for(
        &catalog,
        &model,
        providers::openrouter::Base64AudioInput { timestamps: true, data: &encoded, format: &format, language },
    )?;
    let response = execute_openrouter_json(request).await?;
    let segments = lyrics_sync::segments_from_verbose_json(&response.body);
    if segments.is_empty() {
        anyhow::bail!("{model} answered without timings; pick a model that returns them");
    }
    Ok(segments)
}

/// Writes lyrics and/or the structured caption. Optional by design: with no
/// provider configured this answers 409 and the manual form is unaffected.

/// The same request as `assistant_write`, reported while it happens.
///
/// A model can take a minute, and a button that only spins says nothing about
/// whether the request even left the machine. This sends the stages as they
/// occur - the request going out, the first token coming back - and then the
/// text itself, piece by piece, so the fields fill in front of the user.

/// The OpenRouter model the writing assistant should use.
///
/// Two screens name this: the provider page, where every capability picks its
/// model, and the assistant page, which has a field of its own. They disagreed,
/// and the request went to whichever the code happened to read - so the panel
/// showed one model while another answered. The provider selection wins,
/// because that page is where every other capability is chosen.
async fn assistant_openrouter_model(state: &AppState, config: &AssistantConfig) -> String {
    let selected = state
        .configuration
        .read()
        .await
        .selections
        .iter()
        .find(|selection| selection.capability == Capability::PromptEnhancement)
        .and_then(|selection| selection.cloud_model.clone())
        .filter(|model| !model.trim().is_empty());
    if let Some(model) = selected {
        return model;
    }
    if let Some(model) = config.openrouter_model.clone().filter(|model| !model.trim().is_empty()) {
        return model;
    }
    catalog_for(state)
        .await
        .ok()
        .and_then(|catalog| providers::openrouter::suggested_model(&catalog, Capability::PromptEnhancement))
        .unwrap_or_default()
}

async fn assistant_write_stream(
    State(state): State<AppState>,
    Json(request): Json<assistant::AssistRequest>,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let config = state.assistant.read().await.clone();
    if matches!(config.provider, AssistantProvider::Managed) {
        card_free_of_training(&state, "ask the assistant").await?;
    }
    if !config.available() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "No writing assistant is configured. The manual form does not need one.".into(),
        ));
    }
    let (system, required) = assistant::instructions(&request);
    let user = assistant::user_message(&request);
    let task = request.target;
    let target = assist_target_name(task);

    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(64);
    let emit = |sender: tokio::sync::mpsc::Sender<Result<axum::body::Bytes, std::io::Error>>, event: Value| async move {
        let line = format!("data: {event}\n\n");
        let _ = sender.send(Ok(axum::body::Bytes::from(line))).await;
    };

    tokio::spawn(async move {
        emit(sender.clone(), serde_json::json!({ "stage": "preparing" })).await;

        // The connected agent answers whole: there is nothing to stream.
        if config.provider == AssistantProvider::Agent {
            emit(sender.clone(), serde_json::json!({ "stage": "sent", "model": "agent" })).await;
            match mcp::ask_agent(&system, &user, Some(assistant::draft_schema(&required)), &target).await {
                Ok(text) => {
                    emit(sender.clone(), serde_json::json!({ "stage": "writing" })).await;
                    emit(sender.clone(), serde_json::json!({ "delta": text })).await;
                    match assistant::parse_draft(&text, &required) {
                        Ok(draft) => emit(sender.clone(), serde_json::json!({ "stage": "done", "text": text, "draft": draft })).await,
                        Err(error) => emit(sender.clone(), serde_json::json!({ "error": format!("the agent's answer does not fit the schema: {error}") })).await,
                    }
                }
                Err(error) => emit(sender.clone(), serde_json::json!({ "error": error })).await,
            }
            return;
        }

        // Where the request goes, and with which model.
        let (base, model, key): (String, String, Option<String>) = match config.provider {
            AssistantProvider::OpenRouter => {
                let model = assistant_openrouter_model(&state, &config).await;
                let key = match credentials::openrouter_api_key().map(|(key, _)| key) {
                    Some(key) => key,
                    None => {
                        emit(sender.clone(), serde_json::json!({ "error": "no OpenRouter key is stored" })).await;
                        return;
                    }
                };
                ("https://openrouter.ai/api/v1".to_string(), model, Some(key))
            }
            AssistantProvider::Managed => {
                let own_file = config.managed_path.clone().unwrap_or_default();
                let id = config.managed_model.clone().unwrap_or_default();
                let reasoning = config.reasoning_effort.clone();
                let started = if own_file.trim().is_empty() {
                    state.assistant_runtime.start(&id, ASSISTANT_CONTEXT, reasoning.as_deref()).await
                } else {
                    state.assistant_runtime.start_path(std::path::Path::new(own_file.trim()), ASSISTANT_CONTEXT, reasoning.as_deref()).await
                };
                match started {
                    Ok(base) => (base, if own_file.trim().is_empty() { id } else { "local-model".to_string() }, None),
                    Err(error) => {
                        emit(sender.clone(), serde_json::json!({ "error": format!("the local assistant did not start: {error}") })).await;
                        return;
                    }
                }
            }
            _ => (
                config.local_base_url.clone().unwrap_or_default(),
                config.local_model.clone().unwrap_or_default(),
                None,
            ),
        };

        // A local llama-server enforces the shape while it samples, so the
        // answer cannot come back as prose or as a list where a string belongs.
        let schema = matches!(config.provider, AssistantProvider::Managed | AssistantProvider::Local)
            .then(|| assistant::draft_schema(&required));
        // What the model publishes for itself, exactly as the non-streaming
        // path uses it. Passing nothing here meant every streamed request went
        // out with the studio's own temperature on top of models that had
        // stated their own - a different request from the one the catalogue
        // describes, and the streamed path is the one the window uses.
        let entry = if matches!(config.provider, AssistantProvider::OpenRouter) {
            catalog_describing(&state, &model)
                .await
                .ok()
                .and_then(|catalog| catalog.models.iter().find(|item| item.id == model).cloned())
        } else {
            None
        };
        let published = entry.as_ref().map(|entry| serde_json::to_value(&entry.defaults).unwrap_or(Value::Null));
        // Thinking on the model's own terms: it publishes which efforts it
        // takes, which one it prefers, and whether it can be asked not to
        // think at all. A setting of ours that is not on its list becomes the
        // one it named, because naming an unknown effort is refused outright.
        let effort = match (&entry, config.provider) {
            (Some(entry), AssistantProvider::OpenRouter) => entry
                .reasoning
                .as_ref()
                .and_then(|reasoning| reasoning.effort_for(config.reasoning_effort.as_deref())),
            (_, AssistantProvider::OpenRouter) => None,
            _ => config.reasoning_effort.clone(),
        };
        let fit = if matches!(config.provider, AssistantProvider::Managed | AssistantProvider::Local) {
            assistant::fit_to_local_task
        } else {
            assistant::fit_to_task
        };
        let mut body = fit(
            assistant::chat_body_constrained(&model, &system, &user, effort.as_deref(), published.as_ref(), schema),
            task,
        );
        body["stream"] = Value::Bool(true);

        emit(sender.clone(), serde_json::json!({ "stage": "sent", "model": model })).await;
        request_log::asked("assistant", &model, system.chars().count() + user.chars().count());
        let started = std::time::Instant::now();

        let client = reqwest::Client::new();
        let mut outgoing = client
            .post(format!("{}/chat/completions", base.trim_end_matches('/')))
            .json(&body)
            .timeout(std::time::Duration::from_secs(600));
        if let Some(key) = key {
            outgoing = outgoing
                .header(reqwest::header::AUTHORIZATION, format!("Bearer {key}"))
                .header("HTTP-Referer", music_core::studio().repo_url())
                .header("X-Title", &music_core::studio().name);
        }

        // Until the first byte the window can be closed as well: a model
        // still loading or reading a long prompt sends nothing for minutes.
        let sent = tokio::select! {
            _ = sender.closed() => {
                request_log::failed("assistant", &model, "stopped by the user");
                release_assistant_unless_kept(&state).await;
                return;
            }
            sent = outgoing.send() => sent,
        };
        let response = match sent {
            Ok(response) => response,
            Err(error) => {
                request_log::failed("assistant", &model, &error.to_string());
                emit(sender.clone(), serde_json::json!({ "error": format!("the assistant is unreachable: {error}") })).await;
                return;
            }
        };
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            request_log::answered("assistant", &model, status.as_u16(), started.elapsed().as_secs_f64(), text.chars().count());
            request_log::unusable("assistant", &model, &format!("http {status}"), &text);
            emit(sender.clone(), serde_json::json!({ "error": format!("the assistant returned {status}: {text}") })).await;
            return;
        }

        // Server-sent events, one JSON object per `data:` line, with the text in
        // `choices[0].delta.content`.
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut first = true;
        let mut whole = String::new();
        let mut cut_short = false;
        let mut window_gone = false;
        loop {
            // The window stopped the run or was closed. Dropping the stream
            // closes the connection, which is what makes the provider stop
            // generating; reading on keeps the model busy for nobody.
            let chunk = tokio::select! {
                _ = sender.closed() => {
                    window_gone = true;
                    break;
                }
                chunk = stream.next() => chunk,
            };
            let Some(Ok(chunk)) = chunk else { break };
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer.drain(..line_end + 1);
                let Some(payload) = line.strip_prefix("data:") else { continue };
                let payload = payload.trim();
                if payload == "[DONE]" {
                    continue;
                }
                let Ok(event): Result<Value, _> = serde_json::from_str(payload) else { continue };
                let choice = event.get("choices").and_then(|choices| choices.get(0));
                if choice.and_then(|choice| choice.get("finish_reason")).and_then(Value::as_str) == Some("length") {
                    cut_short = true;
                }
                let delta = choice
                    .and_then(|choice| choice.get("delta"))
                    .and_then(|delta| delta.get("content"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if delta.is_empty() {
                    continue;
                }
                if first {
                    first = false;
                    // The moment the model started answering. Without it a log
                    // of a run that never came back cannot say whether it was
                    // thinking or simply gone.
                    request_log::answered("assistant first token", &model, 200, started.elapsed().as_secs_f64(), 0);
                    emit(sender.clone(), serde_json::json!({ "stage": "writing" })).await;
                }
                whole.push_str(delta);
                emit(sender.clone(), serde_json::json!({ "delta": delta })).await;
            }
        }
        drop(stream);

        request_log::answered("assistant", &model, 200, started.elapsed().as_secs_f64(), whole.chars().count());
        if window_gone {
            request_log::failed("assistant", &model, "stopped by the user");
            release_assistant_unless_kept(&state).await;
            return;
        }
        if cut_short {
            request_log::unusable("assistant", &model, "the answer reached the length limit", &whole);
            emit(
                sender.clone(),
                serde_json::json!({ "error": format!(
                    "the model wrote {} tokens without finishing the answer and was stopped - try again or pick another model",
                    assistant::max_tokens_for(task)
                ) }),
            )
            .await;
            release_assistant_unless_kept(&state).await;
            return;
        }
        // The answer is kept whenever it cannot be turned into a draft. That is
        // the case this log exists for: the window shows one red line, and
        // without this the text behind it is gone the moment it is closed.
        // The draft the window shows is the one it gets: the finished fields
        // come at the end of this stream, not from asking the model again.
        match assistant::parse_draft(&whole, &required) {
            Ok(draft) => emit(sender.clone(), serde_json::json!({ "stage": "done", "text": whole, "draft": draft })).await,
            Err(error) => {
                request_log::unusable("assistant", &model, &error.to_string(), &whole);
                emit(sender.clone(), serde_json::json!({ "error": error.to_string() })).await;
            }
        }
        // The card belongs to whatever runs next unless the user asked for
        // everything to stay resident.
        release_assistant_unless_kept(&state).await;
    });

    // A channel of chunks becomes the response body; the receiver is turned into
    // a stream by hand to avoid another dependency for four lines.
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });
    let body = Body::from_stream(stream);
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .expect("valid stream response"))
}

async fn assistant_write(
    State(state): State<AppState>,
    Json(request): Json<assistant::AssistRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let draft = assistant_draft(&state, &request).await;
    release_assistant_unless_kept(&state).await;
    draft.map(Json)
}

#[derive(Deserialize)]
struct LyricsSectionsRequest {
    lyrics: String,
}

/// Lyrics laid out in tagged sections with their words untouched, the layout
/// a dataset's published lyric sheets get. Tags already there are replaced.
async fn assistant_sections(
    State(state): State<AppState>,
    Json(request): Json<LyricsSectionsRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let lines = assistant::without_section_tags(&request.lyrics);
    if lines.trim().is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "there are no lyrics to lay out".into()));
    }
    let laid_out = prepare::lay_out_lyrics(&state, &lines, assistant::AssistTarget::Sheet).await;
    release_assistant_unless_kept(&state).await;
    laid_out.map(|lyrics| Json(serde_json::json!({ "lyrics": lyrics }))).map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))
}

/// A draft from the assistant, which stays loaded after it: a run of songs
/// asks it many times and releases it once at the end.
async fn assistant_draft(state: &AppState, request: &assistant::AssistRequest) -> Result<Value, (StatusCode, Json<ApiError>)> {
    let (system, required) = assistant::instructions(request);
    let user = assistant::user_message(request);
    let content = assistant_ask(state, &system, &user, Some(assistant::draft_schema(&required)), request.target).await?;
    let draft = assistant::parse_draft(&content, required).map_err(|error| {
        // The answer, kept: this is the difference between "invalid JSON" and
        // seeing that the model wrote an apology instead of a song.
        request_log::unusable("assistant", "", &error.to_string(), &content);
        api_error(StatusCode::BAD_GATEWAY, error.to_string())
    })?;
    Ok(serde_json::to_value(draft).unwrap_or(Value::Null))
}

/// What the assistant is asked for, by the name the API uses.
fn assist_target_name(target: assistant::AssistTarget) -> String {
    serde_json::to_value(target).ok().and_then(|value| value.as_str().map(str::to_string)).unwrap_or_default()
}

/// One question to the writing assistant and its answer as text; a schema,
/// when given, holds a local model to that shape of JSON.
async fn assistant_ask(
    state: &AppState,
    system: &str,
    user: &str,
    schema: Option<Value>,
    target: assistant::AssistTarget,
) -> Result<String, (StatusCode, Json<ApiError>)> {
    let state = state.clone();
    let config = state.assistant.read().await.clone();
    if matches!(config.provider, AssistantProvider::Managed) {
        card_free_of_training(&state, "ask the assistant").await?;
    }
    if !config.available() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "No writing assistant is configured. The manual form does not need one.".into(),
        ));
    }
    if config.provider == AssistantProvider::Agent {
        return mcp::ask_agent(system, user, schema, &assist_target_name(target)).await.map_err(|error| api_error(StatusCode::BAD_GATEWAY, error));
    }

    let response: Value = match config.provider {
        AssistantProvider::None | AssistantProvider::Local | AssistantProvider::Managed => {
            // A managed model is started on first use and then stays loaded, so
            // the second request does not pay for the load again.
            let (base, model) = match config.provider {
                AssistantProvider::Managed => {
                    let own_file = config.managed_path.clone().unwrap_or_default();
                    let id = config.managed_model.clone().unwrap_or_default();
                    let reasoning = config.reasoning_effort.as_deref();
                    let base = if own_file.trim().is_empty() {
                        state.assistant_runtime.start(&id, 8192, reasoning).await
                    } else {
                        state.assistant_runtime.start_path(std::path::Path::new(own_file.trim()), 8192, reasoning).await
                    }
                    .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))?;
                    (base, if own_file.trim().is_empty() { id } else { own_file })
                }
                _ => (
                    config.local_base_url.clone().unwrap_or_default(),
                    config.local_model.clone().unwrap_or_default(),
                ),
            };
            let sent = reqwest::Client::new()
                .post(format!("{}/chat/completions", base.trim_end_matches('/')))
                .json(&assistant::fit_to_local_task(
                    assistant::chat_body_constrained(
                        &model,
                        &system,
                        &user,
                        None,
                        None,
                        schema.filter(|_| matches!(config.provider, AssistantProvider::Managed | AssistantProvider::Local)),
                    ),
                    target,
                ))
                .timeout(std::time::Duration::from_secs(180))
                .send()
                .await
                .map_err(|error| {
                    // A sidecar that died mid-request leaves nothing but a
                    // refused connection unless its own log is quoted back.
                    let tail = state.assistant_runtime.log_tail();
                    let detail = if tail.is_empty() { String::new() } else { format!("
{tail}") };
                    api_error(StatusCode::BAD_GATEWAY, format!("the local assistant is unreachable: {error}{detail}"))
                })?;
            let status = sent.status();
            let body = sent.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(api_error(StatusCode::BAD_GATEWAY, format!("the local assistant returned {status}: {body}")));
            }
            serde_json::from_str(&body)
                .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("invalid assistant response: {error}")))?
        }
        AssistantProvider::OpenRouter => {
            let catalog_now = catalog_for(&state).await.ok();
            let model = assistant_openrouter_model(&state, &config).await;
            let catalog = catalog_for(&state)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error))?;
            catalog
                .selected(Capability::PromptEnhancement, &model)
                .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
            let authenticated = providers::openrouter::authenticated_request_for(providers::openrouter::OpenRouterRequest {
                method: providers::openrouter::HttpMethod::Post,
                path: providers::openrouter::CHAT_COMPLETIONS_PATH,
                // Whatever this model publishes for itself; the studio's own
                // temperature is only for models that publish nothing.
                body: {
                    let entry = catalog_now.as_ref().and_then(|catalog| catalog.models.iter().find(|entry| entry.id == model));
                    let effort = entry
                        .and_then(|entry| entry.reasoning.as_ref())
                        .and_then(|reasoning| reasoning.effort_for(config.reasoning_effort.as_deref()));
                    assistant::fit_to_task(
                        assistant::chat_body_full(
                            &model,
                            &system,
                            &user,
                            effort.as_deref(),
                            entry.map(|entry| serde_json::to_value(&entry.defaults).unwrap_or(Value::Null)).as_ref(),
                        ),
                        target,
                    )
                },
            })
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
            execute_openrouter_json(authenticated.request)
                .await
                .map_err(|error| api_error(StatusCode::BAD_GATEWAY, format!("OpenRouter assistant failed: {error}")))?
                .body
        }
        AssistantProvider::None => unreachable!("availability was checked above"),
        AssistantProvider::Agent => unreachable!("the agent answered above"),
    };

    assistant::content_of(&response).map_err(|error| {
        request_log::unusable("assistant", "", &error.to_string(), &response.to_string());
        api_error(StatusCode::BAD_GATEWAY, error.to_string())
    })
}

/// Frees the assistant's five gigabytes as soon as it has answered.
///
/// "Keep models in VRAM between jobs" is off by default, and it means what it
/// says: nothing stays loaded. The assistant was the exception nobody chose -
/// it wrote a caption, kept the card, and the engine then died trying to load its
/// own weights. With the setting on, it stays, because that is what the
/// setting is for. Either way the next request starts it again.
async fn release_assistant_unless_kept(state: &AppState) {
    if state.engine_options.read().await.keep_loaded {
        return;
    }
    if state.assistant_runtime.base_url().await.is_some() {
        state.assistant_runtime.stop().await;
    }
}

/// Where a library song's MIDI lives: beside its audio, named after it.
fn midi_path(state: &AppState, song_id: &str) -> PathBuf {
    state.library.media_dir().join(format!("{song_id}.mid"))
}

/// What a MIDI file says about itself: its model, its instruments and notes.
fn midi_sidecar(midi: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}.json", midi.display()))
}

fn midi_size(asked: Option<&str>) -> Result<&'static midi::Size, (StatusCode, Json<ApiError>)> {
    let id = asked.map(str::trim).filter(|id| !id.is_empty()).unwrap_or(midi::DEFAULT_SIZE);
    midi::size(id).ok_or_else(|| api_error(StatusCode::BAD_REQUEST, format!("no model size {id}; the sizes are small, medium and large")))
}

/// The transcriber, its model sizes, the download and the run.
async fn midi_status(State(state): State<AppState>) -> Json<Value> {
    let sizes: Vec<Value> = midi::SIZES
        .iter()
        .map(|size| serde_json::json!({ "id": size.id, "params": size.params, "bytes": size.bytes, "installed": state.midi.model_installed(size), "missing_bytes": state.midi.missing_bytes(size) }))
        .collect();
    let run = state.midi_run.read().await.clone().map(|run| {
        let progress = run.progress();
        let mut value = serde_json::to_value(run).unwrap_or(Value::Null);
        value["progress"] = progress.into();
        value
    });
    Json(serde_json::json!({
        "tool_installed": state.midi.tool_installed(),
        "sizes": sizes,
        "default_size": midi::DEFAULT_SIZE,
        "download": state.midi.downloader().active().await,
        "run": run,
        "license": "MuScriptor by Kyutai & Mirelo (arXiv:2607.08168): code MIT, weights CC BY-NC 4.0 - non-commercial use. Native port: HOT-Step-CPP ace-midi.",
    }))
}

/// The notes the run at work has heard so far, for the live piano roll.
async fn midi_live_notes(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!({ "notes": state.midi_notes.read().await.clone() }))
}

#[derive(Debug, Deserialize)]
struct MidiSizeRequest {
    #[serde(default)]
    size: Option<String>,
}

/// Downloads the transcriber and a model size ahead of the first use.
async fn install_midi(State(state): State<AppState>, Json(input): Json<MidiSizeRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let size = midi_size(input.size.as_deref())?;
    let missing = state.midi.missing(size);
    if missing.is_empty() {
        return Ok(Json(serde_json::json!({ "installed": true, "size": size.id })));
    }
    let transcriber = state.midi.clone();
    tokio::spawn(async move {
        if let Err(error) = transcriber.downloader().install_all("midi", &missing).await {
            eprintln!("[ERROR] midi: download failed: {error:#}");
        }
    });
    Ok(Json(serde_json::json!({ "started": true, "size": size.id })))
}

async fn remove_midi_model(State(state): State<AppState>, Json(input): Json<MidiSizeRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let size = midi_size(input.size.as_deref())?;
    let freed = state.midi.remove(size).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")))?;
    Ok(Json(serde_json::json!({ "removed": size.id, "freed_bytes": freed })))
}

/// Stops the transcription at work, or the download it waits for.
async fn cancel_midi(State(state): State<AppState>) -> Json<Value> {
    state.midi_stop.0.store(true, std::sync::atomic::Ordering::Relaxed);
    state.midi_stop.1.notify_waiters();
    state.midi.downloader().cancel();
    Json(serde_json::json!({ "cancelled": true }))
}

#[derive(Debug, Deserialize)]
struct MidiRequest {
    /// A library track: its MIDI is kept beside it.
    #[serde(default)]
    song_id: Option<String>,
    /// Or any audio file on this computer: its MIDI goes to the studio's folder.
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    size: Option<String>,
}

/// Turns a track, a stem or any audio file into MIDI. What the transcriber
/// needs is downloaded first if it is not here yet.
async fn start_midi(State(state): State<AppState>, Json(input): Json<MidiRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    card_free_of_training(&state, "turn tracks into MIDI").await?;
    if state.midi_run.read().await.as_ref().is_some_and(|run| !run.done) {
        return Err(api_error(StatusCode::CONFLICT, "a track is already being turned into MIDI".into()));
    }
    let size = midi_size(input.size.as_deref())?;
    let (song_id, title, audio, output) = match (input.song_id.filter(|id| !id.trim().is_empty()), input.path.filter(|path| !path.trim().is_empty())) {
        (Some(id), _) => {
            let song = state
                .library
                .get_song(&id)
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
            let audio = state.library.media_path_for_song(&song).ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "this track has no stored audio".into()))?;
            (Some(id.clone()), song.title.clone(), audio, midi_path(&state, &id))
        }
        (None, Some(path)) => {
            let audio = PathBuf::from(path.trim());
            if !audio.is_file() {
                return Err(api_error(StatusCode::BAD_REQUEST, format!("no audio file at {}", audio.display())));
            }
            let title = audio.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let output = state.midi.loose_output(&audio);
            (None, title, audio, output)
        }
        _ => return Err(api_error(StatusCode::BAD_REQUEST, "song_id or path is required".into())),
    };
    state.midi_stop.0.store(false, std::sync::atomic::Ordering::Relaxed);
    state.midi_notes.write().await.clear();
    *state.midi_run.write().await = Some(midi::Run { song_id: song_id.clone(), title, size: size.id, stage: "preparing", chunks_done: 0, chunks_total: 0, notes: 0, done: false, error: None, file: None });
    let background = state.clone();
    tokio::spawn(async move {
        let outcome = transcribe_to_midi(&background, size, &audio, &output).await;
        if let Some(run) = background.midi_run.write().await.as_mut() {
            run.done = true;
            match outcome {
                Ok(notes) => {
                    run.stage = "done";
                    run.notes = notes;
                    run.file = Some(plain_path(&output));
                }
                Err(error) => run.error = Some(format!("{error:#}")),
            }
        }
    });
    Ok(Json(serde_json::json!({ "started": true, "song_id": song_id, "size": size.id })))
}

async fn set_midi_stage(state: &AppState, stage: &'static str) {
    if let Some(run) = state.midi_run.write().await.as_mut() {
        run.stage = stage;
    }
}

fn midi_stopped(state: &AppState) -> bool {
    state.midi_stop.0.load(std::sync::atomic::Ordering::Relaxed)
}

/// Fetches what is missing, reads the audio as the transcriber wants it and
/// runs it, following its notes as they come. Returns how many it heard.
async fn transcribe_to_midi(state: &AppState, size: &'static midi::Size, audio: &std::path::Path, output: &std::path::Path) -> anyhow::Result<usize> {
    use tokio::io::AsyncBufReadExt;
    let missing = state.midi.missing(size);
    if !missing.is_empty() {
        // the first use fetches what the tool needs, as the karaoke recogniser does
        set_midi_stage(state, "downloading").await;
        state.midi.downloader().install_all("midi", &missing).await.context("download the MIDI transcriber")?;
        if midi_stopped(state) || !state.midi.missing(size).is_empty() {
            anyhow::bail!("stopped before everything the transcriber needs had arrived");
        }
    }

    // WAV and MP3 go to the transcriber as they are: its own decoder is the
    // one its port was checked against, and the model hears the difference.
    // Anything else is decoded here to the 16 kHz mono it reads raw.
    let native = audio.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| ["wav", "mp3"].contains(&extension.to_ascii_lowercase().as_str()));
    let raw = if native {
        None
    } else {
        set_midi_stage(state, "reading").await;
        let work = state.midi.work_dir();
        std::fs::create_dir_all(&work).with_context(|| format!("create {}", work.display()))?;
        let raw = work.join(format!("{}.f32", uuid::Uuid::now_v7().simple()));
        let (from, to) = (audio.to_path_buf(), raw.clone());
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let samples = audio_pcm::decode_mono_16k(&from)?;
            let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
            std::fs::write(&to, bytes).with_context(|| format!("write {}", to.display()))
        })
        .await??;
        Some(raw)
    };

    set_midi_stage(state, "transcribing").await;
    let partial = PathBuf::from(format!("{}.part", output.display()));
    if let Some(folder) = output.parent() {
        std::fs::create_dir_all(folder).with_context(|| format!("create {}", folder.display()))?;
    }
    let tool = state.midi.tool();
    let mut command = tokio::process::Command::new(&tool);
    command
        .arg("--model")
        .arg(state.midi.model_dir(size))
        .arg(if raw.is_some() { "--transcribe-raw" } else { "--transcribe" })
        .arg(raw.as_deref().unwrap_or(audio))
        .arg("--out")
        .arg(&partial)
        .arg("--jsonl")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    // the CUDA runtime it imports lives beside the engine, as for the trainer
    let mut path = std::ffi::OsString::from(engine_bundle_root().as_os_str());
    if let Some(existing) = std::env::var_os("PATH") {
        path.push(";");
        path.push(existing);
    }
    command.env("PATH", path);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = command.spawn().with_context(|| format!("start {}", tool.display()))?;
    let stdout = child.stdout.take().context("the transcriber's output")?;
    let stderr = child.stderr.take().context("the transcriber's errors")?;
    let said = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::<String>::new()));
    let listener = {
        let said = said.clone();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut said = said.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                said.push_back(line);
                if said.len() > 20 {
                    said.pop_front();
                }
            }
        })
    };

    let mut events = midi::Events::default();
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    let outcome: anyhow::Result<()> = loop {
        if midi_stopped(state) {
            break Err(anyhow::anyhow!("stopped"));
        }
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    let chunks = events.chunks_done;
                    events.take(&line);
                    // the notes are handed on a piece at a time: every note of
                    // a piece is closed by the time the next one starts
                    if events.chunks_done != chunks || events.finished {
                        *state.midi_notes.write().await = events.notes.clone();
                        if let Some(run) = state.midi_run.write().await.as_mut() {
                            run.chunks_done = events.chunks_done;
                            run.chunks_total = events.chunks_total;
                            run.notes = events.notes.len();
                        }
                    }
                }
                Ok(None) => break Ok(()),
                Err(error) => break Err(error.into()),
            },
            _ = state.midi_stop.1.notified() => break Err(anyhow::anyhow!("stopped")),
        }
    };
    if outcome.is_err() {
        let _ = child.kill().await;
    }
    let status = child.wait().await.context("wait for the transcriber")?;
    let _ = listener.await;
    if let Some(raw) = &raw {
        let _ = std::fs::remove_file(raw);
    }
    if let Err(error) = outcome {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    if !status.success() || !events.finished {
        let _ = std::fs::remove_file(&partial);
        // Windows' "a DLL was not found": the CUDA 13 runtime it imports
        if status.code() == Some(-1073741515) {
            anyhow::bail!("the transcriber could not load the CUDA 13 libraries it needs: it runs on an NVIDIA card from the GTX 16 and RTX 20 series on with driver 580 or newer, once the music engine has started on it and fetched them");
        }
        let said = said.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().cloned().collect::<Vec<_>>().join("\n");
        anyhow::bail!("the transcriber stopped with {status}: {said}");
    }
    std::fs::rename(&partial, output).with_context(|| format!("keep {}", output.display()))?;
    let sidecar = midi::Sidecar { size: size.id.to_string(), made_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_secs().to_string()).unwrap_or_default(), instruments: events.instruments(), notes: events.notes.clone() };
    std::fs::write(midi_sidecar(output), serde_json::to_vec(&sidecar)?).with_context(|| format!("write {}", midi_sidecar(output).display()))?;
    let heard = events.notes.len();
    *state.midi_notes.write().await = events.notes;
    Ok(heard)
}

/// A library song's MIDI: where it is, the model, its instruments and notes.
async fn read_song_midi(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let file = midi_path(&state, &id);
    if !file.is_file() {
        return Err(api_error(StatusCode::NOT_FOUND, "this track has no MIDI yet".into()));
    }
    let sidecar: Option<midi::Sidecar> = std::fs::read(midi_sidecar(&file)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
    Ok(Json(serde_json::json!({
        "song_id": id,
        "file": plain_path(&file),
        "size": sidecar.as_ref().map(|sidecar| sidecar.size.clone()),
        "made_at": sidecar.as_ref().map(|sidecar| sidecar.made_at.clone()),
        "instruments": sidecar.as_ref().map(|sidecar| sidecar.instruments.clone()).unwrap_or_default(),
        "notes": sidecar.map(|sidecar| sidecar.notes).unwrap_or_default(),
    })))
}

/// The .mid itself, named after the song, to save or open elsewhere.
async fn song_midi_file(State(state): State<AppState>, Path(id): Path<String>) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    let file = midi_path(&state, &id);
    let bytes = tokio::fs::read(&file).await.map_err(|_| api_error(StatusCode::NOT_FOUND, "this track has no MIDI yet".into()))?;
    let title = state.library.get_song(&id).ok().flatten().map(|song| song.title).unwrap_or_else(|| id.clone());
    let name: String = title.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let disposition = format!("attachment; filename=\"midi.mid\"; filename*=UTF-8''{}", mcp::segment(&format!("{name}.mid")));
    Ok(axum::response::Response::builder()
        .header(header::CONTENT_TYPE, "audio/midi")
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(axum::body::Body::from(bytes))
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?)
}

async fn delete_song_midi(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let file = midi_path(&state, &id);
    if !file.is_file() {
        return Err(api_error(StatusCode::NOT_FOUND, "this track has no MIDI".into()));
    }
    for path in [file.clone(), midi_sidecar(&file)] {
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("remove {}: {error}", path.display())))?;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

/// A path as other programs take it, without the `\\?\` prefix a
/// canonical Windows path carries.
fn plain_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text)
}

/// Where a library song's files are, for an agent that reads or opens them.
async fn library_song_files(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let song = state
        .library
        .get_song(&id)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found".into()))?;
    let stems: Vec<Value> = stems_on_disk(&state, &id).into_iter().map(|stem| serde_json::json!({ "stem": stem, "path": plain_path(&stem_path(&state, &id, &stem)) })).collect();
    Ok(Json(serde_json::json!({
        "audio": state.library.media_path_for_song(&song).map(|path| plain_path(&path)),
        "cover": state.library.cover_path_for_song(&song).map(|(path, _)| plain_path(&path)),
        "stems": stems,
        "midi": Some(midi_path(&state, &id)).filter(|path| path.is_file()).map(|path| plain_path(&path)),
    })))
}

/// Where a dataset song's audio and separated vocals are.
async fn dataset_song_files(State(state): State<AppState>, Path((id, item)): Path<(String, String)>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let audio = state.training.item_audio(&id, &item).map_err(training_error)?;
    let vocals = state.training.item_vocals(&id, &item).map_err(training_error)?;
    Ok(Json(serde_json::json!({ "audio": plain_path(&audio), "vocals": vocals.is_file().then(|| plain_path(&vocals)) })))
}

#[derive(Debug, Deserialize)]
struct GuideQuery {
    #[serde(default)]
    topic: String,
}

/// How to write for the engine, from the rules the studio's assistant follows.
async fn writing_guide(Query(query): Query<GuideQuery>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let topics: Vec<Value> = assistant::GUIDE_TOPICS.iter().map(|(topic, about)| serde_json::json!({ "topic": topic, "about": about })).collect();
    match assistant::writing_guide(query.topic.trim()) {
        Some(guide) => Ok(Json(serde_json::json!({ "topic": query.topic, "guide": guide }))),
        None if query.topic.trim().is_empty() => Ok(Json(serde_json::json!({ "topics": topics }))),
        None => Err(api_error(StatusCode::BAD_REQUEST, format!("No guide '{}'; the topics are: {}", query.topic, assistant::GUIDE_TOPICS.iter().map(|(topic, _)| *topic).collect::<Vec<_>>().join(", ")))),
    }
}

#[derive(Debug, Deserialize)]
struct ExamplesQuery {
    #[serde(default)]
    brief: String,
}

/// The official example requests closest to a
/// brief, whole, to write in their shape.
async fn writing_examples(Query(query): Query<ExamplesQuery>) -> Json<Value> {
    Json(serde_json::json!({ "examples": skill::references(&query.brief) }))
}

#[derive(Debug, Deserialize)]
struct VideoName {
    name: String,
}

/// A clip the video editor rendered for an agent, kept in the studio's
/// `videos` folder under a name that does not overwrite another.
async fn store_video(Query(query): Query<VideoName>, body: axum::body::Bytes) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let folder = studio_data_root().unwrap_or_else(|| PathBuf::from(".")).join("videos");
    std::fs::create_dir_all(&folder).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("create {}: {error}", folder.display())))?;
    let name = std::path::Path::new(&query.name).file_name().and_then(|name| name.to_str()).filter(|name| !name.is_empty()).unwrap_or("clip.mp4").to_string();
    let (stem, extension) = name.rsplit_once('.').map_or((name.clone(), "mp4".to_string()), |(stem, extension)| (stem.to_string(), extension.to_string()));
    let mut path = folder.join(&name);
    let mut number = 2;
    while path.exists() {
        path = folder.join(format!("{stem}-{number}.{extension}"));
        number += 1;
    }
    tokio::fs::write(&path, &body).await.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("write {}: {error}", path.display())))?;
    Ok(Json(serde_json::json!({ "path": plain_path(&path) })))
}

#[derive(Debug, Deserialize)]
struct FindLyricsRequest {
    #[serde(default)]
    artist: String,
    title: String,
    #[serde(default)]
    seconds: f64,
}

/// A song's published lyrics from the lyric databases, for the MCP tools.
async fn find_lyrics(Json(request): Json<FindLyricsRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let sources = lyrics_db::Sources::new().map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")))?;
    let mut failed = Vec::new();
    let song = lyrics_db::Song { artist: request.artist, title: request.title, seconds: request.seconds };
    let found = match sources.find(&song, &mut failed).await {
        Some(lyrics_db::Found::Lyrics { plain, timed, source }) => serde_json::json!({ "source": source, "lyrics": plain, "timed": timed }),
        Some(lyrics_db::Found::Instrumental { source }) => serde_json::json!({ "source": source, "instrumental": true }),
        None => serde_json::json!({ "found": false }),
    };
    Ok(Json(serde_json::json!({ "result": found, "unreachable": failed })))
}

/// What was asked of the cloud and what came back, newest last.
///
/// A failed draft used to leave one red line and nothing behind it; this is
/// where the answer itself is kept, so a model that wrote almost the right
/// thing can be told from one that wrote nothing.
async fn openrouter_logs() -> Json<Value> {
    Json(serde_json::json!({ "path": request_log::path().display().to_string(), "lines": request_log::tail(400) }))
}

async fn openrouter_settings() -> Json<Value> {
    let source = credentials::openrouter_source();
    Json(serde_json::json!({
        "configured": source.is_some(),
        "source": source,
        "environment_variable": credentials::OPENROUTER_ENV_VAR,
    }))
}

async fn update_openrouter_settings(
    State(state): State<AppState>,
    Json(request): Json<OpenRouterSettingsRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let source = credentials::store_openrouter_api_key(request.api_key.as_deref())
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;

    // Connecting a key is the moment to learn what it can reach. After this
    // the catalog is served from the cache on disk until the user asks for a
    // refresh, so the studio never goes to the network on its own again.
    let mut refreshed = false;
    if source.is_some() {
        {
            let mut cached = state.openrouter_catalog.write().await;
            cached.catalog = None;
        }
        refreshed = catalog_for(&state).await.is_ok();
    }

    Ok(Json(serde_json::json!({
        "configured": source.is_some(),
        "source": source,
        "environment_variable": credentials::OPENROUTER_ENV_VAR,
        "catalog_refreshed": refreshed,
    })))
}

async fn setup_status(State(state): State<AppState>) -> Json<Value> {
    let target = effective_install_target(&state).await;
    let manager_status = state.model_manager.status(target).await;
    Json(compose_setup_status(&state, manager_status).await)
}

/// Frees the disk a set of components takes.
///
/// The studio downloads ten gigabytes on request; it must be able to give them
/// back on request too, without sending anyone to hunt through a profile folder.
async fn setup_remove(
    State(state): State<AppState>,
    Json(request): Json<SetupDownloadRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let report = state
        .model_manager
        .remove(&request.ids)
        .await
        .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    Ok(Json(serde_json::json!({
        "removed": report.removed,
        "freed_bytes": report.freed_bytes,
    })))
}

const ADOPT_FOLDER_DEPTH: usize = 8;

/// Files in a folder and its subfolders, `depth` levels down; hidden folders
/// (caches, `.git`) are left out.
fn adoptable_files(folder: &std::path::Path, depth: usize) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder) else { return files };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.is_file() {
            files.push(path);
        } else if depth > 0 && path.is_dir() && !path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with('.')) {
            files.extend(adoptable_files(&path, depth - 1));
        }
    }
    files
}

/// Takes models the user already has instead of downloading them again.
///
/// Anyone who has run ACE-Step through acestep.cpp or another build already has these
/// weights on disk, and they are gigabytes each. This opens a folder picker,
/// looks for the files the catalogue names - by name, then by matching size -
/// and hard-links or copies them into the studio's own model directory.
async fn setup_adopt(State(state): State<AppState>, body: axum::body::Bytes) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let Some(models_root) = studio_data_root().map(|root| root.join("models").join(model_manager::ENGINE_ID)) else {
        return Err(api_error(StatusCode::INTERNAL_SERVER_ERROR, "the studio has no data directory".into()));
    };
    let catalog = state.model_manager.catalog();
    // a folder an agent names is taken as it is; the window asks the user
    let named = serde_json::from_slice::<Value>(&body).ok().and_then(|value| value.get("path").and_then(Value::as_str).map(std::path::PathBuf::from));
    let picked = match named {
        Some(folder) => Some(folder),
        None => tokio::task::spawn_blocking(move || {
            rfd::FileDialog::new().set_title(format!("Folder with {} models", music_core::studio().artist)).pick_folder()
        })
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?,
    };

    let Some(folder) = picked else {
        return Ok(Json(serde_json::json!({ "picked": false, "adopted": [] })));
    };

    let _ = std::fs::create_dir_all(&models_root);
    let mut adopted: Vec<String> = Vec::new();
    std::fs::read_dir(&folder).map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    // Model sets are often kept one component per folder, so the picked
    // folder is searched with its subfolders.
    let searched = folder.clone();
    let entries = tokio::task::spawn_blocking(move || adoptable_files(&searched, ADOPT_FOLDER_DEPTH))
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    for component in &catalog.components {
        let target = models_root.join(component.filename);
        if target.is_file() {
            continue;
        }
        // The name first, because that is unambiguous; then the exact size,
        // because other builds rename the same file.
        let source = entries
            .iter()
            .find(|path| path.file_name().is_some_and(|name| name == component.filename))
            .or_else(|| {
                entries.iter().find(|path| {
                    std::fs::metadata(path).map(|meta| meta.len() == component.bytes).unwrap_or(false)
                })
            });
        let Some(source) = source else { continue };
        // A hard link costs nothing and keeps one copy on disk; a folder on
        // another drive cannot have one, so that falls back to a copy.
        if std::fs::hard_link(source, &target).is_err() && std::fs::copy(source, &target).is_err() {
            continue;
        }
        adopted.push(component.id.to_string());
    }

    let target = effective_install_target(&state).await;
    let status = state.model_manager.status(target).await;
    Ok(Json(serde_json::json!({
        "picked": true,
        "folder": folder.display().to_string(),
        "adopted": adopted,
        "status": compose_setup_status(&state, status).await,
    })))
}

/// Opens the studio's own folder in the system file manager.
///
/// Saying where the ten gigabytes are is half an answer; the other half is
/// getting there without retyping a path from a settings screen.
async fn open_data_directory() -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let Some(root) = studio_data_root() else {
        return Err(api_error(StatusCode::NOT_FOUND, "the studio has no data directory".into()));
    };
    let _ = std::fs::create_dir_all(&root);
    #[cfg(windows)]
    let opened = std::process::Command::new("explorer.exe").arg(&root).spawn();
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("open").arg(&root).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let opened = std::process::Command::new("xdg-open").arg(&root).spawn();
    opened.map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(serde_json::json!({ "opened": root.display().to_string() })))
}

async fn setup_catalog(State(state): State<AppState>) -> Json<model_manager::Catalog> {
    Json(state.model_manager.catalog())
}

async fn setup_download(
    State(state): State<AppState>,
    Json(request): Json<SetupDownloadRequest>,
) -> Result<(StatusCode, Json<model_manager::DownloadJob>), (StatusCode, Json<ApiError>)> {
    let job = state
        .model_manager
        .install(InstallRequest {
            profile_id: request.profile_id,
            component_ids: request.ids,
        })
        .await
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    if request.select {
        let state_for_completion = state.clone();
        let job_id = job.id.clone();
        tokio::spawn(async move { persist_completed_download_profile(state_for_completion, job_id).await });
    }
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn persist_completed_download_profile(state: AppState, job_id: String) {
    loop {
        let Some(job) = state.model_manager.download_job(&job_id).await else { return; };
        match job.status {
            model_manager::DownloadStatus::Completed => {
                if let Some(profile_id) = job.profile_id {
                    *state.selected_profile_id.write().await = Some(profile_id);
                    *state.selected_component_ids.write().await = None;
                } else if state.model_manager.installed_component_files(&job.component_ids).is_ok() {
                    select_components(&state, job.component_ids).await;
                }
                let _ = persist_studio_settings(&state).await;
                return;
            }
            model_manager::DownloadStatus::Cancelled | model_manager::DownloadStatus::Failed => return,
            model_manager::DownloadStatus::Downloading => tokio::time::sleep(std::time::Duration::from_millis(250)).await,
        }
    }
}

/// Makes a set of components the studio's; one that is exactly a ready-made
/// set is recorded as that set, so it is named as the model manager names it.
async fn select_components(state: &AppState, ids: Vec<String>) {
    match model_manager::profile_matching(&ids).map(str::to_owned) {
        Some(profile_id) => {
            *state.selected_profile_id.write().await = Some(profile_id);
            *state.selected_component_ids.write().await = None;
        }
        None => {
            *state.selected_profile_id.write().await = None;
            *state.selected_component_ids.write().await = Some(ids);
        }
    }
}

/// Uses a set that is already on disk.
///
/// Downloading was the only way to change which quantisation the studio runs,
/// so a machine with two sets installed was stuck on whichever arrived last.
/// This switches between what is already there, and refuses a set with a
/// missing file rather than failing at generation time.
async fn setup_select(
    State(state): State<AppState>,
    Json(request): Json<SetupSelectRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    if let Some(profile_id) = request.profile_id.clone().filter(|value| !value.trim().is_empty()) {
        let known = state.model_manager.catalog().profiles.iter().any(|profile| profile.id == profile_id);
        if !known {
            return Err(api_error(StatusCode::BAD_REQUEST, format!("unknown profile {profile_id}")));
        }
        *state.selected_profile_id.write().await = Some(profile_id);
        *state.selected_component_ids.write().await = None;
    } else {
        let ids = request.component_ids.unwrap_or_default();
        if ids.is_empty() {
            return Err(api_error(StatusCode::BAD_REQUEST, "nothing selected".to_string()));
        }
        state
            .model_manager
            .installed_component_files(&ids)
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
        select_components(&state, ids).await;
    }
    let _ = persist_studio_settings(&state).await;
    let target = effective_install_target(&state).await;
    let manager_status = state.model_manager.status(target).await;
    Ok(Json(serde_json::to_value(compose_setup_status(&state, manager_status).await).unwrap_or(Value::Null)))
}

async fn setup_cancel(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    let target = effective_install_target(&state).await;
    let manager_status = state
        .model_manager
        .cancel(target)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(compose_setup_status(&state, manager_status).await))
}

async fn capabilities(State(state): State<AppState>) -> Json<CapabilitiesResponse> {
    let primary_installed = state.music_server.health().await;
    let parakeet_installed = state.lyrics_sync.parakeet_ready();
    let whisper_installed = state.lyrics_sync.whisper_binary().is_some();
    let assistant = state.assistant.read().await.clone();
    let assistant_installed = assistant.available();
    let catalog = state.openrouter_catalog.read().await;
    Json(CapabilitiesResponse {
        engines: capability_engines_with(catalog.catalog.as_ref(), primary_installed, parakeet_installed, whisper_installed, assistant_installed),
    })
}

fn capability_engines(
    catalog: Option<&providers::openrouter::CapabilityCatalog>,
    primary_installed: bool,
) -> Vec<EngineDescriptor> {
    capability_engines_with(catalog, primary_installed, false, false, false)
}

/// The engines the studio can offer, including the local ones it only has when
/// their models are installed. Without these the "Local" button on the provider
/// page was disabled for ever: the studio recognises speech and writes captions
/// locally, but never said so here.
fn capability_engines_with(
    catalog: Option<&providers::openrouter::CapabilityCatalog>,
    primary_installed: bool,
    parakeet_installed: bool,
    whisper_installed: bool,
    assistant_installed: bool,
) -> Vec<EngineDescriptor> {
    let mut openrouter_capabilities = vec![
        Capability::SpeechToText,
        Capability::PromptEnhancement,
        Capability::CoverArt,
    ];
    if catalog.is_some_and(|catalog| catalog.models_for(Capability::MusicGeneration).next().is_some()) {
        openrouter_capabilities.push(Capability::MusicGeneration);
    }
    vec![
        EngineDescriptor {
            id: PRIMARY_MUSIC_ENGINE_ID.into(),
            display_name: format!("{} ({})", music_core::studio().artist, music_engine::server::executable_name()),
            capabilities: vec![Capability::MusicGeneration],
            execution_mode: ExecutionMode::Local,
            installed: primary_installed,
        },
        // Two different recognisers, named. "Parakeet / Whisper" was not a
        // choice, it was a shrug.
        EngineDescriptor {
            id: "parakeet".into(),
            display_name: "Parakeet TDT 0.6B (local)".into(),
            capabilities: vec![Capability::SpeechToText],
            execution_mode: ExecutionMode::Local,
            installed: parakeet_installed,
        },
        EngineDescriptor {
            id: "whisper".into(),
            display_name: "Whisper.cpp (local)".into(),
            capabilities: vec![Capability::SpeechToText],
            execution_mode: ExecutionMode::Local,
            installed: whisper_installed,
        },
        EngineDescriptor {
            id: "local-assistant".into(),
            display_name: "Local GGUF model".into(),
            capabilities: vec![Capability::PromptEnhancement],
            execution_mode: ExecutionMode::Local,
            installed: assistant_installed,
        },
        EngineDescriptor {
            id: "openrouter".into(),
            display_name: "OpenRouter".into(),
            capabilities: openrouter_capabilities,
            execution_mode: ExecutionMode::OpenRouter,
            installed: false,
        },
    ]
}

/// Gets the writing assistant off the graphics card before the engine needs it.
///
/// There is one card, and both models want all of it: Gemma holds five
/// gigabytes from the moment it writes a caption, and the engine then asks for
/// eleven more and dies. The assistant starts itself on the next request it
/// receives, so stopping it here costs a reload later and nothing else.
async fn free_the_card_for_the_engine(state: &AppState) {
    if state.assistant_runtime.base_url().await.is_some() {
        state.assistant_runtime.stop().await;
    }
}

/// What the engine's own log says about why it is not there any more.
///
/// A card that ran out of memory says so in the log and then the process is
/// gone; the studio saw only a refused connection, and told the user to
/// download models that were already on disk.
fn engine_failure_reason() -> Option<String> {
    let tail = music_engine::server::startup_log_tail(80).join("\n").to_lowercase();
    describes_exhausted_memory(&tail)
        .then(|| "The graphics card ran out of memory while the engine was loading the models. Choose a smaller quantisation in the model manager, or close whatever else is using the card - the writing assistant holds several gigabytes of its own.".to_string())
}

/// Whether a lowercased log says the card ran out of room.
fn describes_exhausted_memory(log: &str) -> bool {
    [
        "out of memory",
        "cudamalloc",
        "failed to allocate",
        "insufficient memory",
        "cudaerrormemoryallocation",
        "bad_alloc",
    ]
    .iter()
    .any(|marker| log.contains(marker))
}

async fn create_music_job(
    State(state): State<AppState>,
    Json(request): Json<CreateMusicJobRequest>,
) -> (StatusCode, Json<MusicJob>) {
    let music_selection = state.configuration.read().await.selections.iter().find(|selection| selection.capability == Capability::MusicGeneration).cloned();
    if music_selection.as_ref().is_some_and(|selection| selection.mode == ExecutionMode::OpenRouter) {
        return create_openrouter_music_job(state, request, music_selection.and_then(|selection| selection.cloud_model)).await;
    }
    let engine_id = selected_local_music_engine(&*state.configuration.read().await)
        .unwrap_or_else(|| "unconfigured".into());
    if engine_id != PRIMARY_MUSIC_ENGINE_ID {
        let job = queued_not_configured_job(request, engine_id);
        state.jobs.write().await.insert(job.id.clone(), job.clone());
        return (StatusCode::ACCEPTED, Json(job));
    }
    if state.training.active_run().await.is_some() {
        return (StatusCode::CONFLICT, Json(failed_request_job(request, engine_id, "a LoRA is training on the card; make songs once it finishes".into())));
    }
    let prepared = match ace_request_from(&state, &request).await {
        Ok(value) => value,
        Err(error) => return (StatusCode::BAD_REQUEST, Json(failed_request_job(request, engine_id, error))),
    };
    let job = MusicJob {
        derived: None,
        id: format!("ace-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        engine_id,
        cover_prompt: request.cover_prompt.clone(),
        // Without a name of the user's the song is named once the language
        // model has written it: the lyrics it plans are not in the request.
        title: request.title.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned),
        status: MusicJobStatus::Queued,
        dispatch: MusicJobDispatch::Local,
        phase: MusicJobPhase::Queued,
        caption: request.caption(),
        lyrics: request.lyrics(),
        duration_seconds: request.duration_seconds(),
        generation_settings: prepared.clone(),
        song: None,
        songs: vec![],
        message: "Waiting for the engine.".into(),
        engine_job: None,
        cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    state.jobs.write().await.insert(job.id.clone(), job.clone());
    let run = AceRun {
        job_id: job.id.clone(),
        request: prepared,
        think: request.think,
        source_song_id: request.source_song_id.clone(),
        reference_song_id: request.reference_song_id.clone(),
        fades: (request.fade_in, request.fade_out),
    };
    tokio::spawn(run_ace_job(state.clone(), run));
    (StatusCode::ACCEPTED, Json(job))
}

/// One song request on its way through the engine.
struct AceRun {
    job_id: String,
    /// The prepared engine request: every take's seed counts up from its own.
    request: Value,
    /// Let the language model plan the song before it is rendered.
    think: bool,
    source_song_id: Option<String>,
    reference_song_id: Option<String>,
    /// Fade in and fade out, seconds, put on by the studio.
    fades: (f32, f32),
}

/// Completes the page's request with what the studio decides: the model files
/// of the selected set, the adapters under their engine names, the studio's
/// defaults for anything left out.
async fn ace_request_from(state: &AppState, request: &CreateMusicJobRequest) -> Result<Value, String> {
    let mut fields = request.engine.clone();
    if !fields.contains_key("synth_model") || !fields.contains_key("lm_model") || !fields.contains_key("vae") {
        let files = selected_model_files(state).await?;
        fields.entry("synth_model").or_insert_with(|| Value::from(files.synth_model));
        fields.entry("lm_model").or_insert_with(|| Value::from(files.lm_model));
        fields.entry("vae").or_insert_with(|| Value::from(files.vae));
    }
    let root = state.model_manager.models_directory();
    for key in ["synth_model", "lm_model", "vae"] {
        if let Some(name) = fields.get(key).and_then(Value::as_str) {
            if !root.join(name).is_file() {
                return Err(format!("{name} is no longer on disk; choose another in Settings - Models"));
            }
        }
    }
    if !request.adapters.is_empty() {
        let dit_family = fields.get("synth_model").and_then(Value::as_str).and_then(music_engine::model::model_family);
        for adapter in &request.adapters {
            if let Some(problem) = adapter_misfit(&adapter.id, state.adapters.model_of(&adapter.id).as_deref(), dit_family) {
                return Err(problem);
            }
        }
        let views = adapter_views(state).await;
        let mut dit = Vec::new();
        let mut lm: Option<(String, f64)> = None;
        for adapter in &request.adapters {
            if !state.adapters.exists(&adapter.id) {
                return Err(format!("the adapter {} is not installed", adapter.id));
            }
            let view = views.get(&adapter.id);
            let is_lm = view.is_some_and(|view| view.slots.iter().any(|slot| slot == "lm"));
            let slot = if is_lm { "lm" } else { "dit" };
            let scales = if adapter.scales.is_empty() { state.adapters.starting_scales(&adapter.id, &[slot]) } else { adapter.scales.clone() };
            let scale = scales.get(slot).copied().unwrap_or(1.0);
            if !scale.is_finite() {
                return Err(format!("the strength of {} is not a number", adapter.id));
            }
            if is_lm {
                if lm.is_some() {
                    return Err("the language model takes one adapter at a time".into());
                }
                lm = Some((adapter.id.clone(), scale));
            } else {
                dit.push(serde_json::json!({ "name": adapter.id, "scale": scale }));
            }
        }
        if !dit.is_empty() {
            fields.insert("adapters".into(), Value::Array(dit));
        }
        if let Some((name, scale)) = lm {
            fields.insert("lm_adapter".into(), Value::from(name));
            fields.insert("lm_adapter_scale".into(), Value::from(scale));
        }
    }
    fields.entry("peak_clip").or_insert(Value::from(DEFAULT_PEAK_CLIP));
    fields.entry("mp3_bitrate").or_insert(Value::from(DEFAULT_MP3_KBPS));
    fields.entry("output_format").or_insert(Value::from("mp3"));
    let task = fields.get("task_type").and_then(Value::as_str).unwrap_or("text2music").to_owned();
    if ace::needs_source(&task) && request.source_song_id.is_none() {
        return Err(format!("{task} works on a recording; choose the source track"));
    }
    ace::prepare(&Value::Object(fields)).map_err(|error| error.to_string())
}

/// Why a DiT LoRA cannot go on the chosen DiT: its weights are the width of
/// the other size, and the engine merges none of them and fails the song.
fn adapter_misfit(id: &str, made_for: Option<&str>, dit: Option<&str>) -> Option<String> {
    let (made_for, dit) = (made_for?, dit?);
    (made_for != "lm" && made_for != dit).then(|| {
        let size = |family: &str| family.to_uppercase();
        format!("{id} is a LoRA for {} models and the chosen DiT is {}; switch the DiT above the create form to {} or leave this LoRA out", size(made_for), size(dit), size(made_for))
    })
}

/// The adapters the engine sees, by folder, with the half of the model each
/// one changes.
async fn adapter_views(state: &AppState) -> std::collections::BTreeMap<String, adapters::EngineView> {
    let slot_ids: Vec<&str> = music_engine::model::ADAPTER_SLOTS.iter().map(|slot| slot.id).collect();
    match tokio::time::timeout(std::time::Duration::from_secs(3), state.music_server.props()).await {
        Ok(Ok(props)) => adapters::engine_views(&props, &slot_ids),
        _ => Default::default(),
    }
}

async fn set_job(state: &AppState, job_id: &str, change: impl FnOnce(&mut MusicJob)) {
    if let Some(job) = state.jobs.write().await.get_mut(job_id) {
        change(job);
    }
}

async fn job_cancelled(state: &AppState, job_id: &str) -> bool {
    state.jobs.read().await.get(job_id).is_some_and(|job| job.cancel.load(std::sync::atomic::Ordering::SeqCst))
}

/// Runs an engine job for this song and waits for it, the engine's job id
/// kept on the song's job so cancelling reaches it.
async fn engine_step<F, Fut>(state: &AppState, job_id: &str, submit: F) -> anyhow::Result<String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<String>>,
{
    if job_cancelled(state, job_id).await {
        anyhow::bail!("cancelled");
    }
    let engine_job = submit().await?;
    set_job(state, job_id, |job| job.engine_job = Some(engine_job.clone())).await;
    // a cancel that landed between the submit and the line above
    if job_cancelled(state, job_id).await {
        let _ = state.music_server.cancel(&engine_job).await;
    }
    state.music_server.wait(&engine_job).await?;
    Ok(engine_job)
}

async fn run_ace_job(state: AppState, run: AceRun) {
    let job_id = run.job_id.clone();
    let outcome = {
        let _using = state.engine_use.read().await;
        ace_job(&state, run).await
    };
    let cancelled = job_cancelled(&state, &job_id).await;
    set_job(&state, &job_id, |job| {
        job.engine_job = None;
        match outcome {
            Ok(songs) => {
                job.status = MusicJobStatus::Completed;
                job.phase = MusicJobPhase::Completed;
                job.song = songs.first().cloned();
                job.songs = songs;
                job.message = "The tracks are in the library.".into();
            }
            Err(_) if cancelled => {
                job.status = MusicJobStatus::Cancelled;
                job.phase = MusicJobPhase::Cancelled;
                job.message = "Cancelled.".into();
            }
            Err(error) => {
                job.status = MusicJobStatus::Failed;
                job.phase = MusicJobPhase::Failed;
                job.message = format!("{error:#}");
            }
        }
    })
    .await;
}

async fn ace_job(state: &AppState, run: AceRun) -> anyhow::Result<Vec<CompletedSong>> {
    let AceRun { job_id, request, think, source_song_id, reference_song_id, fades } = run;
    let task = request.get("task_type").and_then(Value::as_str).unwrap_or("text2music").to_owned();
    let has_codes = request.get("audio_codes").and_then(Value::as_str).is_some_and(|codes| !codes.trim().is_empty());
    set_job(state, &job_id, |job| {
        job.status = MusicJobStatus::Running;
        job.phase = MusicJobPhase::Running;
    })
    .await;

    // The language model plans text2music: metadata, lyrics when there are
    // none, the audio codes. A cover or an edit is planned by its source.
    let planned = if think && task == "text2music" && !has_codes {
        set_job(state, &job_id, |job| job.message = "The language model is writing the song.".into()).await;
        let mut lm_request = request.clone();
        lm_request["lm_mode"] = Value::from("generate");
        let lm_job = engine_step(state, &job_id, || state.music_server.submit_lm(std::slice::from_ref(&lm_request))).await?;
        state.music_server.lm_result(&lm_job).await?
    } else {
        vec![request.clone()]
    };

    let encode_here = studio_encodes(&request, fades);
    let requests = ace::synth_requests(planned, encode_here);
    let mut sources = ace::Sources::default();
    if let Some(id) = &source_song_id {
        let (audio, latent) = song_audio_and_latent(state, id)?;
        sources.src_audio = Some(audio);
        sources.src_latents = latent;
    }
    if let Some(id) = &reference_song_id {
        let (audio, latent) = song_audio_and_latent(state, id)?;
        sources.ref_audio = Some(audio);
        sources.ref_latents = latent;
    }
    set_job(state, &job_id, |job| job.message = "The DiT is rendering the song.".into()).await;
    let synth_job = engine_step(state, &job_id, || state.music_server.submit_synth(&requests, sources)).await?;
    let (content_type, body) = state.music_server.result(&synth_job).await?;
    let takes = ace::takes(&requests, &content_type, &body)?;
    let job = state.jobs.read().await.get(&job_id).cloned().context("the job disappeared")?;
    let mut imported = Vec::with_capacity(takes.len());
    for take in takes {
        imported.push(import_take(state, &job, &request, take, source_song_id.as_deref(), reference_song_id.as_deref(), fades).await?);
    }
    Ok(imported)
}

/// A library track's audio, and the latent it was rendered from when the
/// studio kept one (the engine then skips the VAE encode).
fn song_audio_and_latent(state: &AppState, song_id: &str) -> anyhow::Result<(Vec<u8>, Option<Vec<u8>>)> {
    let song = state.library.get_song(song_id)?.with_context(|| format!("the track {song_id} is not in the library"))?;
    let path = state.library.media_path_for_song(&song).with_context(|| format!("{} has no audio file", song.title))?;
    let audio = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let latent = std::fs::read(latent_path(state, song_id)).ok();
    Ok((audio, latent))
}

/// The engine's latent of a rendered track, beside its audio.
fn latent_path(state: &AppState, song_id: &str) -> PathBuf {
    state.library.media_dir().join(format!("{song_id}.latent"))
}

async fn import_take(
    state: &AppState,
    job: &MusicJob,
    submitted: &Value,
    take: ace::Take,
    source_song_id: Option<&str>,
    reference_song_id: Option<&str>,
    fades: (f32, f32),
) -> anyhow::Result<CompletedSong> {
    let mut replay = take.replay;
    // the replay asks for what the page asked for, not for the float WAV the
    // studio encodes from
    for key in ["output_format", "mp3_bitrate"] {
        match submitted.get(key) {
            Some(value) => replay[key] = value.clone(),
            None => {
                if let Some(fields) = replay.as_object_mut() {
                    fields.remove(key);
                }
            }
        }
    }
    let mut extension = multipart::audio_extension(&take.audio_type)?;
    let mut audio = take.audio;
    if studio_encodes(submitted, fades) && extension == "wav" {
        let format = submitted.get("output_format").and_then(Value::as_str).unwrap_or("mp3").to_owned();
        let kbps = submitted.get("mp3_bitrate").and_then(Value::as_u64).map_or(DEFAULT_MP3_KBPS, |value| value as u32);
        let peak_clip = submitted.get("peak_clip").and_then(Value::as_u64).map_or(DEFAULT_PEAK_CLIP, |value| value as u32);
        let target = format.clone();
        audio = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
            let mut stereo = audio_pcm::decode_stereo_bytes(audio, "wav")?;
            // a NaN would become the peak and turn the whole track into silence
            let broken = stereo.left.iter().chain(&stereo.right).filter(|sample| !sample.is_finite()).count();
            if broken > 0 {
                anyhow::bail!("the engine returned {broken} broken samples (NaN or infinity); make the song again");
            }
            audio_post::encode::fade(&mut stereo, fades.0, fades.1);
            if target == "wav32" {
                return audio_pcm::wav_bytes(&stereo, "wav32");
            }
            audio_post::encode::normalize_peak(&mut stereo, peak_clip);
            match target.as_str() {
                "mp3" => audio_post::encode::mp3(&stereo, kbps),
                "flac" => audio_post::encode::flac(&stereo),
                _ => audio_pcm::wav_bytes(&stereo, &target),
            }
        })
        .await
        .context("the encoder stopped")??;
        if format == "mp3" {
            extension = "mp3";
            replay["mp3_bitrate"] = Value::from(audio_post::encode::mp3_bitrate(kbps));
        } else if format == "flac" {
            extension = "flac";
        }
        replay["output_format"] = Value::from(format);
    }
    if fades.0 > 0.0 || fades.1 > 0.0 {
        replay["fade_in"] = Value::from(fades.0);
        replay["fade_out"] = Value::from(fades.1);
    }
    if let Some(id) = source_song_id {
        replay["source_song_id"] = Value::from(id);
    }
    if let Some(id) = reference_song_id {
        replay["reference_song_id"] = Value::from(id);
    }
    let caption = replay.get("caption").and_then(Value::as_str).unwrap_or(&job.caption).to_owned();
    let lyrics = replay.get("lyrics").and_then(Value::as_str).unwrap_or(&job.lyrics).to_owned();
    let audio_codes = replay.get("audio_codes").filter(|codes| codes.as_str().is_some_and(|codes| !codes.is_empty())).cloned();
    let mut generation_settings = replay.clone();
    if let Some(fields) = generation_settings.as_object_mut() {
        fields.remove("audio_codes");
    }
    let bitrate = replay.get("mp3_bitrate").and_then(Value::as_u64).map(|value| value as u32);
    let metadata = serde_json::json!({
        "duration_seconds": library::audio_duration_seconds(&audio, extension, bitrate),
        "seed": replay.get("seed"),
        "lm_seed": replay.get("lm_seed"),
        "bpm": replay.get("bpm"),
        "keyscale": replay.get("keyscale"),
        "timesignature": replay.get("timesignature"),
        "vocal_language": replay.get("vocal_language"),
        "task_type": replay.get("task_type"),
        "synth_model": replay.get("synth_model"),
        "output_format": replay.get("output_format"),
        "cover_prompt": job.cover_prompt.clone(),
        "derived": job.derived.clone(),
    });
    let profile_id = state.selected_profile_id.read().await.clone();
    let instrumental = lyrics.trim().is_empty() || lyrics.trim() == "[Instrumental]";
    let title = match job.title.clone() {
        Some(title) => title,
        None => {
            // the words that switch the song's LoRA on say nothing about it
            let triggers: Vec<String> = replay
                .get("adapters")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|adapter| adapter.get("name").and_then(Value::as_str))
                .chain(replay.get("lm_adapter").and_then(Value::as_str))
                .filter_map(|id| state.adapters.trigger_of(id))
                .collect();
            auto_title::auto_title(&auto_title::without_triggers(&caption, &triggers), &lyrics, instrumental)
        }
    };
    let imported = state.library.import_generated_song(library::GeneratedSongInput {
        title: Some(title),
        metadata,
        caption,
        lyrics,
        generation_settings,
        replay_request: Some(replay),
        audio_codes,
        engine_id: job.engine_id.clone(),
        profile_id,
        source: "local_generation".into(),
        audio_extension: extension,
        audio,
    })?;
    if !take.latent.is_empty() {
        std::fs::write(latent_path(state, &imported.song.id), &take.latent)
            .with_context(|| format!("keep the latent of {}", imported.song.title))?;
    }
    let song_id = imported.song.id.clone();
    tag_stored_song(state, &song_id).await;
    {
        let (cover_state, timing_state) = (state.clone(), state.clone());
        let (cover_song, timing_song) = (song_id.clone(), song_id.clone());
        tokio::spawn(async move { draw_cover_for(cover_state, cover_song).await });
        tokio::spawn(async move { time_lyrics_for(timing_state, timing_song).await });
    }
    Ok(CompletedSong { id: song_id.clone(), audio_url: format!("/v1/library/media/{song_id}"), song: imported.song })
}

#[derive(Debug, Deserialize)]
struct PlanRequest {
    /// inspire: a one-line idea becomes caption, lyrics and metadata;
    /// format: what is written gets its metadata completed and tidied.
    mode: String,
    request: Value,
}

/// The engine's language model plans a song without rendering it.
async fn ace_plan(State(state): State<AppState>, Json(input): Json<PlanRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    if !matches!(input.mode.as_str(), "inspire" | "format") {
        return Err(api_error(StatusCode::BAD_REQUEST, "mode is inspire or format".into()));
    }
    if state.training.active_run().await.is_some() {
        return Err(api_error(StatusCode::CONFLICT, "a LoRA is training on the card; plan once it finishes".into()));
    }
    let mut request = input.request.as_object().cloned().ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "request must be an object".into()))?;
    // the planner the studio renders with, whether the set is a profile or picked by hand
    if !request.contains_key("lm_model") {
        let files = selected_model_files(&state).await.map_err(|error| api_error(StatusCode::CONFLICT, error))?;
        request.insert("lm_model".into(), Value::from(files.lm_model));
    }
    request.insert("lm_mode".into(), Value::from(input.mode));
    let request = Value::Object(request);
    let _using = state.engine_use.read().await;
    let job = state.music_server.submit_lm(std::slice::from_ref(&request)).await.map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
    state.music_server.wait(&job).await.map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))?;
    let planned = state.music_server.lm_result(&job).await.map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))?;
    Ok(Json(serde_json::json!({ "plan": planned.into_iter().next() })))
}

#[derive(Debug, Deserialize)]
struct UnderstandRequest {
    song_id: String,
}

/// The engine listens to a library track: its caption, lyrics, metadata and
/// the audio codes that let a render follow its structure.
async fn ace_understand(State(state): State<AppState>, Json(input): Json<UnderstandRequest>) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    if state.training.active_run().await.is_some() {
        return Err(api_error(StatusCode::CONFLICT, "a LoRA is training on the card; listen once it finishes".into()));
    }
    let (audio, _) = song_audio_and_latent(&state, &input.song_id).map_err(|error| api_error(StatusCode::NOT_FOUND, error.to_string()))?;
    let files = selected_model_files(&state).await.map_err(|error| api_error(StatusCode::CONFLICT, error))?;
    let lm_model = Some(serde_json::json!({ "lm_model": files.lm_model }));
    let _using = state.engine_use.read().await;
    let job = state.music_server.submit_understand(audio, lm_model).await.map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
    state.music_server.wait(&job).await.map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))?;
    let (content_type, body) = state.music_server.result(&job).await.map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))?;
    let heard = ace::understood(&content_type, &body).map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.to_string()))?;
    Ok(Json(serde_json::json!({ "request": heard })))
}

/// The jobs still in flight, oldest first, so a reloaded window can show them.
async fn list_active_music_jobs(State(state): State<AppState>) -> Json<Vec<MusicJob>> {
    let mut active: Vec<MusicJob> = state
        .jobs
        .read()
        .await
        .values()
        .filter(|job| matches!(job.status, MusicJobStatus::Queued | MusicJobStatus::Running))
        .cloned()
        .collect();
    active.sort_by(|a, b| a.id.cmp(&b.id));
    Json(active)
}

/// Renders a track again from its replay record: the same plan and seed, or
/// the changes the re-render asks for.
async fn replay_music_job(
    State(state): State<AppState>,
    Json(request): Json<ReplayMusicJobRequest>,
) -> Result<(StatusCode, Json<MusicJob>), (StatusCode, Json<ApiError>)> {
    if selected_local_music_engine(&*state.configuration.read().await).as_deref() != Some(PRIMARY_MUSIC_ENGINE_ID) {
        return Err(api_error(StatusCode::CONFLICT, "Re-rendering needs the local ACE-Step engine.".into()));
    }
    let mut source_title = None;
    let mut original = None;
    let replay = match (&request.song_id, &request.replay_request) {
        (Some(_), Some(_)) => return Err(api_error(StatusCode::BAD_REQUEST, "Provide either song_id or replay_request, not both.".into())),
        (Some(song_id), None) => {
            let song = state.library.get_song(song_id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
                .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Song not found.".into()))?;
            source_title = Some(song.title.clone());
            let replay = song.replay_request.clone().ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "This track has no replay record.".into()))?;
            original = Some(song);
            replay
        }
        (None, Some(replay)) => replay.clone(),
        (None, None) => return Err(api_error(StatusCode::BAD_REQUEST, "Provide song_id or replay_request.".into())),
    };
    if state.training.active_run().await.is_some() {
        return Err(api_error(StatusCode::CONFLICT, "a LoRA is training on the card; re-render once it finishes".into()));
    }
    let (prepared, source_song_id, reference_song_id, fades) =
        prepare_replay(replay, &request).map_err(|error| api_error(StatusCode::BAD_REQUEST, error))?;
    let caption = prepared.get("caption").and_then(Value::as_str).unwrap_or_default().to_owned();
    let lyrics = prepared.get("lyrics").and_then(Value::as_str).unwrap_or_default().to_owned();
    let job = MusicJob {
        derived: original.as_ref().map(|song| derivation(song, "replay", serde_json::json!({ "steps": request.steps, "seed": request.seed, "output_format": request.output_format }))),
        cover_prompt: None,
        id: format!("ace-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        engine_id: PRIMARY_MUSIC_ENGINE_ID.into(),
        title: source_title,
        status: MusicJobStatus::Queued,
        dispatch: MusicJobDispatch::Local,
        phase: MusicJobPhase::Queued,
        caption,
        lyrics,
        duration_seconds: prepared.get("duration").and_then(Value::as_f64).unwrap_or_default(),
        generation_settings: prepared.clone(),
        song: None,
        songs: vec![],
        message: "Waiting for the engine.".into(),
        engine_job: None,
        cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    state.jobs.write().await.insert(job.id.clone(), job.clone());
    tokio::spawn(run_ace_job(state.clone(), AceRun { job_id: job.id.clone(), request: prepared, think: false, source_song_id, reference_song_id, fades }));
    Ok((StatusCode::ACCEPTED, Json(job)))
}

/// A replay record with the re-render's changes: the plan (audio codes) stays,
/// so the language model does not run again.
fn prepare_replay(mut replay: Value, overrides: &ReplayMusicJobRequest) -> Result<(Value, Option<String>, Option<String>, (f32, f32)), String> {
    let object = replay.as_object_mut().ok_or("the replay record must be a JSON object")?;
    let source = object.remove("source_song_id").and_then(|value| value.as_str().map(str::to_owned));
    let fade_in = object.remove("fade_in").and_then(|value| value.as_f64()).unwrap_or(0.0) as f32;
    let fade_out = object.remove("fade_out").and_then(|value| value.as_f64()).unwrap_or(0.0) as f32;
    let reference = object.remove("reference_song_id").and_then(|value| value.as_str().map(str::to_owned));
    if let Some(steps) = overrides.steps {
        if steps < 1 {
            return Err("steps must be at least 1".into());
        }
        object.insert("inference_steps".into(), Value::from(steps));
    }
    if let Some(seed) = overrides.seed {
        object.insert("seed".into(), Value::from(seed));
    }
    if let Some(format) = &overrides.output_format {
        if !matches!(format.as_str(), "mp3" | "wav16" | "wav24" | "wav32" | "flac") {
            return Err("output_format must be mp3, wav16, wav24, wav32 or flac".into());
        }
        object.insert("output_format".into(), Value::from(format.clone()));
    }
    if let Some(changes) = &overrides.changes {
        for (key, value) in changes.as_object().ok_or("changes must be a JSON object")? {
            object.insert(key.clone(), value.clone());
        }
    }
    let prepared = ace::prepare(&replay).map_err(|error| error.to_string())?;
    Ok((prepared, source, reference, (fade_in, fade_out)))
}

async fn create_openrouter_music_job(state: AppState, request: CreateMusicJobRequest, model_id: Option<String>) -> (StatusCode, Json<MusicJob>) {
    let engine_id = "openrouter".to_owned();
    let model_id = match model_id.filter(|model| !model.trim().is_empty()) {
        Some(model) => model,
        None => return (StatusCode::BAD_REQUEST, Json(failed_request_job(request, engine_id, "OpenRouter music requires a selected model from the refreshed catalog".into()))),
    };
    let prompt = openrouter_music_prompt(&request);
    let catalog = match state.openrouter_catalog.read().await.catalog.clone() {
        Some(catalog) => catalog,
        None => return (StatusCode::BAD_REQUEST, Json(failed_request_job(request, engine_id, "Refresh the OpenRouter catalog before starting cloud music generation".into()))),
    };
    let stream_request = match providers::openrouter::music_stream_request_for(&catalog, &model_id, &prompt) {
        Ok(request) => request,
        Err(error) => return (StatusCode::BAD_REQUEST, Json(failed_request_job(request, engine_id, error.to_string()))),
    };
    let job = MusicJob {
        derived: None,
        id: format!("openrouter-{}", uuid_suffix()), client_ref: request.client_ref.clone(), engine_id: engine_id.clone(), cover_prompt: request.cover_prompt.clone(), title: Some(titled(&request)), status: MusicJobStatus::Running,
        dispatch: MusicJobDispatch::OpenRouter, phase: MusicJobPhase::Running, caption: request.caption(), lyrics: request.lyrics(),
        duration_seconds: request.duration_seconds(), generation_settings: stream_request.request.body.clone(), song: None, songs: vec![],
        engine_job: None, cancel: Default::default(),
        message: "OpenRouter music stream started; the completed audio will be imported into the studio library.".into(),
    };
    state.jobs.write().await.insert(job.id.clone(), job.clone());
    let job_id = job.id.clone();
    tokio::spawn(async move { run_openrouter_music_generation(state, job_id, stream_request).await });
    (StatusCode::ACCEPTED, Json(job))
}

fn openrouter_music_prompt(request: &CreateMusicJobRequest) -> String {
    format!("Music style:\n{}\n\nLyrics:\n{}", request.caption().trim(), request.lyrics().trim())
}

async fn run_openrouter_music_generation(state: AppState, job_id: String, stream_request: providers::openrouter::OpenRouterMusicStreamRequest) {
    let outcome = async {
        let response = reqwest::Client::new()
            .post(format!("{}{}", providers::openrouter::API_BASE_URL, stream_request.request.path))
            .bearer_auth(stream_request.api_key)
            .json(&stream_request.request.body)
            .send().await?
            .error_for_status()?;
        let mut stream = response.bytes_stream();
        let mut sse = Vec::new();
        while let Some(chunk) = stream.next().await { sse.extend_from_slice(&chunk?); }
        let audio = openrouter_stream::decode_audio_sse(&sse)?;
        let job = state.jobs.read().await.get(&job_id).cloned().context("cloud music job disappeared before import")?;
        let imported_song = state.library.import_generated_song(library::GeneratedSongInput {
            title: job.title.clone(),
            metadata: serde_json::json!({ "duration_seconds": job.duration_seconds, "cover_prompt": job.cover_prompt.clone() }),
            caption: job.caption.clone(), lyrics: job.lyrics.clone(), generation_settings: job.generation_settings.clone(),
            replay_request: None, audio_codes: None, engine_id: "openrouter".into(), profile_id: None,
            source: "openrouter_generation".into(), audio_extension: "wav", audio,
        })?;
        // A track goes into the library carrying its own name, style and words.
        tag_stored_song(&state, &imported_song.song.id).await;
        {
            let state = state.clone();
            let song_id = imported_song.song.id.clone();
            let timing_state = state.clone();
            let timing_song = song_id.clone();
            tokio::spawn(async move { draw_cover_for(state, song_id).await });
            tokio::spawn(async move { time_lyrics_for(timing_state, timing_song).await });
        }
        Ok::<CompletedSong, anyhow::Error>(CompletedSong { id: imported_song.song.id.clone(), audio_url: format!("/v1/library/media/{}", imported_song.song.id), song: imported_song.song })
    }.await;
    let mut jobs = state.jobs.write().await;
    let Some(job) = jobs.get_mut(&job_id) else { return; };
    match outcome {
        Ok(song) => { job.status = MusicJobStatus::Completed; job.phase = MusicJobPhase::Completed; job.song = Some(song.clone()); job.songs = vec![song]; job.message = "OpenRouter music stream completed and its audio was imported into the studio library.".into(); }
        Err(error) => { job.status = MusicJobStatus::Failed; job.phase = MusicJobPhase::Failed; job.message = format!("OpenRouter music generation failed: {error}"); }
    }
}

async fn music_job_status(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<MusicJob>, (StatusCode, Json<ApiError>)> {
    state
        .jobs
        .read()
        .await
        .get(&job_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Music job was not found.".into()))
}

async fn cancel_music_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<MusicJob>, (StatusCode, Json<ApiError>)> {
    let job = state
        .jobs
        .read()
        .await
        .get(&job_id)
        .cloned()
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Music job was not found.".into()))?;
    if job.engine_id != PRIMARY_MUSIC_ENGINE_ID {
        return Err(api_error(
            StatusCode::NOT_IMPLEMENTED,
            format!("The selected engine '{}' has no installed cancel adapter.", job.engine_id),
        ));
    }
    if matches!(job.status, MusicJobStatus::Queued | MusicJobStatus::Running) {
        job.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(engine_job) = &job.engine_job {
            state.music_server.cancel(engine_job).await.map_err(|error| {
                api_error(StatusCode::SERVICE_UNAVAILABLE, format!("the engine did not take the cancel: {error}"))
            })?;
        }
    }
    let current = state.jobs.read().await.get(&job_id).cloned().unwrap_or(job);
    Ok(Json(current))
}

async fn local_music_model_catalog(
    State(state): State<AppState>,
) -> Result<Json<LocalMusicModelCatalog>, (StatusCode, Json<ApiError>)> {
    let engine_id = selected_local_music_engine(&*state.configuration.read().await).ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "No local music engine is selected in the capability configuration.".into(),
        )
    })?;
    if engine_id != PRIMARY_MUSIC_ENGINE_ID {
        return Err(api_error(
            StatusCode::NOT_IMPLEMENTED,
            format!("The selected engine '{engine_id}' has no installed server catalog adapter."),
        ));
    }
    let catalog = state.music_server.props().await.map_err(|error| {
        api_error(StatusCode::SERVICE_UNAVAILABLE, format!("the engine's model catalog is unavailable: {error}"))
    })?;
    Ok(Json(LocalMusicModelCatalog { engine_id, catalog }))
}


fn initial_configuration() -> StudioConfiguration {
    let mut configuration = StudioConfiguration::default();
    if let Some(selection) = configuration
        .selections
        .iter_mut()
        .find(|selection| selection.capability == Capability::MusicGeneration)
    {
        selection.mode = ExecutionMode::Local;
        selection.local_engine = Some(PRIMARY_MUSIC_ENGINE_ID.into());
        selection.cloud_model = None;
    }
    configuration
}

/// Settings written by an earlier build may still name local engines this
/// build does not ship (an ASR or LLM engine, for example). Keeping them would
/// make the UI offer a provider the engine registry cannot serve, so any local
/// engine that is not declared by `capability_engines` is dropped and the
/// capability falls back to a mode that actually has an implementation.
fn sanitize_persisted_configuration(mut configuration: StudioConfiguration) -> StudioConfiguration {
    let declared = capability_engines(None, false);
    for selection in &mut configuration.selections {
        let engine_serves_capability = selection.local_engine.as_deref().is_some_and(|engine_id| {
            declared.iter().any(|engine| {
                engine.id == engine_id
                    && engine.execution_mode == ExecutionMode::Local
                    && engine.capabilities.contains(&selection.capability)
            })
        });
        if engine_serves_capability {
            continue;
        }
        selection.local_engine = None;
        if selection.mode == ExecutionMode::Local {
            selection.mode = ExecutionMode::OpenRouter;
        }
    }
    configuration
}

fn selected_local_music_engine(configuration: &StudioConfiguration) -> Option<String> {
    configuration
        .selections
        .iter()
        .find(|selection| {
            selection.capability == Capability::MusicGeneration
                && selection.mode == ExecutionMode::Local
        })
        .and_then(|selection| selection.local_engine.clone())
}

/// The bitrate a track is encoded at when the request names none.
const DEFAULT_MP3_KBPS: u32 = 320;
/// Samples per million allowed to clip when the level is set, as the engine does.
const DEFAULT_PEAK_CLIP: u32 = 10;

/// Whether the studio makes this track's MP3 itself; the engine's own default
/// output is MP3, so a request naming no format counts.
/// MP3 (LAME) and FLAC are encoded by the studio, not the engine.
fn studio_encodes_format(settings: &Value) -> bool {
    settings.get("output_format").and_then(Value::as_str).is_none_or(|format| format == "mp3" || format == "flac")
}

/// The studio makes the file itself (from the engine's float WAV) for the
/// formats it encodes, and whenever it puts fades on.
fn studio_encodes(settings: &Value, fades: (f32, f32)) -> bool {
    studio_encodes_format(settings) || fades.0 > 0.0 || fades.1 > 0.0
}





fn queued_not_configured_job(request: CreateMusicJobRequest, engine_id: String) -> MusicJob {
    MusicJob {
        derived: None,
        cover_prompt: None,
        id: format!("unconfigured-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        engine_id,
        title: request.title.clone(),
        status: MusicJobStatus::Queued,
        dispatch: MusicJobDispatch::NotConfigured,
        phase: MusicJobPhase::Queued,
        caption: request.caption(),
        lyrics: request.lyrics(),
        duration_seconds: request.duration_seconds(),
        generation_settings: serde_json::Value::Null,
        song: None,
        songs: vec![],
        engine_job: None,
        cancel: Default::default(),
        message: "The selected local music engine is not configured; this job remains queued and no inference has started.".into(),
    }
}

fn failed_request_job(request: CreateMusicJobRequest, engine_id: String, error: String) -> MusicJob {
    MusicJob {
        derived: None,
        cover_prompt: None,
        title: request.title.clone(),
        id: format!("rejected-{}", uuid_suffix()),
        client_ref: request.client_ref.clone(),
        engine_id,
        status: MusicJobStatus::Failed,
        dispatch: MusicJobDispatch::NotConfigured,
        phase: MusicJobPhase::Failed,
        caption: request.caption(),
        lyrics: request.lyrics(),
        duration_seconds: request.duration_seconds(),
        generation_settings: serde_json::Value::Null,
        song: None,
        songs: vec![],
        engine_job: None,
        cancel: Default::default(),
        message: error,
    }
}

fn uuid_suffix() -> String {
    uuid::Uuid::now_v7().to_string()
}


fn api_error(status: StatusCode, error: String) -> (StatusCode, Json<ApiError>) {
    (status, Json(ApiError { error }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_song_file_is_found_in_the_media_folder_however_its_path_is_written() {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        let track = media.join("song.mp3");
        std::fs::write(&track, b"mp3").unwrap();
        std::fs::write(root.path().join("outside.mp3"), b"mp3").unwrap();
        assert!(in_media_folder(&media, &track.canonicalize().unwrap()), "a canonical path is in the folder");
        assert!(in_media_folder(&media, &track));
        assert!(!in_media_folder(&media, &root.path().join("outside.mp3")));
        assert!(!in_media_folder(&media, &media.join("missing.mp3")));
    }

    #[test]
    fn a_lora_of_the_other_size_is_refused_before_the_engine() {
        assert!(adapter_misfit("synthwave", Some("xl"), Some("2b")).is_some_and(|problem| problem.contains("for XL models") && problem.contains("chosen DiT is 2B")));
        assert_eq!(adapter_misfit("synthwave", Some("xl"), Some("xl")), None);
        assert_eq!(adapter_misfit("planner", Some("lm"), Some("2b")), None);
        assert_eq!(adapter_misfit("unknown", None, Some("2b")), None);
    }

    #[test]
    fn the_window_mark_comes_back_on_the_job_and_never_reaches_the_engine() {
        let request: CreateMusicJobRequest = serde_json::from_value(serde_json::json!({
            "caption": "synth-pop", "lyrics": "[Instrumental]", "client_ref": "temp_1",
        }))
        .unwrap();
        assert!(!request.engine.contains_key("client_ref"));
        let job = failed_request_job(request, "acestep-cpp".into(), "x".into());
        assert_eq!(serde_json::to_value(&job).unwrap()["client_ref"], "temp_1");

        let agent: CreateMusicJobRequest = serde_json::from_value(serde_json::json!({ "caption": "synth-pop" })).unwrap();
        let job = failed_request_job(agent, "acestep-cpp".into(), "x".into());
        assert!(serde_json::to_value(&job).unwrap().get("client_ref").is_none());
    }

    #[test]
    fn models_are_found_in_subfolders_but_not_hidden_ones() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("YuE2").join("vae");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.path().join(".cache")).unwrap();
        std::fs::write(root.path().join("top.gguf"), b"a").unwrap();
        std::fs::write(nested.join("deep.gguf"), b"b").unwrap();
        std::fs::write(root.path().join(".cache").join("hidden.gguf"), b"c").unwrap();
        let mut names: Vec<String> = adoptable_files(root.path(), ADOPT_FOLDER_DEPTH).iter().map(|path| path.file_name().unwrap().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["deep.gguf", "top.gguf"]);
        assert_eq!(adoptable_files(root.path(), 0).len(), 1, "depth 0 is the folder itself");
    }

    #[test]
    fn a_device_failure_is_told_from_running_out_of_memory() {
        let ptx = "ggml_cuda_compute_forward: get_rows failed\ncuda error: the provided ptx was compiled with an unsupported toolchain.";
        assert!(describes_device_failure(ptx));
        assert!(describes_device_failure("[load] fatal: self-test on vulkan0 failed (status -1, result nan, expected 132)"));
        assert!(describes_device_failure("[load] fatal: mm3_cuda_backend=c:\\x\\cuda13\\ggml-cuda.dll did not load"));
        assert!(!describes_device_failure("cuda error: out of memory\ncudamalloc failed"));
        assert!(!describes_device_failure("[load] self-test on cuda0: ok (1.2 ms)\n[server] listening on 127.0.0.1:8085"));
    }

    #[test]
    fn an_engine_that_left_without_a_word_counts_as_a_lost_device() {
        assert!(died_silently("[dit] graph: 1236 nodes, t=689, b=2"));
        assert!(!died_silently("ggml_assert: x failed"));
        assert!(!died_silently("cuda error: out of memory"));
    }

    #[test]
    fn a_device_chosen_in_settings_is_the_only_one_tried() {
        use music_engine::server::ComputeBackend;
        for device in [ComputeBackend::Cuda, ComputeBackend::Vulkan, ComputeBackend::Cpu] {
            let options = EngineOptions { backend: device, ..EngineOptions::default() };
            assert_eq!(options.device_chain(&[device]), vec![device]);
        }
        let auto = EngineOptions::default();
        assert_eq!(auto.device_chain(&[ComputeBackend::Cuda, ComputeBackend::Vulkan]), vec![ComputeBackend::Cpu]);
        assert_eq!(auto.device_chain(&[]).last(), Some(&ComputeBackend::Cpu));
    }

    #[test]
    fn the_editor_page_loads_only_embedded_files() {
        let page = EDITOR
            .get_file("index.html")
            .and_then(|file| file.contents_utf8())
            .expect("the editor page is embedded");
        let mut checked = 0;
        for attribute in ["src=\"", "href=\""] {
            for (at, _) in page.match_indices(attribute) {
                let rest = &page[at + attribute.len()..];
                let target = &rest[..rest.find('"').expect("a closed attribute")];
                if target.contains(':') || target.starts_with('#') || target.is_empty() {
                    continue;
                }
                assert!(EDITOR.get_file(target).is_some(), "the editor page loads {target}, which is not embedded");
                checked += 1;
            }
        }
        assert!(checked > 20, "only {checked} local references were found in the editor page");
    }

    /// Nothing stays in VRAM unless the user asked for it. This is the setting
    /// the assistant's unload is tied to, and it is off to begin with.
    #[test]
    fn nothing_is_kept_in_memory_by_default() {
        assert!(!EngineOptions::default().keep_loaded);
        assert!(!EngineOptions::default().to_engine().keep_loaded);
    }

    /// A card that ran out of memory has to say so. The studio used to answer
    /// with "download the five components", pointing at models already on disk.
    #[test]
    fn an_out_of_memory_engine_is_named_as_one() {
        for line in [
            "ggml_backend_cuda_buffer_type_alloc_buffer: allocating 4096 MB on device 0: cudaMalloc failed: out of memory",
            "CUDA error: out of memory",
            "std::bad_alloc",
        ] {
            assert!(
                describes_exhausted_memory(&line.to_lowercase()),
                "this is what running out of memory looks like and it was not recognised: {line}"
            );
        }
        assert!(!describes_exhausted_memory("loading model from disk"));
    }

    #[test]
    fn primary_engine_is_selected_by_default() {
        assert_eq!(
            selected_local_music_engine(&initial_configuration()).as_deref(),
            Some(PRIMARY_MUSIC_ENGINE_ID)
        );
    }

    #[test]
    fn persisted_settings_round_trip_a_complete_custom_component_selection() {
        let settings = PersistedStudioSettings {
            engine_options: EngineOptions { keep_loaded: true, max_batch: Some(2), ..EngineOptions::default() },
            assistant: AssistantConfig { provider: AssistantProvider::Local, local_base_url: Some("http://127.0.0.1:8080/v1".into()), local_model: Some("gemma".into()), openrouter_model: None, managed_model: None, managed_path: None, reasoning_effort: None },
            configuration: initial_configuration(),
            // Karaoke is off by default and has to survive a restart the same
            // way the assistant does.
            lyrics_sync: lyrics_sync::LyricsSyncConfig {
                enabled: true,
                provider: lyrics_sync::AsrProvider::Parakeet,
                whisper_model: None,
                openrouter_model: None,
                runtime: lyrics_sync::OnnxFlavour::default(),
            },
            selected_profile_id: None,
            selected_component_ids: Some(vec!["lm-q8".into(), "depth-q8".into(), "condition-f32".into(), "dit-q6".into(), "vocoder-f32".into()]),
            cover_templates: Some(cover_prompt::default_templates()),
            cover_auto: Some(true),
            separation: Some(separation::SeparationConfig::default()),
            cover_template_default: Some("photographic".into()),
        };
        let restored: PersistedStudioSettings = serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(restored.lyrics_sync.available());
        assert_eq!(restored.lyrics_sync.provider, lyrics_sync::AsrProvider::Parakeet);
        assert!(restored.selected_profile_id.is_none());
        assert_eq!(restored.selected_component_ids.unwrap(), vec!["lm-q8", "depth-q8", "condition-f32", "dit-q6", "vocoder-f32"]);
        // Engine flags survive a restart, and the songs-per-request ceiling is
        // derived from them rather than assumed.
        assert!(restored.engine_options.keep_loaded);
        assert_eq!(restored.engine_options.effective_max_batch(), 2);
        // Four by default: songs decoded together are nearly free after the
        // first, and the upstream default of 1 left the slider disabled.
        // Nothing is reserved that nobody asked for.
        assert_eq!(EngineOptions::default().effective_max_batch(), 1);
        // And whatever it is, the engine is started with it: the request
        // carries `lm_batch_size`, and the engine refuses anything above the
        // ceiling it was loaded with. Offering more in the panel than the
        // engine was given is what made a request for two songs fail at once.
        assert_eq!(EngineOptions::default().to_engine().max_batch, Some(1));
        assert_eq!(EngineOptions { max_batch: Some(3), ..EngineOptions::default() }.to_engine().max_batch, Some(3));
        // The assistant is optional: it must survive a restart when configured,
        // and stay unavailable when it is not.
        assert!(restored.assistant.available());
        assert!(!AssistantConfig::default().available());
    }

    #[test]
    fn capabilities_use_the_engines_envelope_and_only_advertise_catalog_verified_cloud_music() {
        let before_refresh = capability_engines(None, false);
        assert!(!before_refresh.iter().find(|engine| engine.id == "openrouter").unwrap().capabilities.contains(&Capability::MusicGeneration));
        let catalog = providers::openrouter::CapabilityCatalog::parse(r#"{"data":[{"id":"catalog/music","name":"Song","description":"Music generation for full-length songs","architecture":{"input_modalities":["text"],"output_modalities":["audio"]}}]}"#).unwrap();
        let after_refresh = CapabilitiesResponse { engines: capability_engines(Some(&catalog), false) };
        assert!(after_refresh.engines.iter().find(|engine| engine.id == "openrouter").unwrap().capabilities.contains(&Capability::MusicGeneration));
        // The music engine, two recognisers, the local assistant and
        // OpenRouter: everything listed is something the studio can actually do.
        assert_eq!(serde_json::to_value(after_refresh).unwrap()["engines"].as_array().unwrap().len(), 5);
    }
}
