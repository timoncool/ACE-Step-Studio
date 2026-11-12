//! Karaoke timings for a finished track.
//!
//! The words are already known - the model sang the lyrics it was given - but the
//! *timings* are not, and the video studio's karaoke layer and the player both
//! need them. This module produces an LRC file for a track using whichever
//! recogniser the user picked; like every optional extra here it is off by
//! default and downloads nothing on its own.
//!
//! Three backends, all of them explicit choices:
//!
//!   * **Whisper** - whisper.cpp run as a sidecar. It writes LRC itself, and
//!     the CUDA build is preferred over the CPU one when both are installed.
//!   * **Parakeet** - the NVIDIA TDT model on transcribe.cpp (CUDA, Vulkan or
//!     the processor), the same one Dub Studio and Speech Studio use.
//!   * **OpenRouter** - a cloud model, billed to the user's own key, asked for
//!     verbose output because plain text carries no timings.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

pub use crate::downloads::Asset;
use crate::downloads::{AssetKind, Downloader};

/// Which recogniser produces the timings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsrProvider {
    #[default]
    None,
    Whisper,
    Parakeet,
    OpenRouter,
}

/// Persisted with the rest of the studio settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LyricsSyncConfig {
    /// The karaoke switch. Off means the buttons do not appear at all.
    pub enabled: bool,
    pub provider: AsrProvider,
    /// Which downloaded Whisper model to run.
    pub whisper_model: Option<String>,
    /// Which OpenRouter speech-to-text model to call.
    pub openrouter_model: Option<String>,
    /// What the local recogniser runs on. It decides which runtime is
    /// downloaded as much as which one is loaded, so it belongs to the setting
    /// rather than to a guess made at load time.
    #[serde(default)]
    pub runtime: OnnxFlavour,
}

impl LyricsSyncConfig {
    pub fn available(&self) -> bool {
        self.enabled && self.provider != AsrProvider::None
    }

    /// A choice the dropdown no longer offers, as the one it became: fp32 Parakeet is the v3 model, and a Whisper size
    /// of the retired whisper.cpp names no model at all.
    pub fn migrated(mut self) -> Self {
        match self.whisper_model.as_deref() {
            Some(PARAKEET_FP32) => self.whisper_model = Some(PARAKEET_V3.to_string()),
            Some(PARAKEET_V3) | Some(PARAKEET_ULTRA) | None => {}
            Some(_) => self.whisper_model = None,
        }
        self
    }
}

/// The reason the interface shows when a recogniser hears nothing sung: a
/// code, not a sentence, so it is put in the reader's language.
pub const NO_WORDS: &str = "karaoke.no-words";

/// A cloud recogniser answered with no timed segments: the model returns no timings, or heard nothing. Not
/// `NO_WORDS`, which marks a song instrumental in dataset preparation.
pub const NO_TIMINGS: &str = "karaoke.no-timings";

/// CrisperWhisper's id in the server and the id of its weights among the downloads; the folder its files land in is
/// the Hugging Face repository's, as the other programs keep it.
pub const CRISPERWHISPER: &str = "crisperwhisper";
pub const CRISPERWHISPER_MODEL: &str = "crisperwhisper-turbo";
const CRISPERWHISPER_DIR: &str = "models/crisperwhisper2-turbo";

/// One file of CrisperWhisper's repository beside its weights.
const fn crisper_file(id: &'static str, url: &'static str, relative_path: &'static str, bytes: u64) -> Asset {
    Asset { id, label: "CrisperWhisper 2.0 turbo", kind: AssetKind::Model, url, relative_path, bytes, unzip_into: None, marker: "", pick: &[], vram_gb: None, note: "Part of CrisperWhisper 2.0 turbo." }
}

/// Phrases Whisper writes over music and silence instead of saying it heard
/// no words: the verbatim hallucinations of the "Bag of Hallucinations"
/// study (Barański et al., ICASSP 2025, MIT) and the per-language lists of
/// NVIDIA NeMo's Granary pipeline (Apache-2.0), as merged by
/// Scicom-AI/Whisper-Hallucination, kept for the studio's languages and a few
/// common ones, phrases of two words or more, normalised.
const HALLUCINATIONS: &str = include_str!("whisper_hallucinations.txt");

/// Where a batch was recognised: on the device chosen, or on the processor
/// after the card refused it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recognised {
    OnDevice,
    OnProcessor,
}

/// Words that only ever appear in a subtitler's credit, never in a song.
const CREDIT_MARKERS: &[&str] = &["dimatorzok", "субтитр", "amara.org", "untertitel", "sous-titr", "subtítulo", "sottotitol", "legendas por", "subtitles by", "字幕", "자막"];

fn normalised(text: &str) -> String {
    let lowered = text.to_lowercase().replace('ё', "е");
    lowered
        .split(|c: char| c.is_whitespace() || ".,!?…\"'«»“”„-–—:;()[]♪。、！？「」".contains(c))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether a recognised segment is Whisper filling a gap rather than words
/// that were sung: a known hallucination said whole, a subtitler's credit, a
/// sound written as a caption ("ВЕСЕЛАЯ МУЗЫКА", "[Music]"), or no letters at
/// all. A segment is judged whole, so a sung line that merely contains such a
/// phrase stays.
pub fn is_hallucination(text: &str) -> bool {
    static KNOWN: std::sync::OnceLock<std::collections::HashSet<&'static str>> = std::sync::OnceLock::new();
    let trimmed = text.trim();
    if !trimmed.chars().any(char::is_alphabetic) {
        return true;
    }
    let bracketed = (trimmed.starts_with('[') && trimmed.ends_with(']')) || (trimmed.starts_with('(') && trimmed.ends_with(')')) || trimmed.starts_with('♪');
    let letters: Vec<char> = trimmed.chars().filter(|c| c.is_alphabetic()).collect();
    let shouted = letters.len() >= 3 && letters.iter().all(|c| !c.is_lowercase()) && letters.iter().any(|c| c.is_uppercase());
    if bracketed || shouted {
        return true;
    }
    let plain = normalised(trimmed);
    if CREDIT_MARKERS.iter().any(|marker| plain.contains(marker)) {
        return true;
    }
    KNOWN.get_or_init(|| HALLUCINATIONS.lines().filter(|line| !line.is_empty()).collect()).contains(plain.as_str())
}

/// The words of a transcript with their start times, less those of a segment that is the model filling a gap rather
/// than words that were sung.
fn heard_words(transcript: &acpp_server::Transcript) -> Vec<(f64, String)> {
    let made_up: Vec<(f64, f64)> = transcript.segments.iter().filter(|segment| is_hallucination(&segment.text)).map(|segment| (segment.start, segment.end)).collect();
    transcript
        .words
        .iter()
        // a segment's end is the next one's start: a word there belongs to the next; a segment of no length holds
        // only a word at its very time
        .filter(|word| !made_up.iter().any(|(start, end)| if start == end { word.start == *start } else { word.start >= *start && word.start < *end }))
        .map(|word| (word.start, word.text.clone()))
        .collect()
}

/// What CrisperWhisper needs on `runtime`'s build: the server and the CUDA libraries it does not carry, and every file
/// of the model.
pub fn crisper_assets(runtime: OnnxFlavour) -> Vec<&'static str> {
    let Ok(build) = crate::acpp::build_for(runtime) else { return Vec::new() };
    let model = OWN.iter().filter(|asset| asset.relative_path.starts_with(CRISPERWHISPER_DIR)).map(|asset| asset.id);
    crate::acpp::server_ids(build).iter().copied().chain(model).collect()
}

/// Every download of karaoke: its own and the audio.cpp server CrisperWhisper runs in.
pub fn assets() -> &'static [Asset] {
    static ALL: std::sync::OnceLock<Vec<Asset>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| crate::acpp::ASSETS.iter().chain(OWN).copied().collect())
}

/// Karaoke's own downloads; the audio.cpp server CrisperWhisper runs in is `acpp::ASSETS`, which `assets()` adds.
const OWN: &[Asset] = &[
    // CrisperWhisper 2.0 turbo as Speech Studio and Dub Studio fetch it: the Hugging Face repository at its revision,
    // in a folder of that name, so one program's copy serves the others
    Asset {
        id: CRISPERWHISPER_MODEL,
        label: "CrisperWhisper 2.0 turbo",
        kind: AssetKind::Model,
        url: "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/model.safetensors",
        relative_path: "models/crisperwhisper2-turbo/model.safetensors",
        bytes: 1_617_907_538,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(3),
        note: "Whisper large-v3 turbo trained for verbatim word timings, in audio.cpp: what karaoke and the dataset lyrics are heard with.",
    },
    crisper_file("crisperwhisper-turbo-license", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/LICENSE.md", "models/crisperwhisper2-turbo/LICENSE.md", 17_784),
    crisper_file("crisperwhisper-turbo-readme", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/README.md", "models/crisperwhisper2-turbo/README.md", 10_974),
    crisper_file("crisperwhisper-turbo-added-tokens", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/added_tokens.json", "models/crisperwhisper2-turbo/added_tokens.json", 35_319),
    crisper_file("crisperwhisper-turbo-config", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/config.json", "models/crisperwhisper2-turbo/config.json", 1_211),
    crisper_file("crisperwhisper-turbo-generation-config", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/generation_config.json", "models/crisperwhisper2-turbo/generation_config.json", 3_880),
    crisper_file("crisperwhisper-turbo-merges", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/merges.txt", "models/crisperwhisper2-turbo/merges.txt", 493_869),
    crisper_file("crisperwhisper-turbo-normalizer", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/normalizer.json", "models/crisperwhisper2-turbo/normalizer.json", 52_666),
    crisper_file("crisperwhisper-turbo-preprocessor-config", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/preprocessor_config.json", "models/crisperwhisper2-turbo/preprocessor_config.json", 357),
    crisper_file("crisperwhisper-turbo-special-tokens-map", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/special_tokens_map.json", "models/crisperwhisper2-turbo/special_tokens_map.json", 2_186),
    crisper_file("crisperwhisper-turbo-tokenizer", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/tokenizer.json", "models/crisperwhisper2-turbo/tokenizer.json", 3_937_105),
    crisper_file("crisperwhisper-turbo-tokenizer-config", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/tokenizer_config.json", "models/crisperwhisper2-turbo/tokenizer_config.json", 288_287),
    crisper_file("crisperwhisper-turbo-vocab", "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/vocab.json", "models/crisperwhisper2-turbo/vocab.json", 835_528),
    // Parakeet on transcribe.cpp, the files Dub Studio and Speech Studio fetch under the same names, so one
    // program's copy serves the others; addresses and sizes are the shared binding's.
    Asset {
        id: PARAKEET_V3,
        label: "Parakeet TDT 0.6B v3 (Q8_0)",
        kind: AssetKind::Model,
        url: dub_tcpp::MODELS[0].download.url,
        relative_path: "models/asr-gguf-parakeet-v3/parakeet-tdt-0.6b-v3-Q8_0.gguf",
        bytes: dub_tcpp::MODELS[0].download.size,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(1),
        note: "NVIDIA's recogniser, the one Dub Studio uses: 25 European languages, the time of each word.",
    },
    Asset {
        id: PARAKEET_ULTRA,
        label: "Parakeet Ultra 0.6B (Q8_0)",
        kind: AssetKind::Model,
        url: dub_tcpp::MODELS[1].download.url,
        relative_path: "models/asr-gguf-parakeet-ultra/parakeet-ultra-Q8_0.gguf",
        bytes: dub_tcpp::MODELS[1].download.size,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(1),
        note: "Moondream's post-trained Parakeet v3: fewer wrong words in Russian and in noise.",
    },
    #[cfg(any(windows, target_os = "linux"))]
    Asset {
        id: "transcribe-engine",
        label: "transcribe.cpp (CUDA)",
        kind: AssetKind::Runtime,
        url: dub_tcpp::Build::Cuda.download().url,
        relative_path: TCPP_CUDA,
        bytes: dub_tcpp::Build::Cuda.download().size,
        unzip_into: Some("transcribe-engine"),
        marker: dub_tcpp::LIBRARY,
        pick: &[],
        vram_gb: None,
        note: "Runs Parakeet on an NVIDIA card.",
    },
    #[cfg(any(windows, target_os = "linux"))]
    Asset {
        id: "transcribe-engine-vulkan",
        label: "transcribe.cpp (Vulkan)",
        kind: AssetKind::Runtime,
        url: dub_tcpp::Build::Vulkan.download().url,
        relative_path: TCPP_VULKAN,
        bytes: dub_tcpp::Build::Vulkan.download().size,
        unzip_into: Some("transcribe-engine-vulkan"),
        marker: dub_tcpp::LIBRARY,
        pick: &[],
        vram_gb: None,
        note: "Runs Parakeet on an AMD or Intel card, or an NVIDIA card CUDA does not run on.",
    },
    #[cfg(any(windows, target_os = "linux"))]
    Asset {
        id: "transcribe-engine-cpu",
        label: "transcribe.cpp (processor)",
        kind: AssetKind::Runtime,
        url: dub_tcpp::Build::Cpu.download().url,
        relative_path: TCPP_CPU,
        bytes: dub_tcpp::Build::Cpu.download().size,
        unzip_into: Some("transcribe-engine-cpu"),
        marker: dub_tcpp::LIBRARY,
        pick: &[],
        vram_gb: None,
        note: "Runs Parakeet on the processor: slower, the same words.",
    },
    #[cfg(target_os = "macos")]
    Asset {
        id: "transcribe-engine-cpu",
        label: "transcribe.cpp (Metal)",
        kind: AssetKind::Runtime,
        url: dub_tcpp::Build::Cpu.download().url,
        relative_path: TCPP_CPU,
        bytes: dub_tcpp::Build::Cpu.download().size,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Runs Parakeet on the Mac: Metal on Apple silicon, the processor on Intel.",
    },
    #[cfg(windows)]
    Asset {
        id: "onnxruntime",
        label: "ONNX Runtime 1.31.1",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-1.31.1.zip",
        relative_path: "runtime/onnxruntime.zip",
        bytes: 82_929_944,
        unzip_into: Some("onnx"),
        marker: "onnxruntime.dll",
        pick: &[],
        vram_gb: None,
        note: "Tempo and key run on this, on the processor; it is loaded at run time, not linked in.",
    },
    #[cfg(target_os = "macos")]
    Asset {
        id: "onnxruntime",
        label: "ONNX Runtime 1.31.1",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-osx-arm64-1.31.1.tgz",
        relative_path: "runtime/onnxruntime.tgz",
        bytes: 36_936_435,
        unzip_into: Some("onnx"),
        marker: "libonnxruntime",
        pick: &[],
        vram_gb: None,
        note: "Tempo and key run on this, on the processor; it is loaded at run time, not linked in.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "onnxruntime",
        label: "ONNX Runtime 1.31.1",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-linux-x64-1.31.1.tgz",
        relative_path: "runtime/onnxruntime.tgz",
        bytes: 11_413_655,
        unzip_into: Some("onnx"),
        marker: "libonnxruntime.so",
        pick: &[],
        vram_gb: None,
        note: "Tempo and key run on this, on the processor; it is loaded at run time, not linked in.",
    },
];

/// The file name of this platform's ONNX Runtime.
#[cfg(target_os = "macos")]
mod libraries {
    pub const ORT: &str = "libonnxruntime.dylib";
}

#[cfg(windows)]
mod libraries {
    pub const ORT: &str = "onnxruntime.dll";
}

#[cfg(target_os = "linux")]
mod libraries {
    pub const ORT: &str = "libonnxruntime.so";
}

/// Which build of the ONNX Runtime to load.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnnxFlavour {
    /// The graphics card if its runtime is installed, otherwise the processor.
    Auto,
    /// The default: this studio exists for machines with an NVIDIA card, and
    /// the processor path is minutes where the card is seconds.
    #[default]
    Cuda,
    Cpu,
}


impl OnnxFlavour {
    /// Whether work runs on the card through CUDA: only an NVIDIA card with a
    /// driver that runs CUDA does. An AMD card was sent to the CUDA provider
    /// and failed, after being offered gigabytes of CUDA libraries.
    pub fn uses_cuda(self) -> bool {
        !matches!(self, OnnxFlavour::Cpu) && crate::cuda_build::current().is_some()
    }
}

/// The words Parakeet hears in one track, each with the second it starts. `language` is a hint the model takes
/// only when it knows it; none, or one it does not, leaves the language to it.
fn parakeet_transcribe(asr: &std::sync::Mutex<dub_tcpp::TcppAsr>, model: &dub_tcpp::Model, audio: &Path, language: Option<&str>) -> Result<Vec<(f64, String)>> {
    let samples = crate::audio_pcm::decode_mono_16k(audio).with_context(|| format!("decode {} for recognition", audio.display()))?;
    let language = language.filter(|language| dub_tcpp::knows(model, language)).unwrap_or("");
    let heard = asr.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).words(&samples, language).map_err(|error| anyhow!("Parakeet transcription failed: {error}"))?;
    let words: Vec<(f64, String)> = heard
        .into_iter()
        .filter_map(|word| {
            let text = word.word.trim().to_string();
            (!text.is_empty()).then_some((word.start, text))
        })
        .collect();
    if words.is_empty() {
        bail!(NO_WORDS);
    }
    Ok(words)
}

fn point_ort_at(runtime: &Path) {
    unsafe { std::env::set_var("ORT_DYLIB_PATH", runtime) };
    #[cfg(windows)]
    if let Some(directory) = runtime.parent() {
        let existing = std::env::var("PATH").unwrap_or_default();
        unsafe { std::env::set_var("PATH", format!("{};{existing}", directory.display())) };
    }
}

/// The dropdown's Parakeet ids, as settings have held them since the ONNX builds: the v3 model, Ultra, and the
/// fp32 choice of before, which is the v3 model now that full weights are not kept.
pub const PARAKEET_V3: &str = "parakeet-tdt-int8";
pub const PARAKEET_ULTRA: &str = "parakeet-ultra-int8";
pub const PARAKEET_FP32: &str = "parakeet-tdt-fp32";

/// Where each transcribe.cpp build's download lands in the karaoke runtime. Windows and Linux fetch the engine-0.2.0
/// archive (the library and a ggml-cpu per instruction level, so any x86-64 runs it) and unpack it into the build's
/// folder under `runtime/`; the Mac fetches the single dylib straight into its folder. The loader opens
/// `dub_tcpp::LIBRARY` in `runtime_dir(build.dir())`.
#[cfg(windows)]
const TCPP_CUDA: &str = "runtime/transcribe-cuda.zip";
#[cfg(windows)]
const TCPP_VULKAN: &str = "runtime/transcribe-vulkan.zip";
#[cfg(windows)]
const TCPP_CPU: &str = "runtime/transcribe-cpu.zip";
#[cfg(target_os = "linux")]
const TCPP_CUDA: &str = "runtime/transcribe-cuda.tar.gz";
#[cfg(target_os = "linux")]
const TCPP_VULKAN: &str = "runtime/transcribe-vulkan.tar.gz";
#[cfg(target_os = "linux")]
const TCPP_CPU: &str = "runtime/transcribe-cpu.tar.gz";
#[cfg(target_os = "macos")]
const TCPP_CPU: &str = "runtime/transcribe-engine-cpu/libtranscribe_engine.dylib";

/// The transcribe.cpp model the dropdown names.
pub fn parakeet_model(variant: Option<&str>) -> &'static dub_tcpp::Model {
    let id = if variant == Some(PARAKEET_ULTRA) { "parakeet-ultra" } else { "parakeet-v3" };
    dub_tcpp::model(id).expect("the shared binding carries both Parakeets")
}

/// The transcribe.cpp builds loaded in this process, by folder, with the release they were loaded from.
fn loaded_engines() -> &'static std::sync::Mutex<std::collections::HashMap<&'static str, String>> {
    static LOADED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<&'static str, String>>> = std::sync::OnceLock::new();
    LOADED.get_or_init(Default::default)
}

/// The asset of the model the dropdown names.
pub fn parakeet_model_asset(variant: Option<&str>) -> &'static str {
    if variant == Some(PARAKEET_ULTRA) { PARAKEET_ULTRA } else { PARAKEET_V3 }
}

/// The engine build this machine runs Parakeet on: the processor when chosen, CUDA on a card and driver CUDA 13
/// runs on (the build is CUDA 13; on Linux only when the system has its cuBLAS), Vulkan on any other card, the
/// processor without one. A Mac has one build, in the processor's place: Metal and the processor on Apple silicon,
/// the processor on Intel.
pub fn parakeet_build(runtime: OnnxFlavour) -> Option<dub_tcpp::Build> {
    if cfg!(target_os = "macos") {
        return Some(dub_tcpp::Build::Cpu);
    }
    if !cfg!(any(windows, target_os = "linux")) {
        return None;
    }
    let cuda = crate::cuda_build::current();
    Some(if matches!(runtime, OnnxFlavour::Cpu) {
        dub_tcpp::Build::Cpu
    } else if cuda == Some(crate::cuda_build::CudaBuild::Cuda13) {
        if cfg!(target_os = "linux") && !system_library("libcublas.so.13") { dub_tcpp::Build::Vulkan } else { dub_tcpp::Build::Cuda }
    } else if cuda.is_some() || crate::presets::display_card().is_some() {
        dub_tcpp::Build::Vulkan
    } else {
        dub_tcpp::Build::Cpu
    })
}

/// Whether Linux's loader finds `name` without help: the CUDA build of transcribe.cpp looks for cuBLAS beside itself
/// and then where the system keeps libraries, as the music engine does, and the studio downloads none for Linux.
fn system_library(name: &str) -> bool {
    static FOUND: std::sync::OnceLock<std::collections::HashMap<String, bool>> = std::sync::OnceLock::new();
    let probe = |name: &str| {
        let listed = std::env::var_os("LD_LIBRARY_PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()));
        let cached = fs::read("/etc/ld.so.cache").is_ok_and(|cache| cache.windows(name.len()).any(|bytes| bytes == name.as_bytes()));
        listed || cached || ["/lib", "/usr/lib", "/lib64", "/usr/lib64", "/usr/lib/x86_64-linux-gnu"].iter().any(|dir| Path::new(dir).join(name).is_file())
    };
    FOUND.get_or_init(|| ["libcublas.so.13"].into_iter().map(|library| (library.to_string(), probe(library))).collect()).get(name).copied().unwrap_or_else(|| probe(name))
}

/// The files Parakeet runs from on this machine: its engine build and the model the dropdown names.
pub fn parakeet_assets(runtime: OnnxFlavour, variant: Option<&str>) -> Vec<&'static str> {
    parakeet_build(runtime).map(|build| vec![build.dir(), parakeet_model_asset(variant)]).unwrap_or_default()
}

pub fn asset(id: &str) -> Option<&'static Asset> {
    assets().iter().find(|asset| asset.id == id)
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    pub enabled: bool,
    pub provider: AsrProvider,
    pub root: String,
    /// True when the selected provider can actually run right now.
    pub ready: bool,
    pub whisper_binary: Option<String>,
    pub whisper_model: Option<String>,
    pub openrouter_model: Option<String>,
    /// What the recogniser runs on, so the page that downloads it can show the
    /// same choice that decides which files it fetches.
    pub runtime: OnnxFlavour,
    pub installed_models: Vec<String>,
    pub assets: Vec<crate::downloads::AssetStatus>,
    pub active_download: Option<crate::downloads::DownloadProgress>,
    /// The recogniser's engine was updated while this process holds the earlier one: the new one runs from the next
    /// start of the studio.
    pub restart_to_apply: bool,
}

pub struct LyricsSync {
    downloader: Downloader,
    /// audio.cpp's server, installed in this folder: CrisperWhisper's, and the stems' and MIDI's.
    acpp: std::sync::Arc<crate::acpp::Acpp>,
}

impl LyricsSync {
    pub fn new(data_root: &Path) -> Self {
        let downloader = Downloader::new(data_root.join("karaoke"));
        let acpp = std::sync::Arc::new(crate::acpp::Acpp::new(downloader.clone()));
        Self { downloader, acpp }
    }

    /// The studio's audio.cpp server.
    pub fn acpp(&self) -> std::sync::Arc<crate::acpp::Acpp> {
        self.acpp.clone()
    }

    /// The folder of the cuBLAS 13 the music engine runs on: transcribe.cpp's and audio.cpp's CUDA builds take links to it.
    pub fn set_cuda_libraries(&self, folder: PathBuf) {
        self.acpp.set_cuda13_libraries(folder);
    }

    fn link_cuda13(&self, engine_dir: &Path, who: &str) -> Result<()> {
        crate::acpp::link_cuda13(self.acpp.cuda13_libraries(), engine_dir, who)
    }

    pub fn downloader(&self) -> &Downloader {
        &self.downloader
    }

    /// The audio.cpp server of the build `runtime` runs on, when it is downloaded.
    pub fn crisper_server_exe(&self, runtime: OnnxFlavour) -> Option<PathBuf> {
        self.acpp.exe(crate::acpp::build_for(runtime).ok()?)
    }

    /// CrisperWhisper runs here: the server of this choice's build, the CUDA libraries it does not carry and every
    /// file of the model are on disk.
    pub fn crisper_ready(&self, runtime: OnnxFlavour) -> bool {
        let wanted = crisper_assets(runtime);
        !wanted.is_empty() && wanted.iter().all(|id| asset(id).is_some_and(|asset| self.downloader.is_installed(asset)))
    }

    /// `work` on the server of `runtime`'s build carrying CrisperWhisper (`Acpp::with_server`). An error, before the
    /// server is looked up, when a part of CrisperWhisper is not downloaded.
    fn with_crisper<T>(&self, runtime: OnnxFlavour, work: impl FnOnce(&acpp_server::Server) -> Result<T>) -> Result<T> {
        let build = crate::acpp::build_for(runtime)?;
        if !self.crisper_ready(runtime) {
            bail!("CrisperWhisper is not completely downloaded into {}: download it in Settings - Models", self.downloader.root().join(CRISPERWHISPER_DIR).display());
        }
        let (entry, files) = self.crisper_entry();
        self.acpp.with_server(build, entry, files, None, work)
    }

    /// CrisperWhisper as audio.cpp's server carries it, with the files it is complete with.
    fn crisper_entry(&self) -> (acpp_server::ModelEntry, Vec<PathBuf>) {
        let model = self.downloader.root().join(CRISPERWHISPER_DIR);
        let files = OWN.iter().filter(|asset| asset.relative_path.starts_with(CRISPERWHISPER_DIR)).map(|asset| self.downloader.root().join(asset.relative_path)).collect();
        (acpp_server::ModelEntry::crisperwhisper(CRISPERWHISPER, &model), files)
    }

    /// Registers CrisperWhisper with audio.cpp's server at start, beside the stems' and MIDI's models.
    pub fn register_crisper(&self) {
        let (entry, files) = self.crisper_entry();
        self.acpp.register(entry, files);
    }

    pub fn installed_models(&self) -> Vec<&'static Asset> {
        OWN
            .iter()
            .filter(|asset| asset.kind == AssetKind::Model && self.downloader.is_installed(asset))
            .collect()
    }

    /// Parakeet runs here: this machine's engine build and the model the dropdown names are on disk.
    pub fn parakeet_ready(&self, runtime: OnnxFlavour, model: Option<&str>) -> bool {
        let wanted = parakeet_assets(runtime, model);
        !wanted.is_empty() && wanted.iter().all(|id| asset(id).is_some_and(|asset| self.downloader.is_installed(asset)))
    }

    /// Any Parakeet whole on disk, whichever build and variant.
    pub fn parakeet_any_ready(&self) -> bool {
        let installed = |id: &str| asset(id).is_some_and(|asset| self.downloader.is_installed(asset));
        dub_tcpp::Build::ALL.iter().any(|build| installed(build.dir())) && [PARAKEET_V3, PARAKEET_ULTRA].iter().any(|id| installed(id))
    }

    /// A Parakeet whole on disk for this machine's build, v3 first: the dropdown id to choose when the one chosen is
    /// not there.
    pub fn installed_parakeet(&self, runtime: OnnxFlavour) -> Option<&'static str> {
        [PARAKEET_V3, PARAKEET_ULTRA].into_iter().find(|model| self.parakeet_ready(runtime, Some(model)))
    }

    /// The loaded Parakeet of this machine's build and the dropdown's model: one load serves every track of a run;
    /// the run unloads it after, so the card is the music engine's again.
    fn load_parakeet(&self, runtime: OnnxFlavour, variant: Option<&str>) -> Result<(std::sync::Arc<std::sync::Mutex<dub_tcpp::TcppAsr>>, &'static dub_tcpp::Model)> {
        let build = parakeet_build(runtime).ok_or_else(|| anyhow!("transcribe.cpp has no build for this system: choose Whisper"))?;
        let root = self.downloader.root().join("models");
        let model = parakeet_model(variant);
        let engine = self.downloader.runtime_dir(build.dir());
        if build == dub_tcpp::Build::Cuda {
            self.link_cuda13(&engine, "transcribe.cpp")?;
        }
        let asr = dub_tcpp::shared(&engine, &dub_tcpp::model_path(&root, model)).map_err(|error| anyhow!("load Parakeet: {error}"))?;
        // the library stays in the process until it ends: an install over it is used from the next start
        if let Some(record) = self.engine_record(build).and_then(|record| crate::install_record::read(&record)) {
            loaded_engines().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).entry(build.dir()).or_insert(record.url);
        }
        Ok((asr, model))
    }

    /// The install record of a transcribe.cpp build's archive.
    fn engine_record(&self, build: dub_tcpp::Build) -> Option<PathBuf> {
        assets().iter().find(|asset| asset.unzip_into == Some(build.dir())).map(|asset| self.downloader.record_of(asset))
    }

    /// A transcribe.cpp build this process has loaded whose files on disk are now another release's: the process keeps
    /// the one it loaded (a loaded build is never unloaded), so the new one runs from the next start of the studio.
    pub fn restart_to_apply(&self) -> bool {
        let loaded = loaded_engines().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        dub_tcpp::Build::ALL.iter().any(|build| {
            loaded.get(build.dir()).is_some_and(|url| {
                self.engine_record(*build).and_then(|record| crate::install_record::read(&record)).is_some_and(|now| now.url != *url)
            })
        })
    }

    /// `ort` loads this at run time; linking it would tie the build to one
    /// toolchain and one machine's libraries. The processor build at today's
    /// pin: an earlier one left on disk is not loaded.
    pub fn onnxruntime_library(&self) -> Option<PathBuf> {
        let cpu = self.downloader.runtime_dir("onnx").join(libraries::ORT);
        (cpu.is_file() && self.pinned(&["onnxruntime"])).then_some(cpu)
    }

    /// Whether every one of these downloads is installed at today's pin.
    fn pinned(&self, ids: &[&str]) -> bool {
        ids.iter().all(|id| asset(id).is_some_and(|asset| self.downloader.is_installed(asset)))
    }

    /// Binds `ort` to the studio's processor runtime - once per process, before
    /// anything touches `ort`, or it binds to whatever `onnxruntime.dll` the
    /// system happens to have. Every piece of work on ONNX asks here first. The
    /// library is loaded here, not on first use: one this process cannot load
    /// (another system's build, a signature macOS refuses) is then an error of
    /// the work that needed it, where `ort` would panic and stay unusable until
    /// the studio restarts.
    pub fn bind_ort(&self) -> Result<()> {
        static BOUND: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        if BOUND.get().is_some() {
            return Ok(());
        }
        let library = self.onnxruntime_library().ok_or_else(|| anyhow!("the ONNX Runtime library is not installed"))?;
        point_ort_at(&library);
        // the process's one environment, telemetry off, over the library pointed at above
        dub_core::runtime::ort_env().map_err(|error| anyhow!(error))?;
        BOUND.get_or_init(|| ());
        Ok(())
    }

    /// A model counts as present only with every one of its files: a directory
    /// missing its tokenizer loads exactly as far as an error message.
    pub async fn status(&self, config: &LyricsSyncConfig) -> SyncStatus {
        let whisper_binary = self.crisper_server_exe(config.runtime);
        let ready = match config.provider {
            AsrProvider::None => false,
            AsrProvider::Whisper => self.crisper_ready(config.runtime),
            AsrProvider::Parakeet => self.parakeet_ready(config.runtime, config.whisper_model.as_deref()),
            AsrProvider::OpenRouter => config.openrouter_model.as_deref().is_some_and(|model| !model.trim().is_empty()),
        };
        SyncStatus {
            enabled: config.enabled,
            provider: config.provider,
            runtime: config.runtime,
            root: self.downloader.root().display().to_string(),
            ready: config.enabled && ready,
            whisper_binary: whisper_binary.map(|path| path.display().to_string()),
            whisper_model: config.whisper_model.clone(),
            openrouter_model: config.openrouter_model.clone(),
            installed_models: self.installed_models().iter().map(|asset| asset.id.to_string()).collect(),
            assets: self.downloader.status_of(assets()),
            active_download: self.downloader.active_for("karaoke").await,
            restart_to_apply: self.restart_to_apply(),
        }
    }

    /// Runs Parakeet in this process and returns the words it hears, each with the second it starts: transcribe.cpp
    /// on this machine's build, as Dub Studio and Speech Studio run it.
    pub fn parakeet_words(&self, runtime: OnnxFlavour, variant: Option<&str>, audio: &Path, language: Option<&str>) -> Result<Vec<(f64, String)>> {
        let (asr, model) = self.load_parakeet(runtime, variant)?;
        let words = parakeet_transcribe(&asr, model, audio, language);
        drop(asr);
        dub_tcpp::unload();
        words
    }

    /// The words of several tracks from one load of the recogniser: Parakeet
    /// loaded once and run over each, Whisper started once with every file.
    /// `heard` gets each track's answer, with its index, the moment it is
    /// there, so a long dataset shows its lyrics song by song.
    pub fn words_many(
        &self,
        config: &LyricsSyncConfig,
        audio: &[PathBuf],
        language: Option<&str>,
        heard: &mut dyn FnMut(usize, Result<Vec<(f64, String)>>),
        cancel: &AtomicBool,
    ) -> Recognised {
        let failed = |heard: &mut dyn FnMut(usize, Result<Vec<(f64, String)>>), error: anyhow::Error| {
            for index in 0..audio.len() {
                heard(index, Err(anyhow!("{error:#}")));
            }
        };
        match config.provider {
            AsrProvider::Parakeet => {
                let (asr, model) = match self.load_parakeet(config.runtime, config.whisper_model.as_deref()) {
                    Ok(loaded) => loaded,
                    Err(error) => {
                        failed(heard, error);
                        return Recognised::OnDevice;
                    }
                };
                for (index, path) in audio.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    heard(index, parakeet_transcribe(&asr, model, path, language));
                }
                // the card is the music engine's between recognitions
                drop(asr);
                dub_tcpp::unload();
                Recognised::OnDevice
            }
            AsrProvider::Whisper => {
                // the server ends after the run, so the card is the music engine's between recognitions
                let run = self.with_crisper(config.runtime, |server| {
                    for (index, path) in audio.iter().enumerate() {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        heard(index, self.crisper_transcribe(server, path, language));
                    }
                    Ok(())
                });
                if let Err(error) = run {
                    failed(heard, error);
                }
                Recognised::OnDevice
            }
            _ => {
                failed(heard, anyhow!("this recogniser does not run on this computer"));
                Recognised::OnDevice
            }
        }
    }

    /// The words CrisperWhisper hears in one track, each with the second it starts, on `runtime`'s build of the
    /// audio.cpp server; the server leaves the card afterwards when nothing else runs on it.
    pub fn crisper_words(&self, runtime: OnnxFlavour, audio: &Path, language: Option<&str>) -> Result<Vec<(f64, String)>> {
        self.with_crisper(runtime, |server| self.crisper_transcribe(server, audio, language))
    }

    /// One track through the running server: decoded to the 16 kHz mono WAV the server reads, told its language when
    /// it is known ("auto" or none lets the model hear it), and what a hallucinated segment holds left out.
    fn crisper_transcribe(&self, server: &acpp_server::Server, audio: &Path, language: Option<&str>) -> Result<Vec<(f64, String)>> {
        let work = self.downloader.root().join("work");
        fs::create_dir_all(&work).with_context(|| format!("create {}", work.display()))?;
        let told = match language.map(str::trim).filter(|code| !code.is_empty() && *code != "auto") {
            Some(code) => Some(acpp_server::language::whisper_code(code).map_err(|error| anyhow!("{error}"))?.to_string()),
            None => None,
        };
        let wav = work.join(format!("sync-{}.wav", uuid::Uuid::now_v7()));
        if let Err(error) = crate::audio_pcm::write_wav16k_mono(audio, &wav) {
            fs::remove_file(&wav).ok();
            return Err(error.context(format!("decode {} for recognition", audio.display())));
        }
        let request = acpp_server::TranscribeRequest {
            model: CRISPERWHISPER.to_string(),
            wav: wav.clone(),
            language: told,
            prompt: None,
            options: serde_json::Map::new(),
            within: None,
        };
        let transcript = server.transcribe_details(&request);
        fs::remove_file(&wav).ok();
        let words = heard_words(&transcript.map_err(|error| anyhow!("CrisperWhisper: {error}"))?);
        if words.is_empty() {
            bail!(NO_WORDS);
        }
        Ok(words)
    }
}

/// Groups a word stream into lines: a sentence's end closes its line; a pause
/// between the starts of two words opens a new one, 1.2 s anywhere (a held
/// note is not a pause) or 0.6 s after a comma, except before the word that
/// ends the sentence within 3 s; a line past 48 characters is
/// cut after its last comma, else before its last word. Words of a script
/// written without spaces (Chinese, Japanese) join without them, and the
/// length counts characters either way.
pub fn group_words(words: &[(f64, String)]) -> Vec<(f64, String)> {
    const PAUSE: f64 = 1.2;
    const PAUSE_AFTER_COMMA: f64 = 0.6;
    const HELD_NOTE: f64 = 3.0;

    let mut lines: Vec<(f64, String)> = Vec::new();
    let mut current: Vec<(f64, &str)> = Vec::new();
    for (at, word) in words {
        let word = word.as_str();
        if let Some(&(last_at, last)) = current.last() {
            let after_comma = ends_clause(last) || starts_with_any(word, CLAUSE_ENDS);
            let wait = if after_comma { PAUSE_AFTER_COMMA } else { PAUSE };
            // a word that ends the sentence stays with it however long the note before it was held;
            // punctuation a recogniser hangs at the start of the next word ends the line before it
            let paused = at - last_at > wait && !(ends_sentence(word) && at - last_at < HELD_NOTE);
            if paused || starts_with_any(word, SENTENCE_ENDS) {
                lines.push(joined_line(&std::mem::take(&mut current)));
            }
        }
        current.push((*at, word));
        if ends_sentence(word) {
            lines.push(joined_line(&std::mem::take(&mut current)));
            continue;
        }
        while current.len() > 1 && joined_line(&current).1.chars().count() > LINE_CHARS {
            let before_last = current.len() - 1;
            let cut = current[..before_last].iter().rposition(|(_, word)| ends_clause(word)).map_or(before_last, |index| index + 1);
            lines.push(joined_line(&current[..cut]));
            current.drain(..cut);
        }
    }
    if !current.is_empty() {
        lines.push(joined_line(&current));
    }
    merge_flashing_lines(lines)
}

/// The longest line, in characters, that reads at a glance.
const LINE_CHARS: usize = 48;
const SENTENCE_ENDS: &[char] = &['.', '!', '?', '…', '。', '！', '？'];
const CLAUSE_ENDS: &[char] = &[',', ';', '，', '、', '；'];
const CLOSING_QUOTES: &[char] = &['"', '\'', '»', '”', '’', ')', '」', '』', '）'];

/// Whether a word ends a sentence, a closing quote or bracket after the mark aside.
fn ends_sentence(word: &str) -> bool {
    word.trim_end().trim_end_matches(CLOSING_QUOTES).ends_with(SENTENCE_ENDS)
}

/// Whether a word ends a clause with a comma or its like.
fn ends_clause(word: &str) -> bool {
    word.trim_end().trim_end_matches(CLOSING_QUOTES).ends_with(CLAUSE_ENDS)
}

fn starts_with_any(word: &str, marks: &[char]) -> bool {
    word.trim_start().starts_with(marks)
}

/// A character of a script written without spaces between words.
fn unspaced(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' | '\u{3000}'..='\u{303F}' | '\u{FF00}'..='\u{FFEF}')
}

/// The words of a line as text, starting at its first word.
fn joined_line(words: &[(f64, &str)]) -> (f64, String) {
    let mut text = String::new();
    for (_, word) in words {
        let word = word.trim();
        let glued = text.chars().last().is_some_and(unspaced) && word.chars().next().is_some_and(unspaced);
        if !text.is_empty() && !glued {
            text.push(' ');
        }
        text.push_str(word);
    }
    (words.first().map_or(0.0, |(at, _)| *at), text)
}

/// ACE Step Studio merged LRC lines that begin less than two seconds apart so
/// they do not flash past unread. Karaoke here follows the same rule, except
/// across the end of a sentence.
fn merge_flashing_lines(lines: Vec<(f64, String)>) -> Vec<(f64, String)> {
    const MIN_DISPLAY_SECONDS: f64 = 2.0;
    let mut merged: Vec<(f64, String)> = Vec::with_capacity(lines.len());
    for (start, text) in lines {
        match merged.last_mut() {
            Some((previous_start, previous_text))
                if start - *previous_start < MIN_DISPLAY_SECONDS
                    && !ends_sentence(previous_text)
                    && previous_text.chars().count() + 1 + text.chars().count() <= LINE_CHARS =>
            {
                if !(previous_text.chars().last().is_some_and(unspaced) && text.chars().next().is_some_and(unspaced)) {
                    previous_text.push(' ');
                }
                previous_text.push_str(&text);
            }
            _ => merged.push((start, text)),
        }
    }
    merged
}

/// The timed segments in an OpenAI-compatible verbose transcription. A model
/// that answered with plain text yields nothing, and the caller reports that
/// rather than inventing timings.
pub fn segments_from_verbose_json(body: &serde_json::Value) -> Vec<(f64, String)> {
    let mut words = Vec::new();
    if let Some(list) = body.get("words").and_then(|value| value.as_array()) {
        for entry in list {
            let (Some(start), Some(text)) = (entry.get("start").and_then(serde_json::Value::as_f64), entry.get("word").or_else(|| entry.get("text")).and_then(serde_json::Value::as_str)) else {
                continue;
            };
            words.push((start, text.trim().to_string()));
        }
    }
    if !words.is_empty() {
        return group_words(&words);
    }
    let mut segments = Vec::new();
    if let Some(list) = body.get("segments").and_then(|value| value.as_array()) {
        for entry in list {
            let (Some(start), Some(text)) = (entry.get("start").and_then(serde_json::Value::as_f64), entry.get("text").and_then(serde_json::Value::as_str)) else {
                continue;
            };
            segments.push((start, text.trim().to_string()));
        }
    }
    segments
}

/// One written line, with a time for every word in it.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedLine {
    pub start: f64,
    /// The words of the written line, each with the moment it is sung.
    pub words: Vec<(f64, String)>,
}

/// Enhanced LRC - the A2 format every karaoke player understands: a line time
/// followed by a time before each word. Without per-word times a player has
/// nothing to do but sweep the highlight linearly, which drifts away from the
/// singing within a line.
pub fn enhanced_lrc(lines: &[TimedLine]) -> String {
    let mut out = String::new();
    for line in lines {
        if line.words.is_empty() {
            continue;
        }
        out.push_str(&format!("[{}]", stamp(line.start)));
        for (at, word) in &line.words {
            out.push_str(&format!("<{}>{} ", stamp(*at), word));
        }
        out.pop();
        out.push('\n');
    }
    out
}

fn stamp(seconds: f64) -> String {
    let total = seconds.max(0.0);
    let minutes = (total as u64) / 60;
    let rest = total - (minutes * 60) as f64;
    format!("{minutes:02}:{rest:05.2}")
}

/// Puts the track's own lyrics on the recogniser's clock, word by word.
///
/// The line anchoring is the same as before; inside a line each written word
/// takes the time of the recognised word it matches, and words nobody matched
/// are spread across the gap in proportion to their length. That is what makes
/// a karaoke highlight land on the syllable instead of drifting through it.
pub fn align_lyrics_words(words: &[(f64, String)], lyrics: &str) -> Vec<TimedLine> {
    let anchored = align_lyrics(words, lyrics);
    if anchored.is_empty() {
        return Vec::new();
    }
    let heard: Vec<(f64, String)> = words.iter().map(|(at, word)| (*at, normalise(word))).collect();
    let mut timed: Vec<TimedLine> = Vec::with_capacity(anchored.len());

    for (index, (start, text)) in anchored.iter().enumerate() {
        let end = anchored.get(index + 1).map(|(next, _)| *next).unwrap_or_else(|| {
            // The last line runs to the last thing anyone heard.
            heard.last().map(|(at, _)| *at + 2.0).unwrap_or(start + 4.0)
        });
        let written: Vec<&str> = text.split_whitespace().collect();
        if written.is_empty() {
            continue;
        }

        // The recognised words that fall inside this line's span.
        let inside: Vec<&(f64, String)> = heard.iter().filter(|(at, _)| *at >= *start - 0.01 && *at < end).collect();
        let mut placed: Vec<Option<f64>> = vec![None; written.len()];
        let mut cursor = 0usize;
        for (position, word) in written.iter().enumerate() {
            let expected = normalise(word);
            if expected.is_empty() {
                continue;
            }
            if let Some(found) = inside[cursor..].iter().position(|(_, heard_word)| {
                // Compare by characters, never by bytes: slicing "неон" at a
                // byte index lands inside a letter and panics.
                heard_word == &expected || (expected.chars().count() > 3 && heard_word.starts_with(&stem(&expected)))
            }) {
                placed[position] = Some(inside[cursor + found].0);
                cursor += found + 1;
            }
        }

        // Anything unmatched is spread by length across the gap it sits in.
        let mut previous_time = *start;
        let mut position = 0usize;
        while position < written.len() {
            if let Some(at) = placed[position] {
                previous_time = at;
                position += 1;
                continue;
            }
            let gap_start = position;
            while position < written.len() && placed[position].is_none() {
                position += 1;
            }
            let next_time = placed.get(position).copied().flatten().unwrap_or(end);
            let span = (next_time - previous_time).max(0.05);
            let weight: usize = written[gap_start..position].iter().map(|word| word.chars().count().max(1)).sum();
            let mut used = 0usize;
            for offset in gap_start..position {
                let length = written[offset].chars().count().max(1);
                // Centred in its own share of the gap: a word placed exactly on
                // the previous word's time would highlight two words at once.
                let share = (used as f64 + length as f64 / 2.0) / weight as f64;
                placed[offset] = Some(previous_time + span * share);
                used += length;
            }
            previous_time = next_time;
        }

        timed.push(TimedLine {
            start: *start,
            words: written
                .iter()
                .zip(placed)
                .map(|(word, at)| (at.unwrap_or(*start), (*word).to_string()))
                .collect(),
        });
    }
    timed
}

/// Puts the track's own lyrics on the recogniser's clock.
///
/// The words are already known - the model sang what it was given - so the
/// recogniser is used for *timing only*, which is what every karaoke aligner
/// worth the name does. Its text is mistrusted: sung vocals are mis-heard
/// constantly, and printing that back as lyrics is how karaoke ends up
/// showing nonsense. All lines are placed at once by a monotonic alignment
/// that maximises how well they match overall: a line matched greedily to a
/// later repeat of itself would strand every line sung in between, so a line
/// that was not heard clearly is left out and filled in between its
/// neighbours instead. A repeated chorus consumes its occurrences in order.
pub fn align_lyrics(words: &[(f64, String)], lyrics: &str) -> Vec<(f64, String)> {
    align_lyrics_with(words, lyrics, GAPS)
}

/// What the alignment pays for heard words it leaves without a line, as the
/// gap penalty of a sequence alignment: a line placed past a sung stretch the
/// text does not cover costs `per_word` for each word skipped, so leaping over
/// a verse for a line matched only in part costs more than that match brings.
/// `leading` counts the words before the first placed line as well.
#[derive(Debug, Clone, Copy)]
pub struct Gaps {
    pub per_word: f64,
    pub leading: bool,
}

pub const GAPS: Gaps = Gaps { per_word: 0.05, leading: true };

pub fn align_lyrics_with(words: &[(f64, String)], lyrics: &str, gaps: Gaps) -> Vec<(f64, String)> {
    let lines: Vec<&str> = lyrics
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !(line.starts_with('[') && line.ends_with(']')))
        .collect();
    if lines.is_empty() || words.is_empty() {
        return Vec::new();
    }

    let heard: Vec<(f64, String)> = words.iter().map(|(at, word)| (*at, normalise(word))).collect();
    let count = heard.len();
    // How well each line matches when it starts at each recognised word; below
    // the threshold a start is not a candidate at all.
    let spans: Vec<usize> = lines
        .iter()
        .map(|line| line.split_whitespace().filter(|word| !normalise(word).is_empty()).count())
        .collect();
    let scores: Vec<Vec<f64>> = lines
        .iter()
        .zip(&spans)
        .map(|(line, &span)| {
            let written: String = line.split_whitespace().map(normalise).collect();
            (0..count)
                .map(|start| {
                    if span == 0 {
                        return 0.0;
                    }
                    // A little slack in length, because the recogniser splits
                    // words differently from the page; each window is scored by
                    // what it misses and what it adds, so starting a word early
                    // costs as much as starting a word late.
                    let score = (span.saturating_sub(1).max(1)..=span + 2)
                        .filter(|length| start + length <= count)
                        .map(|length| {
                            let spoken: String = heard[start..start + length].iter().map(|(_, word)| word.as_str()).collect();
                            dice(&written, &spoken)
                        })
                        .fold(0.0, f64::max);
                    if score >= 0.45 { score } else { 0.0 }
                })
                .collect()
        })
        .collect();

    // best[p]: the best (total, sum of placed line numbers, sum of starts) so
    // far with the next line free to start at word p or later. Each line is
    // either placed at some start >= p, which moves p past it, or left out. A
    // clearly recognised line counts the same wherever it is heard best.
    // Between equal totals the earlier lines of the text win - a verse written
    // out more times than the song sings it is heard as its first copies, so
    // the copies past the song stay unplaced - and then the earlier placement:
    // a chorus the model sang twice in a row shows its lines as they are first
    // sung, not where the recogniser heard them best.
    let credit = |score: f64| if score >= 0.7 { 1.0 } else { score };
    // (total, sum of placed line numbers, sum of starts, whether a line is placed yet)
    type Kept = (f64, usize, usize, bool);
    let better = |left: Kept, right: Kept| {
        left.0 > right.0 + 1e-9 || ((left.0 - right.0).abs() <= 1e-9 && (left.1, left.2) < (right.1, right.2))
    };
    let mut best: Vec<Option<Kept>> = vec![None; count + 1];
    best[0] = Some((0.0, 0, 0, false));
    let mut back: Vec<Vec<(usize, Option<usize>)>> = Vec::with_capacity(lines.len());
    for (index, &span) in spans.iter().enumerate() {
        let mut next: Vec<Option<Kept>> = vec![None; count + 1];
        let mut choice = vec![(0usize, None); count + 1];
        for from in 0..=count {
            let Some(so_far) = best[from] else { continue };
            if next[from].is_none_or(|kept| better(so_far, kept)) {
                next[from] = Some(so_far);
                choice[from] = (from, None);
            }
            if span == 0 {
                continue;
            }
            for start in from..count {
                let score = scores[index][start];
                if score <= 0.0 {
                    continue;
                }
                let after = (start + span).min(count);
                let skipped = if so_far.3 || gaps.leading { start - from } else { 0 };
                let total = (so_far.0 + credit(score) - gaps.per_word * skipped as f64, so_far.1 + index, so_far.2 + start, true);
                if next[after].is_none_or(|kept| better(total, kept)) {
                    next[after] = Some(total);
                    choice[after] = (from, Some(start));
                }
            }
        }
        best = next;
        back.push(choice);
    }

    let mut starts: Vec<Option<usize>> = vec![None; lines.len()];
    let mut position = (0..=count).fold(0, |kept, candidate| match (best[candidate], best[kept]) {
        (Some(offered), Some(held)) if better(offered, held) => candidate,
        (Some(_), None) => candidate,
        _ => kept,
    });
    for index in (0..lines.len()).rev() {
        let (from, start) = back[index][position];
        starts[index] = start;
        position = from;
    }

    // A clearly recognised line was credited in full, so its start may sit as
    // early as the first word it still matches from - a few sounds the
    // recogniser heard before it count too; settle it where it matches best,
    // between its neighbours and within its own length.
    for index in 0..lines.len() {
        let Some(start) = starts[index] else { continue };
        let floor = starts[..index].iter().rev().flatten().next().map(|previous| previous + 1).unwrap_or(0);
        let ceiling = starts[index + 1..].iter().flatten().next().copied().unwrap_or(count);
        let low = start.saturating_sub(2).max(floor);
        let high = (start + spans[index].max(2)).min(ceiling.saturating_sub(1)).min(count.saturating_sub(1));
        let settled = (low..=high).fold(start, |kept, candidate| if scores[index][candidate] > scores[index][kept] { candidate } else { kept });
        starts[index] = Some(settled);
    }
    let mut placed: Vec<Option<f64>> = starts.iter().map(|start| start.map(|start| heard[start].0)).collect();

    // the words heard before the first placed line and after the last one: an
    // edge gap is timed only when the recogniser heard enough there to sing it
    let first_start = starts.iter().flatten().next().copied().unwrap_or(count);
    let after_last = starts
        .iter()
        .zip(&spans)
        .filter_map(|(start, &span)| start.map(|start| (start + span).min(count)))
        .last()
        .unwrap_or(0);
    let edges = Edges {
        before: first_start,
        after: count - after_last.min(count),
        first: heard.first().map(|(at, _)| *at).unwrap_or(0.0),
        last: heard.last().map(|(at, _)| *at).unwrap_or(0.0),
    };
    interpolate(&lines, &mut placed, &edges);
    lines
        .iter()
        .zip(placed)
        .filter_map(|(line, at)| at.map(|at| (at, (*line).to_string())))
        .collect()
}

/// Lines the recogniser could not place are spread evenly between the ones it
/// could, so a karaoke file has no silent holes. A gap before the first placed
/// line or after the last one is timed only when the recogniser heard at least
/// as many unmatched words there as the gap has lines - a first line sung
/// unclearly keeps its place - and otherwise is not in the song as heard (a
/// text longer than what was sung) and gets no time.
fn interpolate(lines: &[&str], placed: &mut [Option<f64>], edges: &Edges) {
    let mut index = 0;
    while index < lines.len() {
        if placed[index].is_some() {
            index += 1;
            continue;
        }
        let gap_start = index;
        while index < lines.len() && placed[index].is_none() {
            index += 1;
        }
        let gap = index - gap_start;
        let before = gap_start.checked_sub(1).and_then(|previous| placed[previous]);
        let after = placed.get(index).copied().flatten();
        let (from, to, leading) = match (before, after) {
            (Some(before), Some(after)) => (before, after, false),
            (None, Some(after)) if edges.before >= gap => (edges.first, after, true),
            (Some(before), None) if edges.after >= gap => (before, edges.last.max(before), false),
            (None, None) if edges.before >= gap => (edges.first, edges.last, true),
            _ => continue,
        };
        // a leading gap starts at the first word heard; any other one starts after the line before it
        let (offset, steps) = if leading { (0.0, gap as f64) } else { (1.0, (gap + 1) as f64) };
        for (position, slot) in placed[gap_start..index].iter_mut().enumerate() {
            *slot = Some(from + (to - from) * ((position as f64 + offset) / steps));
        }
    }
}

/// What the recogniser heard outside the placed lines: how many words before
/// the first one and after the last one, and the times of the first and last word.
struct Edges {
    before: usize,
    after: usize,
    first: f64,
    last: f64,
}

/// A word without its last character, for tolerating inflected endings.
fn stem(word: &str) -> String {
    let count = word.chars().count();
    word.chars().take(count.saturating_sub(1)).collect()
}

fn normalise(word: &str) -> String {
    word.chars().filter(|character| character.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The characters two strings share in order, over both their lengths: 1 for
/// the same text, lower for what either one misses or adds.
fn dice(expected: &str, heard: &str) -> f64 {
    let (left, right) = (expected.chars().count(), heard.chars().count());
    if left + right == 0 {
        return 0.0;
    }
    2.0 * similarity(expected, heard) * left as f64 / (left + right) as f64
}

/// How much of the written line the recogniser heard, compared character by
/// character rather than word by word.
///
/// Sung vocals come back mangled - "on the glass" as "arms the grass" - and a
/// word-level comparison scores that as a miss even though the line is plainly
/// the right one. Comparing characters in order tolerates the mangling, which
/// is what character-level aligners do for exactly this reason.
fn similarity(expected: &str, heard: &str) -> f64 {
    if expected.is_empty() || heard.is_empty() {
        return 0.0;
    }
    let left: Vec<char> = expected.chars().collect();
    let right: Vec<char> = heard.chars().collect();
    let mut previous = vec![0usize; right.len() + 1];
    let mut current = vec![0usize; right.len() + 1];
    for l in 0..left.len() {
        for r in 0..right.len() {
            current[r + 1] = if left[l] == right[r] { previous[r] + 1 } else { current[r].max(previous[r + 1]) };
        }
        std::mem::swap(&mut previous, &mut current);
        current.iter_mut().for_each(|value| *value = 0);
    }
    previous[right.len()] as f64 / left.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model is fetched from a pinned commit: a repository's latest files can be other builds under the same name.
    #[test]
    fn every_model_is_pinned_to_a_commit() {
        for asset in assets().iter().filter(|asset| asset.url.contains("huggingface.co/")) {
            assert!(asset.url.contains("/resolve/") && !asset.url.contains("/resolve/main/"), "{} is not pinned", asset.url);
        }
    }

    #[test]
    fn whisper_fillers_are_dropped_and_sung_lines_kept() {
        assert!(is_hallucination(" Субтитры создавал DimaTorzok"));
        assert!(is_hallucination("Продолжение следует..."));
        assert!(is_hallucination("Thanks for watching!"));
        assert!(is_hallucination("ご視聴ありがとうございました"));
        assert!(is_hallucination("ВЕСЕЛАЯ МУЗЫКА"));
        assert!(is_hallucination("[Music]"));
        assert!(is_hallucination(" 1."));
        assert!(!is_hallucination("Если б мне платили каждый раз,"));
        assert!(!is_hallucination("Спасибо, что ты рядом со мной"));
        assert!(!is_hallucination("Поехали!"));
        let word = |text: &str, start: f64| acpp_server::TranscriptWord { text: text.into(), start, end: start + 0.3, confidence: 1.0 };
        let span = |text: &str, start: f64, end: f64| acpp_server::TranscriptSpan { text: text.into(), start, end, confidence: 1.0 };
        let transcript = acpp_server::Transcript {
            text: String::new(),
            language: Some("ru".into()),
            words: vec![word("Субтитры", 1.0), word("Тьма", 5.0), word("во", 5.4), word("мне", 5.6)],
            segments: vec![span(" Субтитры создавал DimaTorzok", 1.0, 2.0), span(" Тьма во мне", 5.0, 6.0)],
        };
        let words = heard_words(&transcript);
        assert_eq!(words.iter().map(|(_, word)| word.as_str()).collect::<Vec<_>>(), ["Тьма", "во", "мне"]);
        assert_eq!(words[0].0, 5.0);
        let touching = acpp_server::Transcript {
            text: String::new(),
            language: Some("ru".into()),
            words: vec![word("Тьма", 5.2), word("во", 5.6)],
            segments: vec![span("[Music]", 0.0, 5.2), span(" Тьма во", 5.2, 6.0)],
        };
        assert_eq!(heard_words(&touching).len(), 2, "the first word of the line after a filler is sung");
        let instant = acpp_server::Transcript {
            text: String::new(),
            language: Some("ru".into()),
            words: vec![word("Тьма", 12.5), word("во", 20.0), word("мне", 40.0)],
            segments: vec![span("[Music]", 12.0, 12.0), span(" Тьма во мне", 12.5, 41.0)],
        };
        assert_eq!(heard_words(&instant).len(), 3, "a filler of no length takes no later word");
    }

    // The releases every runtime download is pinned to.
    const AUDIOCPP_BUILD: &str = "/audio.cpp/releases/download/server-";
    const SERVER_CUFFT_BUILD: &str = "12.4.0.43";
    const CUBLAS_BUILD: &str = "12.9.2.10";
    const CUDART_BUILD: &str = "12.9.79";
    const CUFFT_BUILD: &str = "11.4.1.4";
    const CUDNN_BUILD: &str = "9.25.0.15";
    const CURAND_BUILD: &str = "10.3.10.19";
    const ONNXRUNTIME_BUILD: &str = "v1.31.1";
    const ONNXRUNTIME_DIRECTML_BUILD: &str = "onnxruntime.directml/1.24.4/";
    const DIRECTML_BUILD: &str = "ai.directml/1.15.4/";

    impl TimedLine {
        fn text(&self) -> String {
            self.words.iter().map(|(_, word)| word.as_str()).collect::<Vec<_>>().join(" ")
        }
    }

    #[test]
    fn words_become_lines_at_pauses_and_never_flash_past() {
        let words = vec![
            (0.0, "neon".into()),
            (0.4, "on".into()),
            (0.8, "the".into()),
            (1.2, "glass".into()),
            // A long pause would start a new line, but it lands inside the two
            // second window ACE used, so it joins the line before it.
            (2.9, "driving".into()),
            (3.3, "home".into()),
            (12.0, "the".into()),
            (12.4, "engine".into()),
            (12.9, "dies".into()),
        ];
        let lines = group_words(&words);
        assert_eq!(
            lines,
            vec![
                (0.0, "neon on the glass".to_string()),
                (2.9, "driving home".to_string()),
                (12.0, "the engine dies".to_string()),
            ]
        );
    }

    fn timed(words: &[(f64, &str)]) -> Vec<(f64, String)> {
        words.iter().map(|(at, word)| (*at, word.to_string())).collect()
    }

    #[test]
    fn a_held_note_does_not_break_a_line() {
        let words = timed(&[(10.0, "Feel"), (10.3, "the"), (10.6, "rhythm"), (11.0, "in"), (11.3, "my"), (12.2, "feet.")]);
        assert_eq!(group_words(&words), vec![(10.0, "Feel the rhythm in my feet.".to_string())]);
        // held past the pause: the word that ends the sentence still stays on its line
        let held = timed(&[(10.0, "Feel"), (10.3, "the"), (10.6, "rhythm"), (11.0, "in"), (11.3, "my"), (13.0, "feet.")]);
        assert_eq!(group_words(&held), vec![(10.0, "Feel the rhythm in my feet.".to_string())]);
    }

    #[test]
    fn a_sentence_end_closes_its_line_even_inside_the_flash_window() {
        let words = timed(&[(20.0, "dancing"), (20.4, "through"), (20.8, "these"), (21.1, "city"), (21.6, "lanes."), (22.2, "Shadows"), (22.6, "moving,"), (23.4, "flashing"), (23.8, "heat.")]);
        assert_eq!(
            group_words(&words),
            vec![(20.0, "dancing through these city lanes.".to_string()), (22.2, "Shadows moving, flashing heat.".to_string())]
        );
    }

    #[test]
    fn a_pause_after_a_comma_is_shorter() {
        let words = timed(&[(0.0, "Take"), (0.3, "my"), (0.6, "hand,"), (1.4, "we'll"), (1.7, "find"), (2.0, "our"), (2.3, "way"), (5.0, "now")]);
        assert_eq!(group_words(&words), vec![(0.0, "Take my hand, we'll find our way".to_string()), (5.0, "now".to_string())]);
        let lines = group_words(&timed(&[(0.0, "Take"), (0.3, "my"), (0.6, "hand,"), (3.0, "we'll"), (3.3, "find")]));
        assert_eq!(lines, vec![(0.0, "Take my hand,".to_string()), (3.0, "we'll find".to_string())]);
    }

    #[test]
    fn a_long_line_without_marks_is_cut_at_a_word() {
        let text = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen";
        let words: Vec<(f64, String)> = text.split(' ').enumerate().map(|(index, word)| (index as f64 * 0.4, word.to_string())).collect();
        let lines = group_words(&words);
        assert!(lines.len() >= 2, "{lines:?}");
        assert!(lines.iter().all(|(_, line)| line.chars().count() <= 48 + "fourteen".len()), "{lines:?}");
        assert_eq!(lines.iter().map(|(_, line)| line.as_str()).collect::<Vec<_>>().join(" "), text);
    }

    #[test]
    fn a_long_line_is_cut_after_its_last_comma() {
        let words = timed(&[(0.0, "Hear"), (0.3, "the"), (0.6, "echoes"), (0.9, "of"), (1.1, "the"), (1.3, "streets,"), (1.6, "whispers"), (1.9, "of"), (2.1, "a"), (2.3, "thousand"), (2.6, "beats"), (2.9, "tonight"), (3.2, "again")]);
        let lines = group_words(&words);
        assert_eq!(lines[0].1, "Hear the echoes of the streets,", "{lines:?}");
        assert_eq!(lines[1].1, "whispers of a thousand beats tonight again", "{lines:?}");
    }

    #[test]
    fn chinese_words_join_without_spaces() {
        let words = timed(&[(0.0, "我"), (0.3, "爱"), (0.6, "你。"), (5.0, "再见")]);
        assert_eq!(group_words(&words), vec![(0.0, "我爱你。".to_string()), (5.0, "再见".to_string())]);
    }

    #[test]
    fn a_verbose_transcription_yields_timed_lines_and_plain_text_yields_none() {
        let verbose = serde_json::json!({
            "text": "neon on the glass",
            "segments": [{ "start": 1.5, "text": " neon on the glass" }, { "start": 9.0, "text": "driving home" }]
        });
        assert_eq!(
            segments_from_verbose_json(&verbose),
            vec![(1.5, "neon on the glass".to_string()), (9.0, "driving home".to_string())]
        );
        assert!(segments_from_verbose_json(&serde_json::json!({ "text": "no timings here" })).is_empty());
    }

    #[test]
    fn the_written_lyrics_are_kept_and_only_the_timing_is_borrowed() {
        // What the recogniser heard: two words wrong, as sung vocals go.
        let heard = vec![
            (1.0, "neon".into()),
            (1.4, "arms".into()),   // "on" misheard
            (1.8, "the".into()),
            (2.2, "grass".into()),  // "glass" misheard
            (9.0, "driving".into()),
            (9.6, "home".into()),
        ];
        let lyrics = "[verse]
Neon on the glass
Driving home
";
        let lines = align_lyrics(&heard, lyrics);

        // The user's words, on the recogniser's clock - never the mishearing.
        assert_eq!(lines, vec![(1.0, "Neon on the glass".to_string()), (9.0, "Driving home".to_string())]);
    }

    #[test]
    fn every_word_gets_its_own_time_and_the_file_says_so() {
        let heard = vec![
            (1.0, "neon".into()),
            (1.4, "arms".into()),
            (1.8, "the".into()),
            (2.2, "glass".into()),
            (9.0, "driving".into()),
            (9.6, "home".into()),
        ];
        let lines = align_lyrics_words(&heard, "Neon on the glass
Driving home");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "Neon on the glass");
        // "Neon", "the" and "glass" were heard; "on" was not, so it lands
        // between the words either side of it rather than on the line start.
        assert_eq!(lines[0].words[0].0, 1.0);
        assert_eq!(lines[0].words[2].0, 1.8);
        assert_eq!(lines[0].words[3].0, 2.2);
        assert!(lines[0].words[1].0 > 1.0 && lines[0].words[1].0 < 1.8, "got {}", lines[0].words[1].0);

        let lrc = enhanced_lrc(&lines);
        assert!(lrc.starts_with("[00:01.00]<00:01.00>Neon <"), "{lrc}");
        assert!(lrc.contains("<00:09.00>Driving <00:09.60>home"), "{lrc}");
    }

    #[test]
    fn russian_lyrics_align_without_slicing_a_letter_in_half() {
        let heard = vec![
            (1.0, "неон".into()),
            (1.5, "дрожит".into()),
            (2.0, "на".into()),
            (2.4, "коже".into()),
        ];
        let lines = align_lyrics_words(&heard, "Неон дрожит на мокрой коже");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].words.len(), 5);
        assert_eq!(lines[0].words[0].0, 1.0);
        assert_eq!(lines[0].words[1].0, 1.5);
        assert!(enhanced_lrc(&lines).contains("<00:01.50>дрожит"));
    }

    #[test]
    fn a_line_nobody_could_place_is_filled_in_between_its_neighbours() {
        let heard = vec![(0.0, "first".into()), (10.0, "third".into())];
        let lines = align_lyrics(&heard, "First
Something entirely unheard
Third");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].0, 0.0);
        assert!(lines[1].0 > 0.0 && lines[1].0 < 10.0, "the middle line got {}", lines[1].0);
        assert_eq!(lines[2].0, 10.0);
    }


    /// A text that repeats its verse more times than the song sings it: the sung lines get their own times from the
    /// first copies, and the copies past the song get no time at all instead of one shared point.
    #[test]
    fn a_text_longer_than_the_song_times_only_what_was_sung() {
        let verse = ["neon fades along the lane", "footsteps keep the time of rain", "fold the night and leave it here", "morning has a sky to clear"];
        let lyrics = (0..4).flat_map(|_| verse.iter()).copied().collect::<Vec<_>>().join("\n");
        // the song sings the verse once and a half
        let mut words: Vec<(f64, String)> = Vec::new();
        let mut at = 10.0;
        for line in verse.iter().chain(&verse[..2]) {
            for word in line.split_whitespace() {
                words.push((at, word.to_string()));
                at += 0.5;
            }
            at += 1.0;
        }
        let lines = align_lyrics(&words, &lyrics);
        assert_eq!(lines.len(), 6, "only the sung lines are timed: {lines:?}");
        assert!(lines.windows(2).all(|pair| pair[0].0 < pair[1].0), "every sung line has its own time: {lines:?}");
        assert_eq!(lines[0].0, 10.0);
        let timed = align_lyrics_words(&words, &lyrics);
        assert_eq!(timed.len(), 6);
    }


    /// A first line the recogniser heard but could not match keeps its place, from the first word heard.
    #[test]
    fn a_first_line_heard_unclearly_starts_at_the_first_word() {
        let lyrics = "Neon fades along the lane\nFootsteps keep the time of rain\nFold the night and leave it here";
        let mut words: Vec<(f64, String)> = vec![(5.0, "mmm".into()), (5.6, "ahh".into()), (6.2, "ooh".into())];
        let mut at = 10.0;
        for line in ["footsteps keep the time of rain", "fold the night and leave it here"] {
            for word in line.split_whitespace() {
                words.push((at, word.to_string()));
                at += 0.5;
            }
            at += 1.0;
        }
        let lines = align_lyrics(&words, lyrics);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert_eq!((lines[0].0, lines[0].1.as_str()), (5.0, "Neon fades along the lane"));
        assert_eq!(lines[1].0, 10.0, "{lines:?}");
    }

    /// Lines past the song stay without a time when nothing was heard after the last sung line, and when what was
    /// heard there is fewer words than the lines.
    #[test]
    fn lines_past_the_song_get_no_time_from_a_few_stray_words() {
        let sung = "neon fades along the lane";
        let lyrics = std::iter::once(sung.to_string()).chain((0..68).map(|line| format!("extra line number {line} of the text"))).collect::<Vec<_>>().join("\n");
        let mut words: Vec<(f64, String)> = sung.split_whitespace().enumerate().map(|(index, word)| (10.0 + index as f64 * 0.5, word.to_string())).collect();
        assert_eq!(align_lyrics(&words, &lyrics).len(), 1, "no words after the sung line");
        words.push((20.0, "yeah".into()));
        words.push((21.0, "oh".into()));
        assert_eq!(align_lyrics(&words, &lyrics).len(), 1, "two stray words do not time 68 lines");
    }

    #[test]
    fn a_verse_whose_first_line_is_not_sung_stays_on_its_first_performance() {
        // what Parakeet heard in en-A, a song that sings its verse twice, against a text whose first line it never sings
        let words: Vec<(f64, String)> = include_str!("testdata/en-A-words.txt")
            .lines()
            .filter_map(|line| line.strip_prefix("W "))
            .filter_map(|rest| {
                let mut parts = rest.trim().splitn(2, ' ');
                Some((parts.next()?.parse().ok()?, parts.next()?.trim().to_string()))
            })
            .collect();
        assert_eq!(words.len(), 150);
        let lines = align_lyrics(&words, include_str!("testdata/en-A-first-line-changed.txt"));
        let starts: Vec<f64> = lines.iter().map(|(at, _)| *at).collect();
        assert_eq!(starts, vec![11.76, 14.48, 17.28, 19.84, 22.88, 25.68], "{lines:?}");
        assert_eq!(lines[1].1, "Footsteps keep the time of rain");
    }

    #[test]
    fn every_runtime_is_pinned_and_every_asset_is_distinct() {
        for entry in assets() {
            assert!(entry.bytes > 0, "{} has no size", entry.id);
            assert!(entry.url.starts_with("https://"), "{} is not fetched over https", entry.id);
            assert_eq!(assets().iter().filter(|other| other.id == entry.id).count(), 1);
            if entry.kind == AssetKind::Runtime {
                // Every runtime is pinned to an exact release - the audio.cpp server to its tag, ONNX Runtime to its
                // version, NVIDIA's libraries to their own version in the archive name - so a working setup keeps
                // working.
                let pinned = entry.url.contains(AUDIOCPP_BUILD)
                    || entry.url.contains(SERVER_CUFFT_BUILD)
                    || entry.url.contains(ONNXRUNTIME_BUILD)
                    || entry.url.contains(ONNXRUNTIME_DIRECTML_BUILD)
                    || entry.url.contains(DIRECTML_BUILD)
                    || entry.url.contains(CUBLAS_BUILD)
                    || entry.url.contains(CUDART_BUILD)
                    || entry.url.contains(CUDNN_BUILD)
                    || entry.url.contains(CUFFT_BUILD)
                    || entry.url.contains(CURAND_BUILD)
                    || entry.url.contains("/transcribe.cpp/releases/download/engine-");
                assert!(pinned, "{} is not pinned to a release", entry.id);
                // a single library is downloaded as it is, not unpacked
                if entry.unzip_into.is_some() {
                    assert!(!entry.marker.is_empty(), "{} has no proof of extraction", entry.id);
                }
            }
        }
    }

    #[test]
    fn karaoke_stays_off_until_a_provider_is_chosen() {
        let mut config = LyricsSyncConfig::default();
        assert!(!config.available());
        config.enabled = true;
        assert!(!config.available());
        config.provider = AsrProvider::Whisper;
        assert!(config.available());
    }

    #[test]
    fn a_retired_choice_of_model_becomes_the_one_offered() {
        let with = |model: Option<&str>| LyricsSyncConfig { whisper_model: model.map(String::from), ..LyricsSyncConfig::default() }.migrated().whisper_model;
        assert_eq!(with(Some(PARAKEET_FP32)).as_deref(), Some(PARAKEET_V3));
        assert_eq!(with(Some(PARAKEET_ULTRA)).as_deref(), Some(PARAKEET_ULTRA));
        assert_eq!(with(Some(PARAKEET_V3)).as_deref(), Some(PARAKEET_V3));
        assert_eq!(with(Some("whisper-large-v3")), None);
        assert_eq!(with(None), None);
    }

    /// Karaoke's engine loaded in this process and installed over: the record says the new release, the process keeps
    /// the one it loaded, and the window is told it runs from the next start; a file of the record gone from the
    /// folder makes the install not done.
    #[test]
    fn an_engine_installed_over_the_loaded_one_waits_for_a_restart_and_must_be_whole() {
        let root = tempfile::tempdir().unwrap();
        let sync = LyricsSync::new(root.path());
        let build = dub_tcpp::Build::Cpu;
        let asset = assets().iter().find(|asset| asset.unzip_into == Some(build.dir())).expect("the processor build");
        let dir = sync.downloader.runtime_dir(build.dir());
        std::fs::create_dir_all(&dir).unwrap();
        let files: Vec<PathBuf> = [dub_tcpp::LIBRARY, "ggml-cpu-alderlake.dll"].iter().map(|name| dir.join(name)).collect();
        for file in &files {
            std::fs::write(file, b"x").unwrap();
        }
        let record = sync.downloader.record_of(asset);
        crate::install_record::write(&record, "https://example.invalid/engine-0.2.0.zip", "", &files, sync.downloader.root()).unwrap();
        loaded_engines().lock().unwrap().insert(build.dir(), "https://example.invalid/engine-0.2.0.zip".into());
        assert!(!sync.restart_to_apply(), "the loaded release is the one on disk");
        crate::install_record::write(&record, asset.url, "", &files, sync.downloader.root()).unwrap();
        assert!(sync.restart_to_apply(), "today's release is on disk, the process holds the earlier one");
        assert!(sync.downloader.is_installed(asset));
        std::fs::remove_file(&files[1]).unwrap();
        assert!(!sync.downloader.is_installed(asset), "a file of the record is missing: not installed");
        loaded_engines().lock().unwrap().remove(build.dir());
    }
}

#[cfg(test)]
mod live_recognition {
    use super::*;

    #[test]
    fn karaoke_files_land_where_the_shared_binding_looks() {
        let root = Path::new("karaoke-root").join("models");
        for (variant, id) in [(None, PARAKEET_V3), (Some(PARAKEET_ULTRA), PARAKEET_ULTRA), (Some(PARAKEET_FP32), PARAKEET_V3)] {
            let model = parakeet_model(variant);
            assert_eq!(parakeet_model_asset(variant), id);
            let asset = asset(id).unwrap();
            assert_eq!(Path::new("karaoke-root").join(asset.relative_path), dub_tcpp::model_path(&root, model));
            assert_eq!((asset.url, asset.bytes), (model.download.url, model.download.size));
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(parakeet_build(OnnxFlavour::Auto), Some(dub_tcpp::Build::Cpu));
            let asset = asset("transcribe-engine-cpu").unwrap();
            // the Mac fetches the single dylib straight into the build's folder under runtime/, where the loader opens it
            assert_eq!(
                Path::new("karaoke-root").join(asset.relative_path),
                Path::new("karaoke-root").join("runtime").join(dub_tcpp::Build::Cpu.dir()).join(dub_tcpp::LIBRARY),
            );
        }
        // Windows and Linux fetch the engine-0.2.0 archive and unpack it into the build's folder under runtime/, where the
        // loader (runtime_dir(build.dir())) opens LIBRARY; the studio ships the CUDA 13, Vulkan and processor builds.
        #[cfg(any(windows, target_os = "linux"))]
        for build in [dub_tcpp::Build::Cuda, dub_tcpp::Build::Vulkan, dub_tcpp::Build::Cpu] {
            let asset = asset(build.dir()).unwrap();
            assert_eq!(asset.unzip_into, Some(build.dir()));
            assert_eq!(asset.marker, dub_tcpp::LIBRARY);
            assert_eq!((asset.url, asset.bytes), (build.download().url, build.download().size));
        }
    }

    /// The whole recognition path against a real installation, when one is
    /// pointed at: decode, run the recogniser, read its JSON, get words with
    /// times. Checking the download and the binary separately is what let a
    /// finished installation still refuse to run.
    #[test]
    fn recognising_a_real_track_end_to_end() {
        let (Some(root), Some(track)) = (std::env::var_os("STUDIO_TEST_DATA_ROOT"), std::env::var_os("STUDIO_TEST_TRACK")) else { return };
        let sync = LyricsSync::new(std::path::Path::new(&root));
        let runtime = if std::env::var_os("STUDIO_TEST_CPU").is_some() { OnnxFlavour::Cpu } else { OnnxFlavour::Cuda };
        assert!(sync.crisper_ready(runtime), "CrisperWhisper is not installed for {:?}", crate::acpp::build_for(runtime));
        let words = sync.crisper_words(runtime, std::path::Path::new(&track), Some("ru")).expect("recognition");
        studio_log!("words: {}", words.len());
        for (at, word) in words.iter().take(8) {
            studio_log!("  {at:.2}s {word}");
        }
        assert!(!words.is_empty(), "nothing was recognised");
    }

    /// A chorus sung twice, heard worse the first time, and one of its lines
    /// not heard at all: nothing may jump to the second chorus and strand the
    /// verse sung in between.
    #[test]
    fn a_line_heard_better_later_does_not_strand_the_verse_before_it() {
        let heard = |from: f64, text: &str| -> Vec<(f64, String)> {
            text.split_whitespace().enumerate().map(|(i, w)| (from + i as f64 * 0.5, w.to_string())).collect()
        };
        let mut words = Vec::new();
        words.extend(heard(1.0, "walking down the road tonight"));
        words.extend(heard(10.0, "my truck talks back"));
        words.extend(heard(13.0, "fast down the dusty track"));
        words.extend(heard(16.0, "ive been part of it"));
        words.extend(heard(20.0, "took my girl out on a friday"));
        words.extend(heard(30.0, "my truck talks back"));
        words.extend(heard(33.0, "down the dusty track"));
        words.extend(heard(36.0, "feeling right tonight"));
        let lyrics = "[Verse 1]\nWalking down the road tonight\n\n[Chorus]\nMy truck talks back\nDown the dusty track\nFeeling right tonight\n\n[Verse 2]\nTook my girl out on a Friday\n\n[Chorus]\nMy truck talks back\nDown the dusty track\nFeeling right tonight";
        let lines = align_lyrics(&words, lyrics);
        let at = |index: usize| lines[index].0;
        assert_eq!(lines.len(), 8);
        assert!((at(2) - 13.5).abs() < 0.6, "the first chorus keeps its own line: {lines:?}");
        assert!(at(3) > at(2) && at(3) < at(4), "an unheard line sits between its neighbours: {lines:?}");
        assert!((at(4) - 20.0).abs() < 0.6, "the second verse is where it was sung: {lines:?}");
        assert!((at(6) - 33.0).abs() < 0.6 && (at(7) - 36.0).abs() < 0.6, "the second chorus is its own: {lines:?}");
    }

    /// The karaoke of a real track: Parakeet's words aligned to the lyrics in
    /// `STUDIO_TEST_LYRICS`, printed line by line with their start times.
    #[test]
    fn parakeet_karaoke_of_a_real_track() {
        let (Some(root), Some(track), Some(lyrics)) =
            (std::env::var_os("STUDIO_TEST_DATA_ROOT"), std::env::var_os("STUDIO_TEST_TRACK"), std::env::var_os("STUDIO_TEST_LYRICS"))
        else {
            return;
        };
        let sync = LyricsSync::new(std::path::Path::new(&root));
        let words = sync.parakeet_words(OnnxFlavour::Auto, None, std::path::Path::new(&track), None).expect("recognition");
        let lyrics = std::fs::read_to_string(lyrics).expect("lyrics file");
        for (at, word) in &words {
            studio_log!("W {at:7.2} {word}");
        }
        for line in align_lyrics_words(&words, &lyrics) {
            studio_log!("{:7.1} {}", line.start, line.words.iter().map(|(_, word)| word.as_str()).collect::<Vec<_>>().join(" "));
        }
    }

    /// The same against Parakeet: what the recogniser actually heard, printed
    /// whole, so a generated song can be checked against its own lyrics.
    #[test]
    fn parakeet_hears_a_real_track() {
        let (Some(root), Some(track)) = (std::env::var_os("STUDIO_TEST_DATA_ROOT"), std::env::var_os("STUDIO_TEST_TRACK")) else { return };
        let sync = LyricsSync::new(std::path::Path::new(&root));
        assert!(sync.parakeet_ready(OnnxFlavour::Auto, None), "Parakeet is not installed");
        let words = sync.parakeet_words(OnnxFlavour::Auto, None, std::path::Path::new(&track), None).expect("recognition");
        let heard: Vec<&str> = words.iter().map(|(_, word)| word.as_str()).collect();
        studio_log!("heard {} words: {}", words.len(), heard.join(" "));
        assert!(!words.is_empty(), "nothing was recognised");
    }
}


#[cfg(all(test, windows))]
mod parakeet_live {
    use super::*;
    use std::time::Instant;

    fn env_path(name: &str) -> PathBuf {
        PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")))
    }

    /// Parakeet on each transcribe.cpp build, from the karaoke folder of `STUDIO_TEST_DATA_ROOT`: how much of the song's own
    /// lyrics (`STUDIO_TEST_LYRICS`) each hears in order, and how long it takes. `GGML_VK_VISIBLE_DEVICES` picks the card
    /// Vulkan runs on.
    #[test]
    #[ignore = "needs STUDIO_TEST_DATA_ROOT with the three engine builds, STUDIO_TEST_TRACK and STUDIO_TEST_LYRICS"]
    fn parakeet_hears_the_lyrics_on_every_build() {
        let root = env_path("STUDIO_TEST_DATA_ROOT").join("karaoke").join("models");
        let track = env_path("STUDIO_TEST_TRACK");
        let lyrics = std::fs::read_to_string(env_path("STUDIO_TEST_LYRICS")).expect("the lyrics");
        let sung = words_of(lyrics.lines().filter(|line| !line.trim_start().starts_with('[')).flat_map(str::split_whitespace));
        let model = parakeet_model(None);
        for build in dub_tcpp::Build::ALL {
            let started = Instant::now();
            let asr = dub_tcpp::shared(&dub_tcpp::engine_dir(&root, build), &dub_tcpp::model_path(&root, model)).expect("load Parakeet");
            let words = parakeet_transcribe(&asr, model, &track, None).expect("recognition");
            let heard = words_of(words.iter().map(|(_, word)| word.as_str()));
            let share = in_order(&sung, &heard);
            studio_log!("Parakeet on {build:?}: {:.0}% of the lyrics in order, {:.1} s\n  {}", share * 100.0, started.elapsed().as_secs_f64(), heard.join(" "));
            assert!(share > 0.5, "{build:?} hears too little of the lyrics");
        }
    }

    /// Parakeet on the processor build over `STUDIO_TEST_TRACK`, then the karaoke of `STUDIO_TEST_LYRICS`: every heard
    /// word with its time, and every line with the time it gets.
    #[test]
    #[ignore = "needs STUDIO_TEST_DATA_ROOT with the processor build, STUDIO_TEST_TRACK and STUDIO_TEST_LYRICS"]
    fn karaoke_of_a_real_track_on_the_processor() {
        let karaoke = env_path("STUDIO_TEST_DATA_ROOT").join("karaoke");
        let model = parakeet_model(None);
        let engine = karaoke.join("runtime").join(dub_tcpp::Build::Cpu.dir());
        let asr = dub_tcpp::shared(&engine, &dub_tcpp::model_path(&karaoke.join("models"), model)).expect("load Parakeet");
        let words = parakeet_transcribe(&asr, model, &env_path("STUDIO_TEST_TRACK"), None).expect("recognition");
        for (at, word) in &words {
            studio_log!("W {at:7.2} {word}");
        }
        let lyrics = std::fs::read_to_string(env_path("STUDIO_TEST_LYRICS")).expect("the lyrics");
        for line in align_lyrics_words(&words, &lyrics) {
            studio_log!("L {:7.2} {}", line.start, line.words.iter().map(|(_, word)| word.as_str()).collect::<Vec<_>>().join(" "));
        }
    }

    /// The line starts of `STUDIO_TEST_LYRICS` over the words a run of the test above logged (`W <time> <word>` lines of
    /// `STUDIO_TEST_WORDS`), with the gap penalty `STUDIO_TEST_GAP` per skipped word, counting the words before the first
    /// line when `STUDIO_TEST_GAP_LEADING` is 1: the penalty is tried without the engine.
    #[test]
    #[ignore = "needs STUDIO_TEST_WORDS, STUDIO_TEST_LYRICS and STUDIO_TEST_GAP"]
    fn line_starts_with_a_gap_penalty_over_logged_words() {
        let logged = std::fs::read_to_string(env_path("STUDIO_TEST_WORDS")).expect("the logged words");
        let words: Vec<(f64, String)> = logged
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix("W "))
            .filter_map(|rest| {
                let mut parts = rest.trim().splitn(2, ' ');
                let at = parts.next()?.parse::<f64>().ok()?;
                Some((at, parts.next()?.trim().to_string()))
            })
            .collect();
        assert!(!words.is_empty(), "no W lines in the log");
        let lyrics = std::fs::read_to_string(env_path("STUDIO_TEST_LYRICS")).expect("the lyrics");
        let per_word: f64 = std::env::var("STUDIO_TEST_GAP").expect("STUDIO_TEST_GAP").parse().expect("a number");
        let leading = std::env::var("STUDIO_TEST_GAP_LEADING").is_ok_and(|value| value == "1");
        studio_log!("gap {per_word} leading {leading}: {} words", words.len());
        for (at, line) in align_lyrics_with(&words, &lyrics, Gaps { per_word, leading }) {
            studio_log!("L {at:7.2} {line}");
        }
    }

    /// Words without punctuation, in lower case.
    fn words_of<'a>(words: impl Iterator<Item = &'a str>) -> Vec<String> {
        words
            .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
            .filter(|word| !word.is_empty())
            .collect()
    }

    /// The share of `reference` that `heard` has in the same order: their
    /// longest common subsequence over the reference's length.
    fn in_order(reference: &[String], heard: &[String]) -> f64 {
        let mut row = vec![0usize; heard.len() + 1];
        for word in reference {
            let mut diagonal = 0;
            for (index, other) in heard.iter().enumerate() {
                let above = row[index + 1];
                row[index + 1] = if word == other { diagonal + 1 } else { row[index + 1].max(row[index]) };
                diagonal = above;
            }
        }
        row[heard.len()] as f64 / reference.len().max(1) as f64
    }
}
