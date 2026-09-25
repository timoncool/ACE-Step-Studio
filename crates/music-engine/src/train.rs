//! How MiniMax Music 3 adapters are trained: the weights the trainer needs,
//! the stages it runs and what they print.
//!
//! The trainer is HOT-Step's `ace-train`, built from a pinned commit and
//! shipped as `music-train`. It trains the planner LM - the half that writes
//! the song - on RVQ codes it encodes from the dataset's audio, and exports a
//! PEFT LoRA the engine merges at load. It reads its own GGUF conversion of
//! the model, which is why the training pack brings its own LM.
//!
//! A dataset reaches the trainer as a folder of 44.1 kHz stereo float WAV
//! files the studio writes, each with a `<id>.mm3.txt` structured caption, and
//! a `dataset.json` listing them with their lyrics.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The Hugging Face repository and commit the training weights come from.
pub const WEIGHTS_REPOSITORY: &str = "scragnog/MiniMax-Music3-GGUF";
pub const WEIGHTS_REVISION: &str = "3bc27a90ab1b182a692744f46be18385098293c7";

/// The rate the trainer's audio encoder takes.
pub const SAMPLE_RATE: u32 = 44_100;

/// Frames per second of the LM's code stream; 9000 frames is its 6:00 limit.
pub const FRAMES_PER_SECOND: u32 = 25;
pub const MAX_FRAMES: u32 = 9000;

/// Video memory a run of the default recipe needs, in GB: the q8_0 base, rank
/// 128 HOT-PiZZA with AdamW and a 1536 frame window of exact attention, as
/// measured on a 24 GB card with room to spare.
pub const MIN_VRAM_GB: u32 = 22;

/// One file of the training pack, stored flat in the training models folder.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TrainingFile {
    pub id: &'static str,
    pub label: &'static str,
    pub file: &'static str,
    pub bytes: u64,
}

pub const TRAINING_FILES: &[TrainingFile] = &[
    TrainingFile { id: "mm3-train-lm", label: "MiniMax Music 3 LM for training (Q8_0)", file: "mm3-lm-q8_0.gguf", bytes: 9_129_105_696 },
    TrainingFile { id: "mm3-train-depth", label: "Depth decoder (acoustic loss)", file: "mm3-depth-f16.gguf", bytes: 1_292_259_232 },
    TrainingFile { id: "mm3-train-rvq", label: "RVQ encoder", file: "mm3-rvq-53kpooled-f32.gguf", bytes: 676_044_352 },
    TrainingFile { id: "mm3-train-enc", label: "Audio encoder", file: "mm3-enc-f16.gguf", bytes: 89_466_528 },
];

pub fn training_file_url(file: &TrainingFile) -> String {
    format!("https://huggingface.co/{WEIGHTS_REPOSITORY}/resolve/{WEIGHTS_REVISION}/{}", file.file)
}

/// What a run is asked for. The defaults are HOT-Step's Balanced recipe for
/// MM3 planner adapters (`MM3_LM_DEFAULTS` with the Balanced preset), except
/// for the window: a whole song needs a 32 GB card, and 1536 frames of exact
/// attention fit 24 GB.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recipe {
    /// `steps`: train `steps`; `epochs`: train `epochs` passes over the songs.
    pub stop: String,
    /// One epoch is one pass over the dataset: a step trains one song.
    pub epochs: u32,
    pub steps: u32,
    pub save_every: u32,
    pub seed: u32,
    /// `hot_pizza` (PiSSA with principal-subspace dropout, the author's
    /// default and his best by ear), `pissa` or plain `lora`.
    pub method: String,
    pub rank: u32,
    pub alpha: f64,
    /// Share of the rank dropped each step; the mask is what HOT-PiZZA is.
    pub rank_dropout: f64,
    /// `adamw`, `prodigy` (finds its own step size) or `muon`.
    pub optimizer: String,
    pub learning_rate: f64,
    pub warmup: u32,
    /// Frames of a song one step sees; 9000 is a whole song.
    pub max_frames: u32,
    /// `exact` or `flash`; flash has a large fixed cost and pays off only for
    /// long windows.
    pub attention: String,
    /// Weight of the acoustic codebooks' loss through the depth decoder;
    /// without it the adapter damages the timbre.
    pub depth_loss_weight: f64,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            stop: "steps".into(),
            epochs: 40,
            steps: 600,
            save_every: 100,
            seed: 42,
            method: "hot_pizza".into(),
            rank: 128,
            alpha: 128.0,
            rank_dropout: 0.1,
            optimizer: "adamw".into(),
            learning_rate: 8e-5,
            warmup: 25,
            max_frames: 1536,
            attention: "exact".into(),
            depth_loss_weight: 1.0,
        }
    }
}

impl Recipe {
    /// The recipe the trainer runs for a dataset of `songs`: by epochs, the
    /// steps are the epochs times the songs.
    pub fn for_songs(&self, songs: usize) -> Recipe {
        let mut recipe = self.clone();
        if recipe.stop == "epochs" {
            recipe.steps = recipe.epochs.saturating_mul(songs.max(1) as u32);
        }
        recipe
    }

    /// Refuses what the trainer would refuse, before any stage starts.
    pub fn check(&self) -> Result<(), String> {
        if self.epochs == 0 {
            return Err("epochs must be at least 1".into());
        }
        if !matches!(self.stop.as_str(), "steps" | "epochs") {
            return Err(format!("unknown stopping rule {}", self.stop));
        }
        if self.steps == 0 || self.save_every == 0 {
            return Err("steps and the checkpoint interval must be at least 1".into());
        }
        if !matches!(self.method.as_str(), "hot_pizza" | "pissa" | "lora") {
            return Err(format!("unknown method {}", self.method));
        }
        if !matches!(self.optimizer.as_str(), "adamw" | "prodigy" | "muon") {
            return Err(format!("unknown optimizer {}", self.optimizer));
        }
        if !matches!(self.attention.as_str(), "exact" | "flash") {
            return Err(format!("unknown attention {}", self.attention));
        }
        if self.rank == 0 || !(64..=MAX_FRAMES).contains(&self.max_frames) {
            return Err(format!("the rank must be at least 1 and the window between 64 and {MAX_FRAMES} frames"));
        }
        let finite = [self.alpha, self.learning_rate, self.rank_dropout, self.depth_loss_weight].iter().all(|value| value.is_finite());
        if !finite || self.alpha <= 0.0 || self.learning_rate <= 0.0 || !(0.0..1.0).contains(&self.rank_dropout) || self.depth_loss_weight < 0.0 {
            return Err("a recipe number is out of range".into());
        }
        if self.method == "hot_pizza" && self.rank_dropout <= 0.0 {
            return Err("HOT-PiZZA needs a rank dropout above 0: the mask is the method".into());
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

/// The settings of a MiniMax Music 3 run, in the order the page shows them.
pub fn recipe_fields() -> Vec<RecipeField> {
    vec![
        choice("stop", "stop", &["steps", "epochs"]),
        RecipeField { shown_when: Some(FieldCondition { field: "stop", values: &["steps"] }), ..integer("steps", "stop", 1.0, 5000.0, 50.0) },
        RecipeField { shown_when: Some(FieldCondition { field: "stop", values: &["epochs"] }), ..integer("epochs", "stop", 1.0, 500.0, 1.0) },
        integer("save_every", "stop", 1.0, 1000.0, 10.0),
        integer("seed", "stop", 0.0, 4_294_967_295.0, 1.0),
        choice("method", "adapter", &["hot_pizza", "pissa", "lora"]),
        integer("rank", "adapter", 1.0, 512.0, 8.0),
        number("alpha", "adapter", 1.0, 1024.0, 8.0),
        RecipeField { off_when: Some(FieldCondition { field: "method", values: &["lora", "pissa"] }), ..number("rank_dropout", "adapter", 0.0, 0.9, 0.05) },
        choice("optimizer", "optimizer", &["adamw", "prodigy", "muon"]),
        RecipeField { off_when: Some(FieldCondition { field: "optimizer", values: &["prodigy"] }), ..number("learning_rate", "optimizer", 0.0, 0.01, 0.00001) },
        integer("warmup", "optimizer", 0.0, 1000.0, 5.0),
        integer("max_frames", "window", 64.0, MAX_FRAMES as f64, 250.0),
        choice("attention", "window", &["exact", "flash"]),
        number("depth_loss_weight", "window", 0.0, 4.0, 0.1),
    ]
}

/// What a run is asked to do.
#[derive(Debug, Clone)]
pub struct TrainingInputs {
    /// `dataset.json`, the captions and the WAV files, written by the studio.
    pub data: PathBuf,
    /// The training models folder, holding [`TRAINING_FILES`].
    pub models: PathBuf,
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

/// The trainer's stages of a run, in order: RVQ codes from the audio, then
/// training. The audio and captions they read are the studio's to write first.
pub fn training_stages(inputs: &TrainingInputs) -> Vec<TrainingStage> {
    let models = &inputs.models;
    let recipe = &inputs.recipe;
    let codes = inputs.run.join("codes");
    let manifest = inputs.data.join("dataset.json");
    let arg = |text: &str| OsString::from(text);
    let text = |value: &dyn ToString| OsString::from(value.to_string());
    let codes_stage = vec![
        arg("mm3-codes"),
        arg("--jsonl"),
        arg("--dataset"),
        path(&manifest),
        arg("--rvq"),
        path(&models.join(TRAINING_FILES[2].file)),
        arg("--enc"),
        path(&models.join(TRAINING_FILES[3].file)),
        arg("--out"),
        path(&codes),
        arg("--ffmpeg"),
        arg("none"),
    ];
    let mut train = vec![
        arg("mm3-lm-train"),
        arg("--jsonl"),
        arg("--lm"),
        path(&models.join(TRAINING_FILES[0].file)),
        arg("--depth"),
        path(&models.join(TRAINING_FILES[1].file)),
        arg("--manifest"),
        path(&manifest),
        arg("--captions"),
        path(&inputs.data),
        arg("--codes"),
        path(&codes.join("codes")),
        arg("--out"),
        path(&inputs.run.join("output")),
        arg("--rank"),
        text(&recipe.rank),
        arg("--alpha"),
        text(&recipe.alpha),
        arg("--lr"),
        text(&recipe.learning_rate),
        arg("--lr-end-frac"),
        arg("0.005"),
        arg("--steps"),
        text(&recipe.steps),
        arg("--save-every"),
        text(&recipe.save_every.max(1)),
        arg("--warmup"),
        text(&recipe.warmup),
        arg("--seed"),
        text(&recipe.seed),
        arg("--max-frames"),
        text(&recipe.max_frames),
        arg("--drop-over-frames"),
        text(&MAX_FRAMES),
        arg("--crop-mode"),
        arg("structured"),
        arg("--crop-start-frac"),
        arg("0.2"),
        arg("--crop-end-frac"),
        arg("0.15"),
        arg("--crop-anchor"),
        arg("song"),
        arg("--optimizer"),
        arg(&recipe.optimizer),
        arg("--attn"),
        arg(&recipe.attention),
        arg("--depth-loss-weight"),
        text(&recipe.depth_loss_weight),
        arg("--depth-loss-frames"),
        arg("128"),
        arg("--holdout"),
        arg("0"),
    ];
    match recipe.method.as_str() {
        "hot_pizza" => train.extend([arg("--hot-pizza"), arg("--rank-dropout"), text(&recipe.rank_dropout)]),
        "pissa" => train.push(arg("--pissa")),
        _ => {}
    }
    if recipe.method != "lora" {
        // A plain rank-2r LoRA on the original base, which the engine merges;
        // the default delta form needs a residual file no loader here reads.
        // The frozen factors in F16 halve their memory, as HOT-Step runs it.
        train.extend([arg("--pissa-standalone"), arg("--pissa-frozen-f16")]);
    }
    vec![TrainingStage { id: "codes", args: codes_stage }, TrainingStage { id: "train", args: train }]
}

/// A run trained further, up to `inputs.recipe.steps`: the trainer resumes
/// the optimizer, the song order and the step count from `resume` and keeps
/// writing into the run's output. Its songs and codes are the run's already.
pub fn continuation_stage(inputs: &TrainingInputs, resume: &Path) -> TrainingStage {
    let mut train = training_stages(inputs).pop().expect("the training stage").args;
    train.extend([OsString::from("--resume"), path(resume)]);
    TrainingStage { id: "train", args: train }
}

/// The trainer's refusal, before it is asked: PiSSA and HOT-PiZZA fit frozen
/// factors to the base at step 0, and the resume state does not carry them.
pub fn continuation_refused(recipe: &Recipe) -> bool {
    recipe.method != "lora"
}

/// The state a run can be continued from, and its step: the trainer writes it
/// on a clean finish, `resume-state.bin` with `resume-state.json` beside it.
pub fn resume_point(run: &Path) -> Option<(u32, PathBuf)> {
    let output = run.join("output");
    let state = output.join("resume-state.bin");
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(output.join("resume-state.json")).ok()?).ok()?;
    let step = meta.get("step")?.as_u64()? as u32;
    state.is_file().then_some((step, state))
}

/// A step of the training stage, as the trainer reports it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TrainingStep {
    pub step: u32,
    pub loss: f64,
    pub step_ms: Option<f64>,
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
        step_ms: value.get("stepMs").and_then(serde_json::Value::as_f64).filter(|value| value.is_finite()),
    })
}

/// A finished checkpoint: its step and the adapter files generation uses.
#[derive(Debug, Clone, Serialize)]
pub struct TrainingCheckpoint {
    pub step: u32,
    pub files: Vec<PathBuf>,
}

/// The checkpoints a run has written so far, the latest first: PEFT folders
/// `ckpt-<step>` with the weights and the config that carries their alpha.
pub fn checkpoints(run: &Path) -> Vec<TrainingCheckpoint> {
    let Ok(entries) = std::fs::read_dir(run.join("output")) else { return Vec::new() };
    let mut found: Vec<TrainingCheckpoint> = entries
        .flatten()
        .filter_map(|entry| {
            let step = entry.file_name().to_str()?.strip_prefix("ckpt-")?.parse().ok()?;
            let files: Vec<PathBuf> = ["adapter_model.safetensors", "adapter_config.json"].iter().map(|name| entry.path().join(name)).collect();
            files.iter().all(|file| file.is_file()).then_some(TrainingCheckpoint { step, files })
        })
        .collect();
    found.sort_by(|a, b| b.step.cmp(&a.step));
    found
}

/// The caption a song trains with: its structured caption, the trigger word
/// leading the Global Metadata section, where the create form puts it too.
pub fn caption_with_trigger(caption: &str, trigger: &str) -> String {
    const HEADING: &str = "Global Metadata";
    let caption = caption.trim();
    let trigger = trigger.trim();
    let structured = caption.starts_with(HEADING);
    let (head, body) = if structured { caption.split_at(HEADING.len()) } else { ("", caption) };
    let body = body.trim_start_matches(['\r', '\n']);
    let lead = if trigger.is_empty() || body.split(',').any(|part| part.trim().eq_ignore_ascii_case(trigger)) {
        body.to_string()
    } else if body.is_empty() {
        trigger.to_string()
    } else {
        format!("{trigger}, {body}")
    };
    format!("{}\n{lead}", if structured { head } else { HEADING })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_goes_on_from_the_state_it_finished_with() {
        let run = std::env::temp_dir().join(format!("mm-train-resume-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&run);
        let output = run.join("output");
        std::fs::create_dir_all(&output).unwrap();
        assert!(resume_point(&run).is_none(), "no state, nothing to go on from");
        std::fs::write(output.join("resume-state.bin"), b"state").unwrap();
        std::fs::write(output.join("resume-state.json"), br#"{"reason": "final", "state": "x", "step": 600, "totalSteps": 600}"#).unwrap();
        let (step, state) = resume_point(&run).unwrap();
        assert_eq!(step, 600);
        let inputs = TrainingInputs { data: run.join("data"), models: PathBuf::from("models"), run: run.clone(), recipe: Recipe { method: "lora".into(), steps: 850, ..Recipe::default() } };
        let stage = continuation_stage(&inputs, &state);
        let args: Vec<String> = stage.args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(stage.id, "train");
        assert_eq!(args[args.iter().position(|arg| arg == "--resume").unwrap() + 1], state.to_string_lossy());
        assert_eq!(args[args.iter().position(|arg| arg == "--steps").unwrap() + 1], "850");
        assert!(!args.iter().any(|arg| arg.starts_with("--pissa") || arg == "--hot-pizza"));
        assert!(continuation_refused(&Recipe::default()), "HOT-PiZZA, the default, cannot be continued");
        let _ = std::fs::remove_dir_all(&run);
    }

    #[test]
    fn epochs_become_steps() {
        let by_epochs = Recipe { stop: "epochs".into(), epochs: 40, ..Recipe::default() }.for_songs(13);
        assert_eq!(by_epochs.steps, 520);
        assert_eq!(Recipe::default().for_songs(13).steps, 600);
    }

    #[test]
    fn a_step_line_becomes_a_step_and_other_lines_do_not() {
        let step = parse_training_step(r#"{"type":"step","step":12,"totalSteps":600,"loss":2.5,"lr":8e-5,"gradNorm":1,"clipScale":1,"ms":900,"stepMs":750,"reg":false,"depthLoss":0.4}"#).unwrap();
        assert_eq!(step, TrainingStep { step: 12, loss: 2.5, step_ms: Some(750.0) });
        assert!(parse_training_step(r#"{"type":"vram","step":0,"usedMb":1}"#).is_none());
        assert!(parse_training_step("[mm3-lm-train] loading").is_none());
    }

    #[test]
    fn stages_encode_codes_then_train_with_the_recipe() {
        let inputs = TrainingInputs { data: "d".into(), models: "m".into(), run: "r".into(), recipe: Recipe::default() };
        let stages = training_stages(&inputs);
        assert_eq!(stages.iter().map(|stage| stage.id).collect::<Vec<_>>(), ["codes", "train"]);
        let strings = |index: usize| -> Vec<String> { stages[index].args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect() };
        assert!(strings(0).windows(2).any(|pair| pair == ["--ffmpeg", "none"]));
        let train = strings(1);
        for pair in [["--rank", "128"], ["--optimizer", "adamw"], ["--max-frames", "1536"], ["--attn", "exact"], ["--rank-dropout", "0.1"]] {
            assert!(train.windows(2).any(|window| window == pair), "{pair:?}");
        }
        assert!(train.iter().any(|arg| arg == "--hot-pizza"));
        assert!(train.iter().any(|arg| arg == "--pissa-standalone"), "the engine merges plain LoRA only");
        assert!(!train.iter().any(|arg| arg == "--pissa-cache-dir"), "a PiSSA cache would export a delta the engine cannot merge");
        let plain = TrainingInputs { recipe: Recipe { method: "lora".into(), ..Recipe::default() }, ..inputs };
        assert!(!training_stages(&plain)[1].args.iter().any(|arg| arg == "--hot-pizza" || arg == "--rank-dropout"));
    }

    #[test]
    fn every_recipe_setting_has_a_field() {
        let recipe = serde_json::to_value(Recipe::default()).unwrap();
        let keys: Vec<&str> = recipe.as_object().unwrap().keys().map(String::as_str).collect();
        let fields: Vec<&str> = recipe_fields().iter().map(|field| field.key).collect();
        assert_eq!(keys.len(), fields.len());
        for key in &keys {
            assert!(fields.contains(key), "{key} has no field");
        }
        assert!(Recipe::default().check().is_ok());
    }

    #[test]
    fn the_trigger_leads_the_global_metadata() {
        let caption = "Global Metadata\nindie pop, 90 BPM\nVocal Details\nsoft female";
        assert_eq!(caption_with_trigger(caption, "monetochka"), "Global Metadata\nmonetochka, indie pop, 90 BPM\nVocal Details\nsoft female");
        assert_eq!(caption_with_trigger("indie pop", "monetochka"), "Global Metadata\nmonetochka, indie pop");
        assert_eq!(caption_with_trigger("Global Metadata\nmonetochka, pop", "monetochka"), "Global Metadata\nmonetochka, pop");
        assert_eq!(caption_with_trigger("indie pop", ""), "Global Metadata\nindie pop");
    }
}
