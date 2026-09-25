//! Dataset preparation as one job on the server: the vocals separated, the
//! lyrics recognised and laid out, every song described by ear, and, when
//! asked, the training run started after. Each model loads once for the whole
//! batch and is let go when its stage ends, so the next one has the card:
//! the lyrics databases, then the separator and the recogniser for what they
//! do not know, then MOSS with the small tempo and key models beside it, then
//! the assistant for the lyrics.
//! A single song is the same job with one item.
//!
//! Every song keeps where it stands - lyrics wanted, found or done; style
//! wanted or done - and the job itself is kept on disk, so a restart
//! picks it up and each song does only what it still lacks.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{api_error, assistant, audio_facts, listen, lyrics_db, lyrics_sync, training, ApiError, AppState};

/// Which songs a step is done for.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Fill {
    /// Not at all.
    None,
    /// Only where the field is empty; what the user wrote stays.
    #[default]
    Missing,
    /// Every chosen song, written over.
    All,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrainAfter {
    #[serde(default)]
    pub name: String,
    pub recipe: training::Recipe,
}

#[derive(Debug, Deserialize)]
pub struct PrepareRequest {
    /// The songs to prepare; all of the dataset's when absent.
    #[serde(default)]
    pub items: Option<Vec<String>>,
    #[serde(default)]
    pub lyrics: Fill,
    #[serde(default)]
    pub style: Fill,
    /// The language sung, when the user knows it.
    #[serde(default)]
    pub language: Option<String>,
    /// Start this run once every song is ready.
    #[serde(default)]
    pub train: Option<TrainAfter>,
    /// Who lays the lyrics out: the studio's assistant, or an agent connected
    /// over MCP, which finds the songs left `found` and lays them out itself.
    /// The captions are MOSS's either way.
    #[serde(default)]
    pub writer: Writer,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Writer {
    #[default]
    Studio,
    Agent,
}

#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub item: String,
    pub step: &'static str,
    pub error: String,
}

/// The job as kept on disk: which songs, which steps. "Again" was turned
/// into song states when it started, so resuming it redoes nothing.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct Job {
    dataset: String,
    items: Option<Vec<String>>,
    lyrics: bool,
    style: bool,
    language: Option<String>,
    train: Option<TrainAfter>,
    #[serde(default)]
    writer: Writer,
}

/// What the job is doing, for the training page to show.
#[derive(Debug, Clone, Serialize)]
pub struct PrepareStatus {
    pub dataset: String,
    /// The stages at work now, the latest last.
    pub stages: Vec<StageStatus>,
    /// Songs still to be touched by any stage.
    pub pending: Vec<String>,
    pub failures: Vec<Failure>,
    /// Why a whole step was left out, as a message key the page translates.
    pub notices: Vec<&'static str>,
    pub finished: bool,
    pub cancelled: bool,
    /// The run started after, when one was asked for.
    pub run: Option<String>,
    /// What the captioner said about the device it ran on.
    pub device: Option<String>,
    /// A run starts once every song is ready.
    pub train_after: bool,
}

/// One stage at work: lookup, vocals, lyrics, listen, writing, training.
#[derive(Debug, Clone, Serialize)]
pub struct StageStatus {
    pub name: &'static str,
    pub done: usize,
    pub total: usize,
    /// The song the stage works on now.
    pub current: Option<String>,
    /// The songs of the stage, in the order it takes them.
    #[serde(skip)]
    order: Vec<String>,
}

pub type Shared = Arc<Mutex<Option<PrepareStatus>>>;
/// The run to start after, changeable while the job works.
pub type TrainSlot = Arc<Mutex<Option<TrainAfter>>>;

fn update(shared: &Shared, change: impl FnOnce(&mut PrepareStatus)) {
    if let Some(status) = shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
        change(status);
    }
}

fn stage(shared: &Shared, name: &'static str, order: Vec<String>) {
    update(shared, |status| {
        status.stages.retain(|stage| stage.name != name);
        status.stages.push(StageStatus { name, done: 0, total: order.len(), current: order.first().cloned(), order });
    });
}

/// `done` songs of the stage finished; the next one is the current.
fn progress(shared: &Shared, name: &'static str, done: impl FnOnce(usize) -> usize) {
    update(shared, |status| {
        if let Some(stage) = status.stages.iter_mut().find(|stage| stage.name == name) {
            stage.done = done(stage.done).min(stage.total);
            stage.current = stage.order.get(stage.done).cloned();
        }
    });
}

fn end_stage(shared: &Shared, name: &'static str) {
    update(shared, |status| status.stages.retain(|stage| stage.name != name));
}

/// Stores what a step made of a song; a song deleted meanwhile fails alone
/// and the job goes on with the others.
fn store(training: &training::Training, shared: &Shared, dataset: &str, item: &str, step: &'static str, patch: training::ItemPatch) {
    if let Err(error) = training.update_item(dataset, item, patch) {
        fail(shared, item, step, format!("{error:#}"));
    }
}

fn fail(shared: &Shared, item: &str, step: &'static str, error: impl std::fmt::Display) {
    let error = error.to_string();
    update(shared, |status| status.failures.push(Failure { item: item.into(), step, error }));
}

pub async fn start(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<PrepareRequest>,
) -> Result<Json<PrepareStatus>, (StatusCode, Json<ApiError>)> {
    if state.training.active_run().await.is_some() {
        return Err(api_error(StatusCode::CONFLICT, "a LoRA is training on the card; prepare songs once it finishes".into()));
    }
    if state.prepare.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref().is_some_and(|running| !running.finished) {
        return Err(api_error(StatusCode::CONFLICT, "songs are already being prepared; wait for them or stop it".into()));
    }
    // the agent writes after the preparation, so training cannot follow it
    if request.writer == Writer::Agent && request.train.is_some() {
        return Err(api_error(StatusCode::BAD_REQUEST, "with writer agent the songs are written after the preparation; start the training once they are, not with train".into()));
    }
    let dataset = state.training.dataset(&id).map_err(crate::training_error)?;
    let chosen: Vec<String> = dataset.items.iter().filter(|item| request.items.as_ref().is_none_or(|ids| ids.contains(&item.id))).map(|item| item.id.clone()).collect();
    if chosen.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "no songs to prepare".into()));
    }
    // "again" sends the songs back to the start of the step
    state.training.reset_states(&id, &chosen, request.lyrics == Fill::All, request.style == Fill::All).map_err(crate::training_error)?;
    let job = Job {
        dataset: id,
        items: request.items,
        lyrics: request.lyrics != Fill::None,
        style: request.style != Fill::None,
        language: request.language,
        train: request.train,
        writer: request.writer,
    };
    Ok(Json(launch(&state, job).map_err(crate::training_error)?))
}

/// Picks up the preparation a restart cut off, if there was one.
pub fn resume(state: &AppState) {
    let path = state.training.prepare_job_path();
    let Ok(bytes) = std::fs::read(&path) else { return };
    match serde_json::from_slice::<Job>(&bytes) {
        Ok(job) => {
            eprintln!("[OK] resuming the preparation of dataset {}", job.dataset);
            if let Err(error) = launch(state, job) {
                eprintln!("[ERROR] resuming the preparation: {error:#}");
            }
        }
        Err(error) => eprintln!("[ERROR] {} is not a preparation job: {error}", path.display()),
    }
}

fn launch(state: &AppState, job: Job) -> anyhow::Result<PrepareStatus> {
    let status = PrepareStatus {
        dataset: job.dataset.clone(),
        stages: Vec::new(),
        pending: Vec::new(),
        failures: Vec::new(),
        notices: Vec::new(),
        finished: false,
        cancelled: false,
        run: None,
        device: None,
        train_after: job.train.is_some(),
    };
    // kept on disk first: a job that cannot be kept is not started
    let path = state.training.prepare_job_path();
    std::fs::write(&path, serde_json::to_vec_pretty(&job)?).with_context(|| format!("write {}", path.display()))?;
    *state.prepare.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(status.clone());
    state.prepare_cancel.store(false, Ordering::Relaxed);
    *state.prepare_train.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = job.train.clone();
    let background = state.clone();
    tokio::spawn(async move {
        let shared = background.prepare.clone();
        let id = job.dataset.clone();
        let hooks = crate::card_hooks(&background).await;
        (hooks.take)().await;
        let outcome = run(&background, &job).await;
        (hooks.give_back)().await;
        let cancelled = background.prepare_cancel.load(Ordering::Relaxed);
        if let Err(error) = &outcome {
            if !cancelled {
                fail(&shared, "", "job", format!("{error:#}"));
            }
        }
        // finished, stopped or failed: nothing to pick up after a restart
        let _ = std::fs::remove_file(background.training.prepare_job_path());
        let clean = outcome.is_ok() && !cancelled && shared.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_some_and(|status| status.failures.is_empty());
        let train = background.prepare_train.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        if let (true, Writer::Studio, Some(train)) = (clean, job.writer, train) {
            stage(&shared, "training", Vec::new());
            match crate::start_training_run(&background, &id, &train.name, train.recipe).await {
                Ok(run) => update(&shared, |status| status.run = Some(run.id)),
                Err(error) => fail(&shared, "", "training", error),
            }
        }
        update(&shared, |status| {
            status.stages.clear();
            status.pending.clear();
            status.finished = true;
            status.cancelled = cancelled;
        });
    });
    Ok(status)
}

#[derive(Debug, Deserialize)]
pub struct TrainAfterRequest {
    #[serde(default)]
    pub train: Option<TrainAfter>,
}

/// Turns "train once ready" on or off for the job at work.
pub async fn set_train_after(State(state): State<AppState>, Json(request): Json<TrainAfterRequest>) -> Json<serde_json::Value> {
    let on = request.train.is_some();
    *state.prepare_train.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = request.train;
    update(&state.prepare, |status| status.train_after = on);
    Json(serde_json::json!({ "train_after": on }))
}

pub async fn cancel(State(state): State<AppState>) -> Json<serde_json::Value> {
    state.prepare_cancel.store(true, Ordering::Relaxed);
    Json(serde_json::json!({ "cancelled": true }))
}

/// The steps a job has passed, so a song whose next step is behind leaves
/// the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Passed {
    Nothing,
    Lyrics,
    Listening,
    Writing,
    Styles,
}

/// Whether a song still has a step ahead of it in this job.
fn has_work(item: &training::DatasetItem, job: &Job, can_listen: bool, passed: Passed) -> bool {
    use training::{LyricsState, StyleState};
    (job.lyrics && item.lyrics_state == LyricsState::Wanted && passed < Passed::Lyrics)
        || (job.lyrics && job.writer == Writer::Studio && item.lyrics_state == LyricsState::Found && passed < Passed::Writing)
        || (job.style && can_listen && item.style_state == StyleState::Wanted && passed < Passed::Listening)
        || (job.style && job.writer == Writer::Studio && item.style_state == StyleState::Heard && passed < Passed::Styles)
}

/// The queue: the songs of the job with a step still ahead, failures out.
fn settle_pending(training: &training::Training, job: &Job, shared: &Shared, can_listen: bool, passed: Passed) {
    let dataset = match training.dataset(&job.dataset) {
        Ok(dataset) => dataset,
        Err(error) => return fail(shared, "", "queue", format!("{error:#}")),
    };
    update(shared, |status| {
        let failed: Vec<String> = status.failures.iter().map(|failure| failure.item.clone()).collect();
        status.pending = dataset
            .items
            .iter()
            .filter(|item| job.items.as_ref().is_none_or(|ids| ids.contains(&item.id)))
            .filter(|item| !failed.contains(&item.id) && has_work(item, job, can_listen, passed))
            .map(|item| item.id.clone())
            .collect();
    });
}

/// The songs of the job, as they stand now.
fn songs(state: &AppState, job: &Job) -> anyhow::Result<Vec<training::DatasetItem>> {
    Ok(state.training.dataset(&job.dataset)?.items.into_iter().filter(|item| job.items.as_ref().is_none_or(|ids| ids.contains(&item.id))).collect())
}

async fn run(state: &AppState, job: &Job) -> anyhow::Result<()> {
    use training::{LyricsState, StyleState};
    let shared = state.prepare.clone();
    let cancel = state.prepare_cancel.clone();
    let stopped = || cancel.load(Ordering::Relaxed);
    let can_listen = state.training.listen_ready() && state.lyrics_sync.onnxruntime_library().is_some();
    let items = songs(state, job)?;
    if job.style && !can_listen && items.iter().any(|item| item.style_state == StyleState::Wanted) {
        update(&shared, |status| status.notices.push("listen_missing"));
    }
    settle_pending(&state.training, job, &shared, can_listen, Passed::Nothing);

    // The ONNX Runtime the card can use, before anything binds another
    if let Some(runtime) = crate::preferred_onnx_runtime(state) {
        crate::point_ort_at(&runtime);
    }

    // One model on the card at a time: Whisper, then MOSS, then the assistant
    let lyric_items: Vec<&training::DatasetItem> = items.iter().filter(|item| job.lyrics && item.lyrics_state == LyricsState::Wanted).collect();
    let timed = lyrics_branch(state, job, &lyric_items).await?;
    if stopped() {
        return Ok(());
    }
    settle_pending(&state.training, job, &shared, can_listen, Passed::Lyrics);
    if can_listen && job.style {
        let style_items: Vec<training::DatasetItem> = songs(state, job)?.into_iter().filter(|item| item.style_state == StyleState::Wanted).collect();
        listen_branch(state, job, &style_items.iter().collect::<Vec<_>>(), can_listen).await?;
        if stopped() {
            return Ok(());
        }
    }
    settle_pending(&state.training, job, &shared, can_listen, Passed::Listening);
    // an agent lays out what is left found itself
    if job.writer == Writer::Agent {
        return Ok(());
    }
    let outcome = async {
        if job.lyrics {
            write_lyrics(state, job, &timed, can_listen).await?;
        }
        anyhow::Ok(())
    }
    .await;
    crate::release_assistant_unless_kept(state).await;
    outcome
}

/// The lyrics: the databases first, then vocals and Whisper for what none of
/// them knows. Each song is stored as found the moment its words are there;
/// gives the timed text of each, which lays out better than the plain lines.
async fn lyrics_branch(state: &AppState, job: &Job, lyric_items: &[&training::DatasetItem]) -> anyhow::Result<HashMap<String, String>> {
    use training::LyricsState;
    let id = job.dataset.as_str();
    let shared = state.prepare.clone();
    let cancel = state.prepare_cancel.clone();
    let stopped = || cancel.load(Ordering::Relaxed);
    // Lyrics: the databases music players read them from first; only what
    // none of them knows has its vocals separated and is heard by Whisper
    let mut sheets: Vec<(String, String)> = Vec::new();
    let mut lyric_items = lyric_items.to_vec();
    let lookup: Vec<&training::DatasetItem> = lyric_items.iter().copied().filter(|item| !item.source.starts_with("song:")).collect();
    if !lookup.is_empty() {
        stage(&shared, "lookup", lookup.iter().map(|item| item.id.clone()).collect());
        let sources = Arc::new(lyrics_db::Sources::new()?);
        let fallback_artist = state.training.dataset(id).map(|dataset| dataset.name).unwrap_or_default();
        let songs: Vec<(String, lyrics_db::Song)> = lookup
            .iter()
            .map(|item| {
                let artist = if item.artist.trim().is_empty() { fallback_artist.clone() } else { item.artist.clone() };
                (item.id.clone(), lyrics_db::Song { artist, title: item.title.clone(), seconds: item.seconds })
            })
            .collect();
        // a song with no artist of its own was looked up under the dataset's
        // name; a sheet matching title, artist and length confirms that name
        let unnamed: HashMap<String, String> = lookup
            .iter()
            .filter(|item| item.artist.trim().is_empty() && !fallback_artist.trim().is_empty())
            .map(|item| (item.id.clone(), fallback_artist.clone()))
            .collect();
        // each answer is stored the moment it comes back
        let mut known: Vec<String> = Vec::new();
        let mut unreachable = 0;
        let mut asking = tokio::task::JoinSet::new();
        let mut waiting = songs.into_iter();
        loop {
            while asking.len() < 4 {
                let Some((item, song)) = waiting.next() else { break };
                let (sources, cancel) = (sources.clone(), cancel.clone());
                asking.spawn(async move {
                    let mut failed = Vec::new();
                    let found = if cancel.load(Ordering::Relaxed) { None } else { sources.find(&song, &mut failed).await };
                    (item, found, failed)
                });
            }
            let Some(answer) = asking.join_next().await else { break };
            let (item, found, failed) = answer?;
            progress(&shared, "lookup", |done| done + 1);
            if failed.len() == lyrics_db::SOURCES {
                unreachable += 1;
            }
            match found {
                Some(lyrics_db::Found::Lyrics { plain, timed, source }) => {
                    let found = training::ItemPatch {
                        lyrics: Some(plain),
                        instrumental: Some(false),
                        lyrics_source: Some(source.into()),
                        lyrics_state: Some(LyricsState::Found),
                        artist: unnamed.get(&item).cloned(),
                        ..Default::default()
                    };
                    store(&state.training, &shared, id, &item, "lyrics", found);
                    sheets.push((item.clone(), timed));
                    known.push(item);
                }
                Some(lyrics_db::Found::Instrumental { source }) => {
                    let instrumental = training::ItemPatch { lyrics: Some(String::new()), instrumental: Some(true), lyrics_source: Some(source.into()), lyrics_state: Some(LyricsState::Done), ..Default::default() };
                    store(&state.training, &shared, id, &item, "lyrics", instrumental);
                    known.push(item);
                }
                None => {}
            }
        }
        if stopped() {
            return Ok(HashMap::new());
        }
        if unreachable > 0 && unreachable == lookup.len() {
            update(&shared, |status| status.notices.push("lyrics_db_unreachable"));
        }
        lyric_items.retain(|item| !known.contains(&item.id));
    }

    // Whatever the databases did not know: the recogniser chosen under Settings - Karaoke,
    // told the language the found lyrics are in, since it guesses stylised
    // Russian as Ukrainian or Belarusian
    let language = job.language.clone().or_else(|| sung_language(sheets.iter().map(|(_, text)| text.as_str())).map(str::to_string));
    let mut transcripts: Vec<(String, String)> = Vec::new();
    if !lyric_items.is_empty() {
        let config = state.lyrics_sync_config.read().await.clone();
        if !config.available() || matches!(config.provider, lyrics_sync::AsrProvider::None) {
            update(&shared, |status| status.notices.push("recogniser_missing"));
        } else if !crate::ensure_local_recogniser(state, &config, id).await {
            for item in &lyric_items {
                fail(&shared, &item.id, "lyrics", "the speech recogniser could not be downloaded");
            }
        } else {
            // The vocals alone recognise far better than the mix; they are kept
            // with the dataset, where lyric timing reads them too.
            let mut heard: Vec<(String, PathBuf)> = Vec::new();
            let mut to_separate: Vec<(String, PathBuf, PathBuf)> = Vec::new();
            for item in &lyric_items {
                let audio = state.training.item_audio(id, &item.id)?;
                let vocals = state.training.item_vocals(id, &item.id)?;
                if vocals.is_file() {
                    heard.push((item.id.clone(), vocals));
                } else {
                    to_separate.push((item.id.clone(), audio, vocals));
                }
            }
            match crate::vocal_separator(state).await {
                Some(separator) if !to_separate.is_empty() => {
                    // the processor was chosen for separation on a machine with a card that runs it
                    if matches!(state.separation_config.read().await.runtime, lyrics_sync::OnnxFlavour::Cpu) && crate::cuda_build::current().is_some() {
                        update(&shared, |status| status.notices.push("separator_on_cpu"));
                    }
                    stage(&shared, "vocals", to_separate.iter().map(|(item, ..)| item.clone()).collect());
                    let (task_shared, task_cancel) = (shared.clone(), cancel.clone());
                    let separated = tokio::task::spawn_blocking(move || {
                        let mut separated = Vec::new();
                        for (item, audio, vocals) in to_separate {
                            if task_cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            let outcome = (|| -> anyhow::Result<()> {
                                let folder = vocals.parent().context("vocals folder")?;
                                std::fs::create_dir_all(folder)?;
                                let partial = folder.join(format!("vocals.{}.part.wav", uuid::Uuid::now_v7().simple()));
                                separator.separate(&audio, &partial)?;
                                std::fs::rename(&partial, &vocals)?;
                                Ok(())
                            })();
                            match outcome {
                                // a failed separation still leaves the mix to recognise
                                Ok(()) => separated.push((item, vocals)),
                                Err(error) => {
                                    eprintln!("[ERROR] separating the vocals of {item}: {error:#}");
                                    separated.push((item, audio));
                                }
                            }
                            progress(&task_shared, "vocals", |done| done + 1);
                        }
                        separated
                    })
                    .await?;
                    heard.extend(separated);
                }
                _ => {
                    for (item, audio, _) in to_separate {
                        heard.push((item, audio));
                    }
                }
            }
            if stopped() {
                return Ok(HashMap::new());
            }

            stage(&shared, "lyrics", heard.iter().map(|(item, _)| item.clone()).collect());
            // Each song's words are kept the moment they are heard: the plain
            // lines go into the song at once, the layout in sections comes later
            let found: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
            let items: Vec<String> = heard.iter().map(|(item, _)| item.clone()).collect();
            let keep = {
                let (training, shared, found, id) = (state.training.clone(), shared.clone(), found.clone(), id.to_string());
                move |index: usize, words: anyhow::Result<Vec<(f64, String)>>| {
                    let item = &items[index];
                    progress(&shared, "lyrics", |done| done + 1);
                    match words.map_err(|error| format!("{error:#}")).and_then(|words| transcript(&words).ok_or_else(|| lyrics_sync::NO_WORDS.to_string())) {
                        Ok(text) => {
                            let plain = text.lines().map(|line| line.split_once("] ").map_or(line, |(_, words)| words)).collect::<Vec<_>>().join("\n");
                            let heard = training::ItemPatch { lyrics: Some(plain), instrumental: Some(false), lyrics_source: Some("recognised".into()), lyrics_state: Some(LyricsState::Found), ..Default::default() };
                            if let Err(error) = training.update_item(&id, item, heard) {
                                fail(&shared, item, "lyrics", format!("{error:#}"));
                            }
                            found.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push((item.clone(), text));
                        }
                        // no words at all: the song is an instrumental
                        Err(error) if error.contains(lyrics_sync::NO_WORDS) => {
                            let instrumental = training::ItemPatch { instrumental: Some(true), lyrics: Some(String::new()), lyrics_source: Some("recognised".into()), lyrics_state: Some(LyricsState::Done), ..Default::default() };
                            if let Err(error) = training.update_item(&id, item, instrumental) {
                                fail(&shared, item, "lyrics", format!("{error:#}"));
                            }
                        }
                        Err(error) => fail(&shared, item, "lyrics", error),
                    }
                }
            };
            match config.provider {
                lyrics_sync::AsrProvider::OpenRouter => {
                    for (index, (_, path)) in heard.iter().enumerate() {
                        if stopped() {
                            return Ok(HashMap::new());
                        }
                        keep(index, crate::karaoke_words_from_openrouter(state, &config, &path.to_string_lossy(), language.as_deref()).await);
                    }
                }
                _ => {
                    let (sync, task_cancel) = (state.lyrics_sync.clone(), cancel.clone());
                    let paths: Vec<PathBuf> = heard.iter().map(|(_, path)| path.clone()).collect();
                    let language = language.clone();
                    let mut keep = keep;
                    let recognised = tokio::task::spawn_blocking(move || sync.words_many(&config, &paths, language.as_deref(), &mut keep, &task_cancel)).await?;
                    if recognised == lyrics_sync::Recognised::OnProcessor {
                        update(&shared, |status| status.notices.push("recogniser_on_cpu"));
                    }
                }
            }
            if stopped() {
                return Ok(HashMap::new());
            }
            transcripts = std::mem::take(&mut *found.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
        }
    }
    end_stage(&shared, "lookup");
    end_stage(&shared, "vocals");
    end_stage(&shared, "lyrics");
    Ok(sheets.into_iter().chain(transcripts).collect())
}

/// The listening: MOSS over every song in one load, tempo and key measured
/// beside it. Each song's style is stored the moment it is heard.
async fn listen_branch(state: &AppState, job: &Job, style_items: &[&training::DatasetItem], can_listen: bool) -> anyhow::Result<()> {
    let id = job.dataset.as_str();
    let shared = state.prepare.clone();
    let cancel = state.prepare_cancel.clone();
    let stopped = || cancel.load(Ordering::Relaxed);
    if !style_items.is_empty() {
        stage(&shared, "listen", style_items.iter().map(|item| item.id.clone()).collect());
        crate::release_assistant_unless_kept(state).await;
        // The captioner is a CUDA 13 build and imports cuBLAS 13 from beside
        // the engine; on a card that build does not run, it runs on the processor
        if crate::cuda_build::current() == Some(crate::cuda_build::CudaBuild::Cuda13) {
            state.engine_runtime.install_missing(crate::cuda_build::CudaBuild::Cuda13).await.context("install cuBLAS for the captioner")?;
        } else {
            update(&shared, |status| status.notices.push("listen_on_cpu"));
        }
        let (captioner, moss, facts_dir) = (state.training.captioner(), state.training.moss_dir(), state.training.audio_facts_dir());
        let ids: Vec<String> = style_items.iter().map(|item| item.id.clone()).collect();
        let audio: Vec<PathBuf> = ids.iter().map(|item| state.training.item_audio(id, item)).collect::<anyhow::Result<_>>()?;
        // tempo and key are measured ahead of MOSS: four threads decode, Beat
        // This! and S-KEY - loaded once, on the card - measure. Each song's
        // description is stored the moment MOSS has heard it
        let facts: Arc<Mutex<Vec<Option<Result<audio_facts::Facts, String>>>>> = Arc::new(Mutex::new(vec![None; audio.len()]));
        let (decoded_tx, decoded) = std::sync::mpsc::sync_channel::<(usize, anyhow::Result<Vec<f32>>)>(8);
        let mut measuring: Vec<std::thread::JoinHandle<()>> = (0..4)
            .map(|lane| {
                let (audio, decoded_tx) = (audio.clone(), decoded_tx.clone());
                std::thread::spawn(move || {
                    for (index, path) in audio.iter().enumerate().skip(lane).step_by(4) {
                        if decoded_tx.send((index, audio_facts::decode(path))).is_err() {
                            return;
                        }
                    }
                })
            })
            .collect();
        drop(decoded_tx);
        // set when the measurer ends, by panic too, so no song waits on a
        // measurement that will never come
        let measured_all = Arc::new(AtomicBool::new(false));
        {
            let (facts, facts_dir, facts_shared, finished) = (facts.clone(), facts_dir.clone(), shared.clone(), measured_all.clone());
            measuring.push(std::thread::spawn(move || {
                struct Finished(Arc<AtomicBool>);
                impl Drop for Finished {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::Relaxed);
                    }
                }
                let _finished = Finished(finished);
                let mut measurer = audio_facts::Measurer::load(&facts_dir, true).map_err(|error| format!("{error:#}"));
                if matches!(&measurer, Ok(measurer) if !measurer.on_gpu) {
                    update(&facts_shared, |status| status.notices.push("facts_on_cpu"));
                }
                for (index, mono) in decoded {
                    let measured = match (&mut measurer, mono) {
                        (Ok(measurer), Ok(mono)) => measurer.measure_mono(&mono).map_err(|error| format!("{error:#}")),
                        (Err(error), _) => Err(error.clone()),
                        (_, Err(error)) => Err(format!("{error:#}")),
                    };
                    facts.lock().unwrap_or_else(|poisoned| poisoned.into_inner())[index] = Some(measured);
                }
            }));
        }
        let (task_shared, task_cancel, task_facts, task_measured_all) = (shared.clone(), cancel.clone(), facts.clone(), measured_all.clone());
        let (training, dataset_id, task_ids, task_job) = (state.training.clone(), id.to_string(), ids.clone(), job.clone());
        let libraries = crate::engine_bundle_root();
        let log = tokio::task::spawn_blocking(move || {
            listen::hear_batch(&captioner, &moss, Some(&libraries), &audio, &task_cancel, |index, caption| {
                let item = &task_ids[index];
                let measured = loop {
                    if let Some(measured) = task_facts.lock().unwrap_or_else(|poisoned| poisoned.into_inner())[index].clone() {
                        break measured;
                    }
                    if task_cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    if task_measured_all.load(Ordering::Relaxed) && task_facts.lock().unwrap_or_else(|poisoned| poisoned.into_inner())[index].is_none() {
                        break Err("the measurer stopped before this song".to_string());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                };
                // MiniMax trains on the structured caption MOSS writes itself;
                // only its numbers are replaced with the measured ones
                let outcome = measured.map_err(|error| anyhow::anyhow!("measure tempo and key: {error}")).and_then(|facts| Ok(listen::heard(&caption?, &facts)));
                match outcome {
                    Ok(heard) => {
                        let done = training::ItemPatch { style: Some(heard.mm3), style_state: Some(training::StyleState::Done), ..Default::default() };
                        if let Err(error) = training.update_item(&dataset_id, item, done) {
                            fail(&task_shared, item, "style", format!("{error:#}"));
                        }
                    }
                    Err(error) => fail(&task_shared, item, "style", format!("{error:#}")),
                }
                progress(&task_shared, "listen", |done| done + 1);
                settle_pending(&training, &task_job, &task_shared, can_listen, Passed::Lyrics);
            })
        })
        .await?;
        for lane in measuring {
            lane.join().map_err(|_| anyhow::anyhow!("measuring tempo and key failed"))?;
        }
        if stopped() {
            return Ok(());
        }
        let log = log?;
        let device = log.iter().find(|line| line.contains("CUDA") || line.contains("Vulkan") || line.to_lowercase().contains("backend")).cloned();
        update(&shared, |status| status.device = device);
    }
    end_stage(&shared, "listen");
    Ok(())
}


/// Every found text laid out in sections by the assistant, each song stored
/// as done when its layout is: a published sheet keeps its words, a
/// recognised transcript has its mishearings fixed.
async fn write_lyrics(state: &AppState, job: &Job, timed: &HashMap<String, String>, can_listen: bool) -> anyhow::Result<()> {
    use training::LyricsState;
    let (shared, cancel) = (state.prepare.clone(), state.prepare_cancel.clone());
    let found: Vec<training::DatasetItem> = songs(state, job)?.into_iter().filter(|item| item.lyrics_state == LyricsState::Found).collect();
    if found.is_empty() {
        return Ok(());
    }
    stage(&shared, "writing", found.iter().map(|item| item.id.clone()).collect());
    for item in &found {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let text = timed.get(&item.id).unwrap_or(&item.lyrics);
        let target = if item.lyrics_source == "recognised" { assistant::AssistTarget::Transcript } else { assistant::AssistTarget::Sheet };
        match lay_out_lyrics(state, text, target).await {
            Ok(lyrics) => {
                let done = training::ItemPatch { lyrics: Some(lyrics), instrumental: Some(false), lyrics_state: Some(LyricsState::Done), ..Default::default() };
                store(&state.training, &shared, &job.dataset, &item.id, "lyrics", done);
            }
            Err(error) => fail(&shared, &item.id, "lyrics", error),
        }
        progress(&shared, "writing", |done| done + 1);
        settle_pending(&state.training, job, &shared, can_listen, Passed::Listening);
    }
    end_stage(&shared, "writing");
    Ok(())
}

/// The language of Cyrillic lyrics, told by the letters only one of them
/// has: і ї є ґ Ukrainian, ў Belarusian, ы э ъ Russian. None for other
/// alphabets, which Whisper tells apart well enough itself.
fn sung_language<'a>(texts: impl Iterator<Item = &'a str>) -> Option<&'static str> {
    let (mut cyrillic, mut latin, mut russian, mut ukrainian, mut belarusian) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for c in texts.flat_map(str::chars).flat_map(char::to_lowercase) {
        match c {
            'ы' | 'э' | 'ъ' => russian += 1,
            'і' | 'ї' | 'є' | 'ґ' => ukrainian += 1,
            'ў' => belarusian += 1,
            _ => {}
        }
        if ('\u{0400}'..='\u{04FF}').contains(&c) {
            cyrillic += 1;
        } else if c.is_ascii_alphabetic() {
            latin += 1;
        }
    }
    if cyrillic < 200 || cyrillic < latin * 3 {
        return None;
    }
    Some(if belarusian > russian { "be" } else if ukrainian > russian { "uk" } else { "ru" })
}

/// The recognised words as timed lines: punctuation the recogniser hangs at
/// the start of a line belongs to the line before, and an unknown-token
/// marker is not a word. None when nothing is left.
fn transcript(words: &[(f64, String)]) -> Option<String> {
    let mut lines: Vec<(f64, String)> = Vec::new();
    for (time, text) in lyrics_sync::group_words(words) {
        let text = text.replace("<unk>", "");
        let body = text.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | '!' | '?' | ';' | ':'));
        let lead = text.trim_start()[..text.trim_start().len() - body.len()].trim();
        if let (false, Some(last)) = (lead.is_empty(), lines.last_mut()) {
            last.1.push_str(lead);
        }
        if !body.trim().is_empty() {
            lines.push((time, body.trim().to_string()));
        }
    }
    (!lines.is_empty()).then(|| {
        lines
            .iter()
            .map(|(time, text)| format!("[{}:{:02}] {}", (*time as u64) / 60, (*time as u64) % 60, text.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// A timed transcript line without its "[m:ss] " time; any other bracket at
/// the start of a line is the singer's and stays.
fn without_time(line: &str) -> &str {
    match line.strip_prefix('[').and_then(|rest| rest.split_once("] ")) {
        Some((time, words)) if !time.is_empty() && time.chars().all(|c| c.is_ascii_digit() || c == ':' || c == '.') => words,
        _ => line,
    }
}

/// The transcript laid out in tagged sections by the assistant. A small model
/// can answer a Cyrillic transcript in Latin letters; that is not the song, so
/// it is asked once more and then refused, never stored.
pub(crate) async fn lay_out_lyrics(state: &AppState, transcript: &str, target: assistant::AssistTarget) -> Result<String, String> {
    let request = assistant::AssistRequest {
        target,
        description: transcript.to_string(),
        instruction: String::new(),
        lyrics: String::new(),
        global_metadata: String::new(),
        vocal_details: String::new(),
        arrangement: String::new(),
        duration_seconds: 0.0,
        instrumental: false,
    };
    // a published sheet's words are exact: the assistant marks where its
    // sections start, and the sheet's own lines go under the tags
    if target == assistant::AssistTarget::Sheet {
        let sheet: Vec<&str> = transcript
            .lines()
            .map(|line| without_time(line).trim())
            .filter(|line| !line.is_empty())
            .collect();
        for _ in 0..2 {
            let answer = crate::assistant_ask(state, assistant::SHEET_SECTIONS_PROMPT, &assistant::numbered_lines(&sheet), Some(assistant::sheet_sections_schema()), target)
                .await
                .map_err(|(_, Json(error))| error.error)?;
            if let Some(laid_out) = assistant::sheet_in_sections(&answer, &sheet) {
                return Ok(laid_out);
            }
        }
        return Err("the assistant marked no sections in the published lyrics twice; lay out the sections by hand".into());
    }
    let heard_cyrillic = assistant::cyrillic_share(&request.description) > 0.5;
    let mut problem = "returned no lyrics twice";
    for _ in 0..2 {
        let draft = crate::assistant_draft(state, &request).await.map_err(|(_, Json(error))| error.error)?;
        let lyrics = assistant::cyrillic_homoglyphs(draft.get("lyrics").and_then(serde_json::Value::as_str).unwrap_or_default().trim());
        if lyrics.is_empty() {
            continue;
        }
        if heard_cyrillic && assistant::cyrillic_share(&lyrics) <= 0.5 {
            problem = "rewrote the lyrics in Latin letters twice";
            continue;
        }
        return Ok(lyrics);
    }
    Err(format!("the assistant {problem}; choose a larger assistant model or lay out the sections by hand"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_time_leaves_the_start_of_a_line() {
        assert_eq!(without_time("[1:05] Среди связок"), "Среди связок");
        assert_eq!(without_time("[Смех] ха-ха"), "[Смех] ха-ха");
        assert_eq!(without_time("[x] в горле"), "[x] в горле");
    }

    #[test]
    fn the_language_comes_from_the_letters() {
        let russian = "Ночью в лесу холодно-темно, и горит впотьмах лишь мое окно. ".repeat(10);
        assert_eq!(sung_language([russian.as_str()].into_iter()), Some("ru"));
        let ukrainian = "На Русі зима дуже люта, і в моєму лісі тим паче, їжак іде. ".repeat(10);
        assert_eq!(sung_language([ukrainian.as_str()].into_iter()), Some("uk"));
        assert_eq!(sung_language(["Thanks for all the fish ".repeat(20).as_str()].into_iter()), None);
    }

    fn song(lyrics: training::LyricsState, style: training::StyleState) -> training::DatasetItem {
        training::DatasetItem {
            id: "a".into(),
            title: String::new(),
            artist: String::new(),
            lyrics_source: String::new(),
            style: String::new(),
            lyrics: String::new(),
            instrumental: false,
            file: String::new(),
            seconds: 0.0,
            source: String::new(),
            lyrics_state: lyrics,
            style_state: style,
            heard: None,
        }
    }

    #[test]
    fn a_song_leaves_the_queue_once_its_steps_are_behind() {
        use training::{LyricsState as L, StyleState as S};
        let job = Job { dataset: "d".into(), items: None, lyrics: true, style: true, language: None, train: None, writer: Writer::Studio };
        // found lyrics wait for the writing, whatever else is done
        assert!(has_work(&song(L::Found, S::Done), &job, true, Passed::Listening));
        assert!(!has_work(&song(L::Found, S::Done), &job, true, Passed::Writing));
        // a song the recogniser missed has nothing ahead once the lyrics step passed
        assert!(!has_work(&song(L::Wanted, S::Done), &job, true, Passed::Lyrics));
        // with no listening model a wanted style is no work
        assert!(!has_work(&song(L::Done, S::Wanted), &job, false, Passed::Nothing));
        // a heard song, cut off before its style was written, is picked up
        assert!(has_work(&song(L::Done, S::Heard), &job, false, Passed::Writing));
        // a lyrics-only job leaves the style alone
        let lyrics_only = Job { style: false, ..job.clone() };
        assert!(!has_work(&song(L::Done, S::Heard), &lyrics_only, true, Passed::Nothing));
    }

    #[test]
    fn transcript_moves_leading_punctuation_back() {
        let words = vec![(0.0, "hello".to_string()), (0.3, "there".to_string()), (2.0, ", again".to_string())];
        let text = transcript(&words).expect("words");
        assert!(text.starts_with("[0:00] hello there"), "{text}");
        assert!(transcript(&[(0.0, "<unk>".to_string())]).is_none());
    }
}
