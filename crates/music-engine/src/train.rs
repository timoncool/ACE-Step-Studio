//! How ACE-Step DiT adapters are trained: the stages the trainer runs, the
//! recipe it takes and what it prints.
//!
//! The trainer is HOT-Step's `ace-train`, built from a pinned commit and
//! shipped as `music-train`. `preprocess` encodes every song with the
//! studio's own VAE, text encoder and DiT condition encoder into a tensor
//! cache; `train-dit` trains a LoRA or LoKr on the DiT from it and exports
//! the adapter the engine loads (a PEFT folder, or `lokr_weights.safetensors`).
//!
//! A dataset reaches the trainer as a Side-Step `dataset.json` beside the
//! songs, each with its caption, genre, lyrics and metadata; the trigger word
//! is the dataset's `custom_tag`, put in front of every caption.
//!
//! The defaults are HOT-Step's Training Studio's (`TRAIN_DIT_DEFAULTS`,
//! `TRAIN_DIT_LOKR_DEFAULTS` at 8a5e42c4): flow-SNR loss, rank 128 / alpha 256
//! with the MLP trained, Prodigy, fused flash attention with the crop fitted
//! to the card.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The rate the trainer resamples to; the studio writes the songs at it.
pub const SAMPLE_RATE: u32 = 48_000;

/// Video memory the smallest configuration the trainer fits needs, in GB:
/// the XL base mirrored in bf16 is 8.1 GB, and the author measured full-depth
/// training on a card with 12 GB free.
pub const MIN_VRAM_GB: u32 = 12;

/// Weights the trainer needs beyond the studio's own models: none, it trains
/// on the DiT, VAE and text encoder the studio renders with.
pub struct TrainingFile {
    pub id: &'static str,
    pub label: &'static str,
    pub file: &'static str,
    pub bytes: u64,
}

pub const TRAINING_FILES: &[TrainingFile] = &[];

pub fn training_file_url(file: &TrainingFile) -> String {
    file.file.to_string()
}

/// What a run is asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recipe {
    /// Passes over the songs; the run stops earlier once the smoothed loss
    /// reaches `target_loss`, and the best adapter is what it leaves.
    pub epochs: u32,
    pub target_loss: f64,
    pub seed: u32,
    /// `lora` or `lokr`.
    pub adapter: String,
    pub rank: u32,
    pub alpha: f64,
    pub lokr_dim: u32,
    pub lokr_alpha: f64,
    pub lokr_factor: u32,
    /// Train the feed-forward blocks too, not only attention.
    pub target_mlp: bool,
    /// `prodigy` (finds its own step size), `adamw` or `muon`.
    pub optimizer: String,
    pub learning_rate: f64,
    pub grad_accum: u32,
    /// Percent of steps that train on the genre tags instead of the caption,
    /// so the adapter answers a short prompt too.
    pub genre_ratio: u32,
    /// Share of steps with the caption dropped, which classifier-free
    /// guidance needs at generation.
    pub cfg_ratio: f64,
    /// `flash` (fused, attention memory linear in the crop) or `exact`.
    pub attention: String,
    /// Steps one epoch takes; filled in from the trainer's own report.
    pub steps: u32,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            epochs: 500,
            target_loss: 0.3,
            seed: 42,
            adapter: "lora".into(),
            rank: 128,
            alpha: 256.0,
            lokr_dim: 512,
            lokr_alpha: 512.0,
            lokr_factor: 6,
            target_mlp: true,
            optimizer: "prodigy".into(),
            learning_rate: 5e-4,
            grad_accum: 4,
            genre_ratio: 30,
            cfg_ratio: 0.15,
            attention: "flash".into(),
            steps: 0,
        }
    }
}

impl Recipe {
    /// The recipe for a dataset of `songs`: nothing depends on the count
    /// before the trainer reports its steps.
    pub fn for_songs(&self, _songs: usize) -> Recipe {
        self.clone()
    }

    /// Refuses what the trainer would refuse, before any stage starts.
    pub fn check(&self) -> Result<(), String> {
        if self.epochs == 0 || self.grad_accum == 0 {
            return Err("epochs and gradient accumulation must be at least 1".into());
        }
        if !matches!(self.adapter.as_str(), "lora" | "lokr") {
            return Err(format!("unknown adapter {}", self.adapter));
        }
        if !matches!(self.optimizer.as_str(), "prodigy" | "adamw" | "muon") {
            return Err(format!("unknown optimizer {}", self.optimizer));
        }
        if !matches!(self.attention.as_str(), "flash" | "exact") {
            return Err(format!("unknown attention {}", self.attention));
        }
        if self.rank == 0 || self.lokr_dim == 0 || self.lokr_factor == 0 {
            return Err("the rank, the LoKr dimension and its factor must be at least 1".into());
        }
        let numbers = [self.alpha, self.lokr_alpha, self.learning_rate, self.target_loss, self.cfg_ratio];
        if !numbers.iter().all(|value| value.is_finite()) || self.alpha <= 0.0 || self.lokr_alpha <= 0.0 || self.learning_rate <= 0.0 {
            return Err("a recipe number is out of range".into());
        }
        if self.genre_ratio > 100 || !(0.0..1.0).contains(&self.cfg_ratio) || self.target_loss < 0.0 {
            return Err("the genre share is a percent and the dropped-caption share below 1".into());
        }
        Ok(())
    }
}

/// How the training page shows one setting of a recipe. The page renders
/// whatever an engine lists; labels and hints are `trainingField_<key>` and
/// `trainingHint_<key>`.
#[derive(Debug, Clone, Serialize)]
pub struct RecipeField {
    pub key: &'static str,
    /// Fields sharing a group are shown together, under `trainingGroup_<group>`.
    pub group: &'static str,
    pub kind: FieldKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub choices: &'static [&'static str],
    /// Shown only while another field holds one of these values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown_when: Option<FieldCondition>,
    /// Greyed out, with `trainingHint_<key>_off`, while another field holds one of these values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub off_when: Option<FieldCondition>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Number,
    Integer,
    Choice,
    Toggle,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct FieldCondition {
    pub field: &'static str,
    pub values: &'static [&'static str],
}

const fn number(key: &'static str, group: &'static str, min: f64, max: f64, step: f64) -> RecipeField {
    RecipeField { key, group, kind: FieldKind::Number, min: Some(min), max: Some(max), step: Some(step), choices: &[], shown_when: None, off_when: None }
}

const fn integer(key: &'static str, group: &'static str, min: f64, max: f64, step: f64) -> RecipeField {
    RecipeField { key, group, kind: FieldKind::Integer, min: Some(min), max: Some(max), step: Some(step), choices: &[], shown_when: None, off_when: None }
}

const fn choice(key: &'static str, group: &'static str, choices: &'static [&'static str]) -> RecipeField {
    RecipeField { key, group, kind: FieldKind::Choice, min: None, max: None, step: None, choices, shown_when: None, off_when: None }
}

const fn toggle(key: &'static str, group: &'static str) -> RecipeField {
    RecipeField { key, group, kind: FieldKind::Toggle, min: None, max: None, step: None, choices: &[], shown_when: None, off_when: None }
}

const LORA: FieldCondition = FieldCondition { field: "adapter", values: &["lora"] };
const LOKR: FieldCondition = FieldCondition { field: "adapter", values: &["lokr"] };

/// The settings of an ACE-Step run, in the order the page shows them.
pub fn recipe_fields() -> Vec<RecipeField> {
    vec![
        integer("epochs", "stop", 1.0, 2000.0, 10.0),
        number("target_loss", "stop", 0.0, 2.0, 0.05),
        integer("seed", "stop", 0.0, 4_294_967_295.0, 1.0),
        choice("adapter", "adapter", &["lora", "lokr"]),
        RecipeField { shown_when: Some(LORA), ..integer("rank", "adapter", 1.0, 512.0, 8.0) },
        RecipeField { shown_when: Some(LORA), ..number("alpha", "adapter", 1.0, 1024.0, 8.0) },
        RecipeField { shown_when: Some(LOKR), ..integer("lokr_dim", "adapter", 1.0, 4096.0, 64.0) },
        RecipeField { shown_when: Some(LOKR), ..number("lokr_alpha", "adapter", 1.0, 4096.0, 64.0) },
        RecipeField { shown_when: Some(LOKR), ..integer("lokr_factor", "adapter", 1.0, 64.0, 1.0) },
        toggle("target_mlp", "adapter"),
        choice("optimizer", "optimizer", &["prodigy", "adamw", "muon"]),
        RecipeField { off_when: Some(FieldCondition { field: "optimizer", values: &["prodigy"] }), ..number("learning_rate", "optimizer", 0.0, 0.05, 0.0001) },
        integer("grad_accum", "optimizer", 1.0, 64.0, 1.0),
        integer("genre_ratio", "data", 0.0, 100.0, 5.0),
        number("cfg_ratio", "data", 0.0, 0.5, 0.05),
        choice("attention", "window", &["flash", "exact"]),
    ]
}

/// The studio's models a run encodes and trains with.
#[derive(Debug, Clone)]
pub struct TrainingBase {
    /// The folder the engine reads its models from.
    pub models: PathBuf,
    pub dit: String,
    pub vae: String,
    pub text_encoder: String,
}

/// What a run is asked to do.
#[derive(Debug, Clone)]
pub struct TrainingInputs {
    /// `dataset.json` and the WAV files, written by the studio.
    pub data: PathBuf,
    pub base: TrainingBase,
    /// The run's own folder; every stage writes below it.
    pub run: PathBuf,
    pub recipe: Recipe,
}

/// One trainer invocation.
#[derive(Debug, Clone)]
pub struct TrainingStage {
    /// The stage's name for the interface to translate.
    pub id: &'static str,
    pub args: Vec<OsString>,
}

fn path(value: &Path) -> OsString {
    value.as_os_str().to_owned()
}

/// The folder a stage of training exports the adapter into.
fn output(run: &Path) -> PathBuf {
    run.join("output")
}

fn base_args(base: &TrainingBase) -> Vec<OsString> {
    vec![
        OsString::from("--models"),
        path(&base.models),
        OsString::from("--dit"),
        OsString::from(&base.dit),
    ]
}

/// The train-dit invocation of a recipe, writing into `out`. A continuation
/// (`from` an adapter) leaves the adapter's shape out: the trainer adopts the
/// one it was trained with.
fn train_args(inputs: &TrainingInputs, out: &Path, from: Option<&Path>) -> Vec<OsString> {
    let recipe = &inputs.recipe;
    let arg = |text: &str| OsString::from(text);
    let text = |value: &dyn ToString| OsString::from(value.to_string());
    let mut train = vec![arg("train-dit"), arg("--jsonl"), arg("--stages"), arg("train,export"), arg("--tensors"), path(&inputs.run.join("tensors")), arg("--out"), path(out)];
    train.extend(base_args(&inputs.base));
    match from {
        Some(adapter) => train.extend([arg("--init-adapter"), path(adapter)]),
        None => {
            train.extend([arg("--adapter-type"), arg(&recipe.adapter)]);
            if recipe.adapter == "lokr" {
                train.extend([arg("--lokr-dim"), text(&recipe.lokr_dim), arg("--lokr-alpha"), text(&recipe.lokr_alpha), arg("--lokr-factor"), text(&recipe.lokr_factor)]);
            } else {
                train.extend([arg("--rank"), text(&recipe.rank), arg("--alpha"), text(&recipe.alpha)]);
            }
            train.push(arg(if recipe.target_mlp { "--target-mlp" } else { "--no-target-mlp" }));
        }
    }
    train.extend([
        arg("--optimizer"),
        arg(&recipe.optimizer),
        arg("--lr"),
        text(&recipe.learning_rate),
        arg("--epochs"),
        text(&recipe.epochs),
        arg("--target-loss"),
        text(&recipe.target_loss),
        arg("--grad-accum"),
        text(&recipe.grad_accum),
        arg("--genre-ratio"),
        text(&recipe.genre_ratio),
        arg("--cfg-ratio"),
        text(&recipe.cfg_ratio),
        arg("--seed"),
        text(&recipe.seed),
        arg("--attn"),
        arg(&recipe.attention),
        // the author's shipped companions of the flash crop: bf16 storage
        // computed in f32, the mul_mat backward
        arg("--mirror"),
        arg(if recipe.attention == "flash" { "bf16-f32" } else { "f32" }),
        arg("--bwd"),
        arg("mm"),
        arg("--loss-weighting"),
        arg("flow_snr"),
        arg("--overwrite"),
    ]);
    train
}

/// The trainer's stages of a run, in order: the tensor cache from the songs,
/// then training and export. The audio and `dataset.json` are the studio's
/// to write first.
pub fn training_stages(inputs: &TrainingInputs) -> Vec<TrainingStage> {
    let arg = |text: &str| OsString::from(text);
    let mut preprocess = vec![
        arg("preprocess"),
        arg("--jsonl"),
        arg("--dataset"),
        path(&inputs.data.join("dataset.json")),
        arg("--out"),
        path(&inputs.run.join("tensors")),
    ];
    preprocess.extend(base_args(&inputs.base));
    preprocess.extend([arg("--vae"), arg(&inputs.base.vae), arg("--text-enc"), arg(&inputs.base.text_encoder), arg("--overwrite")]);
    vec![
        TrainingStage { id: "preprocess", args: preprocess },
        TrainingStage { id: "train", args: train_args(inputs, &output(&inputs.run), None) },
    ]
}

/// A run trained further from the adapter it left: `train-dit` starts from
/// it (`--init-adapter`) and writes a new export beside it, so the one
/// already installed stays as it was.
pub fn continuation_stage(inputs: &TrainingInputs, resume: &Path, out: &Path) -> TrainingStage {
    TrainingStage { id: "train", args: train_args(inputs, out, Some(resume)) }
}

/// The adapter files of an export folder, when it holds a finished one.
fn adapter_files(folder: &Path) -> Option<Vec<PathBuf>> {
    let lokr = folder.join("lokr_weights.safetensors");
    if lokr.is_file() {
        return Some(vec![lokr]);
    }
    let files: Vec<PathBuf> = ["adapter_model.safetensors", "adapter_config.json"].iter().map(|name| folder.join(name)).collect();
    files.iter().all(|file| file.is_file()).then_some(files)
}

/// The epoch an export's adapter is from, from the log the trainer writes
/// beside it: the best epoch it kept, else the last it ran.
fn trained_epochs(folder: &Path) -> Option<u32> {
    let log: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.join("dit_train_log.json")).ok()?).ok()?;
    ["saved_epoch", "epochs_run"].iter().find_map(|key| log.get(*key).and_then(serde_json::Value::as_u64)).map(|epochs| epochs as u32)
}

/// What a run counts its progress and its checkpoints in: epochs.
pub const PROGRESS_UNIT: &str = "epoch";

/// The export folders of a run, oldest first: `output`, then the
/// continuations' `output-<n>` by their number.
fn exports(run: &Path) -> Vec<PathBuf> {
    numbered_exports(run).into_iter().map(|(_, folder)| folder).collect()
}

fn numbered_exports(run: &Path) -> Vec<(u32, PathBuf)> {
    let order = |name: &str| if name == "output" { Some(0) } else { name.strip_prefix("output-")?.parse::<u32>().ok() };
    let mut folders: Vec<(u32, PathBuf)> = std::fs::read_dir(run)
        .map(|entries| entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|folder| Some((order(folder.file_name()?.to_str()?)?, folder)))
        .collect();
    folders.sort_by_key(|(number, _)| *number);
    folders
}

/// Where a run can be trained further from: its latest adapter, and the
/// epochs trained into it, the continuations' included.
pub fn resume_point(run: &Path) -> Option<(u32, PathBuf)> {
    let finished: Vec<PathBuf> = exports(run).into_iter().filter(|folder| adapter_files(folder).is_some()).collect();
    let epochs = finished.iter().filter_map(|folder| trained_epochs(folder)).sum();
    Some((epochs, finished.last()?.clone()))
}

/// The folder a continuation exports into.
pub fn continuation_output(run: &Path) -> PathBuf {
    // past the highest number, so a folder a failed continuation left is never reused
    run.join(format!("output-{}", numbered_exports(run).last().map_or(1, |(number, _)| number + 1)))
}

/// A step of the training stage, as the trainer reports it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TrainingStep {
    pub step: u32,
    pub loss: f64,
    pub step_ms: Option<f64>,
    /// The steps the whole run takes, as the trainer counts them.
    pub total: Option<u32>,
}

/// Reads one line of the trainer's output; progress lines are JSON.
pub fn parse_training_step(line: &str) -> Option<TrainingStep> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("type")?.as_str()? != "step" {
        return None;
    }
    let loss = value.get("loss").and_then(serde_json::Value::as_f64).filter(|value| value.is_finite())?;
    Some(TrainingStep {
        step: value.get("step")?.as_u64()? as u32,
        loss,
        step_ms: value.get("ms").and_then(serde_json::Value::as_f64).filter(|value| value.is_finite()),
        total: value.get("totalSteps").and_then(serde_json::Value::as_u64).map(|total| total as u32),
    })
}

/// A finished adapter: the epochs it trained, continuations included, and the
/// files generation uses.
#[derive(Debug, Clone, Serialize)]
pub struct TrainingCheckpoint {
    pub step: u32,
    pub files: Vec<PathBuf>,
}

/// The adapters a run has exported, the latest first.
pub fn checkpoints(run: &Path) -> Vec<TrainingCheckpoint> {
    let mut trained = 0;
    let mut found: Vec<TrainingCheckpoint> = exports(run)
        .into_iter()
        .filter_map(|folder| {
            let files = adapter_files(&folder)?;
            trained += trained_epochs(&folder).unwrap_or(0);
            Some(TrainingCheckpoint { step: trained, files })
        })
        .collect();
    found.reverse();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(recipe: Recipe) -> TrainingInputs {
        TrainingInputs {
            data: "d".into(),
            base: TrainingBase { models: "m".into(), dit: "acestep-v15-turbo-Q8_0.gguf".into(), vae: "vae-BF16.gguf".into(), text_encoder: "Qwen3-Embedding-0.6B-Q8_0.gguf".into() },
            run: "r".into(),
            recipe,
        }
    }

    fn strings(stage: &TrainingStage) -> Vec<String> {
        stage.args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect()
    }

    fn has(args: &[String], pair: [&str; 2]) -> bool {
        args.windows(2).any(|window| window == pair)
    }

    #[test]
    fn stages_preprocess_then_train_with_the_studio_s_models() {
        let stages = training_stages(&inputs(Recipe::default()));
        assert_eq!(stages.iter().map(|stage| stage.id).collect::<Vec<_>>(), ["preprocess", "train"]);
        let preprocess = strings(&stages[0]);
        for pair in [["--dit", "acestep-v15-turbo-Q8_0.gguf"], ["--vae", "vae-BF16.gguf"], ["--text-enc", "Qwen3-Embedding-0.6B-Q8_0.gguf"], ["--models", "m"]] {
            assert!(has(&preprocess, pair), "{pair:?}");
        }
        let train = strings(&stages[1]);
        for pair in [["--rank", "128"], ["--alpha", "256"], ["--optimizer", "prodigy"], ["--attn", "flash"], ["--mirror", "bf16-f32"], ["--genre-ratio", "30"], ["--stages", "train,export"]] {
            assert!(has(&train, pair), "{pair:?}");
        }
        assert!(train.contains(&"--target-mlp".to_string()));
        assert!(!train.iter().any(|arg| arg == "--lokr-dim"));
    }

    #[test]
    fn a_lokr_recipe_trains_a_lokr() {
        let train = strings(&training_stages(&inputs(Recipe { adapter: "lokr".into(), target_mlp: false, ..Recipe::default() }))[1]);
        assert!(has(&train, ["--adapter-type", "lokr"]) && has(&train, ["--lokr-factor", "6"]));
        assert!(!train.iter().any(|arg| arg == "--rank"));
        assert!(train.contains(&"--no-target-mlp".to_string()));
    }

    #[test]
    fn a_continuation_starts_from_the_adapter_and_keeps_its_shape() {
        let stage = continuation_stage(&inputs(Recipe::default()), Path::new("r/output"), Path::new("r/output-1"));
        let args = strings(&stage);
        assert!(has(&args, ["--init-adapter", "r/output"]));
        assert!(has(&args, ["--out", "r/output-1"]));
        for flag in ["--adapter-type", "--rank", "--alpha", "--target-mlp"] {
            assert!(!args.iter().any(|arg| arg == flag), "{flag} is the adapter's identity");
        }
        assert!(has(&args, ["--epochs", "500"]), "the schedule is kept");
    }

    #[test]
    fn a_step_line_becomes_a_step_and_other_lines_do_not() {
        let step = parse_training_step(r#"{"type":"step","epoch":2,"step":12,"totalSteps":600,"micro":4,"loss":0.52,"rawLoss":0.6,"lr":1,"gradNorm":0.1,"clipScale":1,"t":0.4,"crop":1500,"cropStart":0,"cfgDrop":0,"ms":900,"vramMb":14000}"#).unwrap();
        assert_eq!(step, TrainingStep { step: 12, loss: 0.52, step_ms: Some(900.0), total: Some(600) });
        assert!(parse_training_step(r#"{"type":"epoch","epoch":1,"epochs":500,"loss":0.6}"#).is_none());
        assert!(parse_training_step("[train-dit] loading").is_none());
    }

    #[test]
    fn exports_are_the_checkpoints_and_the_latest_resumes() {
        let run = std::env::temp_dir().join(format!("ace-train-exports-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&run);
        std::fs::create_dir_all(run.join("output")).unwrap();
        assert!(resume_point(&run).is_none() && checkpoints(&run).is_empty(), "nothing exported yet");
        std::fs::write(run.join("output").join("adapter_model.safetensors"), b"w").unwrap();
        std::fs::write(run.join("output").join("adapter_config.json"), b"{}").unwrap();
        std::fs::write(run.join("output").join("dit_train_log.json"), br#"{"saved_epoch": 300, "epochs_run": 320}"#).unwrap();
        let (step, folder) = resume_point(&run).unwrap();
        assert_eq!((step, folder.file_name().unwrap().to_str().unwrap()), (300, "output"));
        assert_eq!(continuation_output(&run).file_name().unwrap().to_str().unwrap(), "output-1");
        assert_eq!(checkpoints(&run)[0].files.len(), 2);
        let _ = std::fs::remove_dir_all(&run);
    }

    #[test]
    fn every_recipe_setting_has_a_field() {
        let recipe = serde_json::to_value(Recipe::default()).unwrap();
        let keys: Vec<&str> = recipe.as_object().unwrap().keys().map(String::as_str).filter(|key| *key != "steps").collect();
        let fields: Vec<&str> = recipe_fields().iter().map(|field| field.key).collect();
        assert_eq!(keys.len(), fields.len());
        for key in &keys {
            assert!(fields.contains(key), "{key} has no field");
        }
        assert!(Recipe::default().check().is_ok());
    }
}
