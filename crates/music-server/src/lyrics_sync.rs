//! Karaoke timings for a finished track.
//!
//! The words are already known - Music3 sang the lyrics it was given - but the
//! *timings* are not, and the video studio's karaoke layer and the player both
//! need them. This module produces an LRC file for a track using whichever
//! recogniser the user picked; like every optional extra here it is off by
//! default and downloads nothing on its own.
//!
//! Three backends, all of them explicit choices:
//!
//!   * **Whisper** - whisper.cpp run as a sidecar. It writes LRC itself, and
//!     the CUDA build is preferred over the CPU one when both are installed.
//!   * **Parakeet** - the NVIDIA TDT model, the same one Dub Studio uses.
//!   * **OpenRouter** - a cloud model, billed to the user's own key, asked for
//!     verbose output because plain text carries no timings.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use parakeet_rs::Transcriber;

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
}

/// whisper.cpp is pinned to one release so a working setup keeps working.
/// The recogniser is pinned to one release, so a setup that works keeps
/// working: Purfview's standalone faster-whisper, the build Dub Studio runs.
const WHISPER_BUILD: &str = "Whisper-Faster_r192.3";
/// CTranslate2 in that build links against CUDA 11, not the 12 the separator
/// uses, so Whisper carries its own pair of libraries.
const WHISPER_CUBLAS_BUILD: &str = "11.11.3.6";
const WHISPER_CUDNN_BUILD: &str = "8.9.7.29";
/// The ONNX Runtime that Parakeet loads. Mixing versions deadlocks the loader,
/// so this is pinned exactly as Dub Studio pins it.
/// The NVIDIA libraries the CUDA provider links against, pinned like the rest.
const CUBLAS_BUILD: &str = "12.9.2.10";
const CUDART_BUILD: &str = "12.9.79";
const CUFFT_BUILD: &str = "11.4.1.4";
const CUDNN_BUILD: &str = "9.25.0.15";
const ONNXRUNTIME_BUILD: &str = "v1.30.0";

/// What a recogniser that heard nothing sung says; the dataset preparation
/// takes it to mean the song is an instrumental.
pub const NO_WORDS: &str = "the recogniser found no words to time";

/// Where the recogniser's binaries live once unpacked. CTranslate2 loads its
/// CUDA libraries from beside the executable, so they share one directory - the
/// way Dub Studio arranges it, and the reason its card mode works instead of
/// quietly falling back to the processor.
pub const WHISPER_RUNTIME_DIR: &str = "whisper";

/// The model sizes the recogniser knows, as `--model` names them.
pub const WHISPER_SIZES: &[&str] = &["tiny", "base", "small", "medium", "large-v3", "large-v3-turbo"];

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

/// The words and their times out of faster-whisper's JSON.
///

/// The words and their times out of faster-whisper's JSON.
///
/// A segment that came back without word timestamps becomes one long "word":
/// better a line placed roughly than a line dropped, and the written lyrics are
/// laid back over whatever times these are.
fn whisper_words_from_json(text: &str) -> Vec<(f64, String)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    let Some(segments) = value.get("segments").and_then(|value| value.as_array()) else { return Vec::new() };
    let mut words = Vec::new();
    for segment in segments {
        if is_hallucination(segment.get("text").and_then(|value| value.as_str()).unwrap_or_default()) {
            continue;
        }
        match segment.get("words").and_then(|value| value.as_array()) {
            Some(list) if !list.is_empty() => {
                for entry in list {
                    let word = entry
                        .get("word")
                        .or_else(|| entry.get("text"))
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    if word.is_empty() {
                        continue;
                    }
                    words.push((entry.get("start").and_then(|value| value.as_f64()).unwrap_or(0.0), word));
                }
            }
            _ => {
                let word = segment.get("text").and_then(|value| value.as_str()).unwrap_or_default().trim().to_string();
                if !word.is_empty() {
                    words.push((segment.get("start").and_then(|value| value.as_f64()).unwrap_or(0.0), word));
                }
            }
        }
    }
    words
}

pub const ASSETS: &[Asset] = &[
    Asset {
        id: "whisper-engine",
        label: "Whisper (faster-whisper standalone)",
        kind: AssetKind::Runtime,
        url: "https://github.com/Purfview/whisper-standalone-win/releases/download/faster-whisper/Whisper-Faster_r192.3_windows.zip",
        relative_path: "runtime/whisper-faster.zip",
        bytes: 87_654_143,
        unzip_into: Some(WHISPER_RUNTIME_DIR),
        marker: "whisper-faster",
        pick: &[],
        vram_gb: None,
        note: "Purfview's build of faster-whisper: word timestamps, on the card or the processor.",
    },
    Asset {
        id: "whisper-cublas",
        label: "NVIDIA cuBLAS 11.11 (for Whisper)",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-11.11.3.6-archive.zip",
        relative_path: "runtime/whisper-cublas.zip",
        bytes: 420_850_025,
        unzip_into: Some(WHISPER_RUNTIME_DIR),
        marker: "cublas64_11",
        pick: &["cublas64_11.dll", "cublasLt64_11.dll"],
        vram_gb: None,
        note: "CTranslate2 is built against CUDA 11; without these the card is never used.",
    },
    Asset {
        id: "whisper-cudnn",
        label: "NVIDIA cuDNN 8.9 (for Whisper)",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/windows-x86_64/cudnn-windows-x86_64-8.9.7.29_cuda11-archive.zip",
        relative_path: "runtime/whisper-cudnn.zip",
        bytes: 704_240_064,
        unzip_into: Some(WHISPER_RUNTIME_DIR),
        marker: "cudnn64_8",
        pick: &["cudnn64_8.dll", "cudnn_ops_infer64_8.dll", "cudnn_cnn_infer64_8.dll"],
        vram_gb: None,
        note: "The convolution kernels the encoder spends its time in.",
    },
    Asset {
        id: "whisper-tiny",
        label: "Whisper tiny",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-tiny/resolve/main/model.bin",
        relative_path: "models/whisper/faster-whisper-tiny/model.bin",
        bytes: 75_538_270,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(1),
        note: "The smallest there is. For a quick check, not for lyrics.",
    },
    Asset {
        id: "whisper-tiny-config",
        label: "Whisper tiny (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-tiny/resolve/main/config.json",
        relative_path: "models/whisper/faster-whisper-tiny/config.json",
        bytes: 2_249,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-tiny-tokenizer",
        label: "Whisper tiny (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-tiny/resolve/main/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-tiny/tokenizer.json",
        bytes: 2_203_239,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-tiny-vocabulary",
        label: "Whisper tiny (vocabulary.txt)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-tiny/resolve/main/vocabulary.txt",
        relative_path: "models/whisper/faster-whisper-tiny/vocabulary.txt",
        bytes: 459_861,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-base",
        label: "Whisper base",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-base/resolve/main/model.bin",
        relative_path: "models/whisper/faster-whisper-base/model.bin",
        bytes: 145_217_532,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(1),
        note: "Fast and small; misses words in dense mixes.",
    },
    Asset {
        id: "whisper-base-config",
        label: "Whisper base (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-base/resolve/main/config.json",
        relative_path: "models/whisper/faster-whisper-base/config.json",
        bytes: 2_309,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-base-tokenizer",
        label: "Whisper base (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-base/resolve/main/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-base/tokenizer.json",
        bytes: 2_203_239,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-base-vocabulary",
        label: "Whisper base (vocabulary.txt)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-base/resolve/main/vocabulary.txt",
        relative_path: "models/whisper/faster-whisper-base/vocabulary.txt",
        bytes: 459_861,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-small",
        label: "Whisper small",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-small/resolve/main/model.bin",
        relative_path: "models/whisper/faster-whisper-small/model.bin",
        bytes: 483_546_902,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(2),
        note: "Noticeably better than base without asking much of the card.",
    },
    Asset {
        id: "whisper-small-config",
        label: "Whisper small (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-small/resolve/main/config.json",
        relative_path: "models/whisper/faster-whisper-small/config.json",
        bytes: 2_370,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-small-tokenizer",
        label: "Whisper small (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-small/resolve/main/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-small/tokenizer.json",
        bytes: 2_203_239,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-small-vocabulary",
        label: "Whisper small (vocabulary.txt)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-small/resolve/main/vocabulary.txt",
        relative_path: "models/whisper/faster-whisper-small/vocabulary.txt",
        bytes: 459_861,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-medium",
        label: "Whisper medium",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-medium/resolve/main/model.bin",
        relative_path: "models/whisper/faster-whisper-medium/model.bin",
        bytes: 1_527_906_378,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(3),
        note: "Slower than turbo and rarely better on sung words.",
    },
    Asset {
        id: "whisper-medium-config",
        label: "Whisper medium (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-medium/resolve/main/config.json",
        relative_path: "models/whisper/faster-whisper-medium/config.json",
        bytes: 2_257,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-medium-tokenizer",
        label: "Whisper medium (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-medium/resolve/main/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-medium/tokenizer.json",
        bytes: 2_203_239,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-medium-vocabulary",
        label: "Whisper medium (vocabulary.txt)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-medium/resolve/main/vocabulary.txt",
        relative_path: "models/whisper/faster-whisper-medium/vocabulary.txt",
        bytes: 459_861,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3",
        label: "Whisper large-v3",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-large-v3/resolve/main/model.bin",
        relative_path: "models/whisper/faster-whisper-large-v3/model.bin",
        bytes: 3_087_284_237,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(5),
        note: "The full model. Slower than turbo, and the most accurate on hard mixes.",
    },
    Asset {
        id: "whisper-large-v3-config",
        label: "Whisper large-v3 (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-large-v3/resolve/main/config.json",
        relative_path: "models/whisper/faster-whisper-large-v3/config.json",
        bytes: 2_394,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-preprocessor-config",
        label: "Whisper large-v3 (preprocessor_config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-large-v3/resolve/main/preprocessor_config.json",
        relative_path: "models/whisper/faster-whisper-large-v3/preprocessor_config.json",
        bytes: 340,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-tokenizer",
        label: "Whisper large-v3 (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-large-v3/resolve/main/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-large-v3/tokenizer.json",
        bytes: 2_480_617,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-vocabulary",
        label: "Whisper large-v3 (vocabulary.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/Systran/faster-whisper-large-v3/resolve/main/vocabulary.json",
        relative_path: "models/whisper/faster-whisper-large-v3/vocabulary.json",
        bytes: 1_068_114,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-turbo",
        label: "Whisper large-v3-turbo",
        kind: AssetKind::Model,
        url: "https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/main/model.bin",
        relative_path: "models/whisper/faster-whisper-large-v3-turbo/model.bin",
        bytes: 1_617_884_929,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(3),
        note: "The accurate choice for sung lyrics.",
    },
    Asset {
        id: "whisper-large-v3-turbo-config",
        label: "Whisper large-v3-turbo (config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/main/config.json",
        relative_path: "models/whisper/faster-whisper-large-v3-turbo/config.json",
        bytes: 2_263,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-turbo-preprocessor-config",
        label: "Whisper large-v3-turbo (preprocessor_config.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/main/preprocessor_config.json",
        relative_path: "models/whisper/faster-whisper-large-v3-turbo/preprocessor_config.json",
        bytes: 340,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-turbo-tokenizer",
        label: "Whisper large-v3-turbo (tokenizer.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/main/tokenizer.json",
        relative_path: "models/whisper/faster-whisper-large-v3-turbo/tokenizer.json",
        bytes: 2_710_337,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "whisper-large-v3-turbo-vocabulary",
        label: "Whisper large-v3-turbo (vocabulary.json)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/main/vocabulary.json",
        relative_path: "models/whisper/faster-whisper-large-v3-turbo/vocabulary.json",
        bytes: 1_068_114,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Part of the model above.",
    },
    Asset {
        id: "parakeet-tdt-int8",
        label: "Parakeet TDT 0.6B v3 (int8)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main/encoder-model.int8.onnx",
        relative_path: "models/parakeet/encoder-model.int8.onnx",
        bytes: 652_183_999,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(2),
        note: "The encoder; the decoder and vocabulary come with it.",
    },
    // The fp32 encoder, exactly as Dub Studio fetches it: the graph and its
    // weights are two files, and the weights are the 2.4 GB half.
    Asset {
        id: "parakeet-tdt-fp32",
        label: "Parakeet TDT 0.6B v3 (fp32)",
        kind: AssetKind::Model,
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main/encoder-model.onnx",
        relative_path: "models/parakeet-fp32/encoder-model.onnx",
        bytes: 41_770_866,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: Some(4),
        note: "Full precision: heavier than int8, and the most accurate of the two.",
    },
    Asset {
        id: "parakeet-tdt-fp32-weights",
        label: "Parakeet fp32 weights",
        kind: AssetKind::Model,
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main/encoder-model.onnx.data",
        relative_path: "models/parakeet-fp32/encoder-model.onnx.data",
        bytes: 2_435_420_160,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "The weights the fp32 graph points at.",
    },
    Asset {
        id: "parakeet-decoder",
        label: "Parakeet decoder",
        kind: AssetKind::Model,
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main/decoder_joint-model.int8.onnx",
        relative_path: "models/parakeet/decoder_joint-model.int8.onnx",
        bytes: 18_202_004,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Required alongside the Parakeet encoder.",
    },
    Asset {
        id: "parakeet-features",
        label: "Parakeet feature extractor",
        kind: AssetKind::Model,
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main/nemo128.onnx",
        relative_path: "models/parakeet/nemo128.onnx",
        bytes: 139_764,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "The mel front end the encoder expects.",
    },
    Asset {
        id: "parakeet-vocab",
        label: "Parakeet vocabulary",
        kind: AssetKind::Model,
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main/vocab.txt",
        relative_path: "models/parakeet/vocab.txt",
        bytes: 93_939,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Token table.",
    },
    Asset {
        id: "parakeet-config",
        label: "Parakeet configuration",
        kind: AssetKind::Model,
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main/config.json",
        relative_path: "models/parakeet/config.json",
        bytes: 97,
        unzip_into: None,
        marker: "",
        pick: &[],
        vram_gb: None,
        note: "Token table.",
    },
    Asset {
        id: "onnxruntime-cuda",
        label: "ONNX Runtime 1.30.0 · CUDA",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-win-x64-gpu_cuda12-1.30.0.zip",
        relative_path: "runtime/onnxruntime-cuda.zip",
        bytes: 379_723_801,
        unzip_into: Some("onnx-cuda"),
        marker: "onnxruntime_providers_cuda.dll",
        pick: &[],
        vram_gb: Some(2),
        note: "Runs the separator on an NVIDIA card instead of the processor. Needs CUDA 12.",
    },
    Asset {
        id: "cuda-cublas",
        label: "NVIDIA cuBLAS 12.9",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-12.9.2.10-archive.zip",
        relative_path: "runtime/cuda-cublas.zip",
        bytes: 549_731_131,
        unzip_into: Some("onnx-cuda"),
        marker: "cublasLt64_12.dll",
        pick: &["cublasLt64_12.dll", "cublas64_12.dll"],
        vram_gb: None,
        note: "The linear algebra the CUDA provider is built on.",
    },
    Asset {
        id: "cuda-cudart",
        label: "NVIDIA CUDA runtime 12.9",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/cuda_cudart/windows-x86_64/cuda_cudart-windows-x86_64-12.9.79-archive.zip",
        relative_path: "runtime/cuda-cudart.zip",
        bytes: 3_521_238,
        unzip_into: Some("onnx-cuda"),
        marker: "cudart64_12.dll",
        pick: &["cudart64_12.dll"],
        vram_gb: None,
        note: "The CUDA runtime itself.",
    },
    Asset {
        id: "cuda-cufft",
        label: "NVIDIA cuFFT 11.4",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-11.4.1.4-archive.zip",
        relative_path: "runtime/cuda-cufft.zip",
        bytes: 198_361_265,
        unzip_into: Some("onnx-cuda"),
        marker: "cufft64_11.dll",
        pick: &["cufft64_11.dll"],
        vram_gb: None,
        note: "The transforms the provider uses for spectral work.",
    },
    Asset {
        id: "cuda-cudnn",
        label: "NVIDIA cuDNN 9.25",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/windows-x86_64/cudnn-windows-x86_64-9.25.0.15_cuda12-archive.zip",
        relative_path: "runtime/cuda-cudnn.zip",
        bytes: 1_904_452_100,
        unzip_into: Some("onnx-cuda"),
        marker: "cudnn64_9.dll",
        // Everything the convolution path loads, and nothing else: the
        // attention kernels alone are another 250 MB the separator never calls.
        pick: &[
            "cudnn64_9.dll",
            "cudnn_graph64_9.dll",
            "cudnn_ops64_9.dll",
            "cudnn_cnn64_9.dll",
            "cudnn_heuristic64_9.dll",
            "cudnn_engines_precompiled64_9.dll",
            "cudnn_engines_runtime_compiled64_9.dll",
            "cudnn_engines_tensor_ir64_9.dll",
            "cudnn_ext64_9.dll",
        ],
        vram_gb: None,
        note: "The convolution kernels the separator spends its time in.",
    },
    Asset {
        id: "onnxruntime",
        label: "ONNX Runtime 1.30.0",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-win-x64-1.30.0.zip",
        relative_path: "runtime/onnxruntime.zip",
        bytes: 82_645_522,
        unzip_into: Some("onnx"),
        marker: "onnxruntime.dll",
        pick: &[],
        vram_gb: None,
        note: "Parakeet runs on this; it is loaded at run time, not linked in.",
    },
];

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

/// Every Parakeet file, because the model is useless without all of them.
pub const PARAKEET_ASSET_IDS: [&str; 5] =
    ["parakeet-tdt-int8", "parakeet-decoder", "parakeet-features", "parakeet-vocab", "parakeet-config"];

/// The same recogniser at full precision. The encoder is a graph plus a
/// separate weights file; everything else is shared with the int8 set.
pub const PARAKEET_FP32_ASSET_IDS: [&str; 6] = [
    "parakeet-tdt-fp32",
    "parakeet-tdt-fp32-weights",
    "parakeet-decoder",
    "parakeet-features",
    "parakeet-vocab",
    "parakeet-config",
];

pub fn asset(id: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|asset| asset.id == id)
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
}

pub struct LyricsSync {
    downloader: Downloader,
}

impl LyricsSync {
    pub fn new(data_root: &Path) -> Self {
        Self { downloader: Downloader::new(data_root.join("karaoke")) }
    }

    pub fn downloader(&self) -> &Downloader {
        &self.downloader
    }

    /// The CUDA build first: on a machine that has one, the CPU build would be
    /// a silent downgrade.
    pub fn whisper_binary(&self) -> Option<PathBuf> {
        crate::downloads::locate_binary(self.downloader.root(), &[WHISPER_RUNTIME_DIR], "whisper-faster")
    }

    /// What `--model_dir` is given: the binary looks inside it for a directory
    /// called `faster-whisper-<size>`.
    fn whisper_model_dir(&self) -> PathBuf {
        self.downloader.root().join("models").join("whisper")
    }

    /// The size a model id stands for - `whisper-large-v3` is `large-v3`, which
    /// is the name the binary is given.
    fn whisper_size(id: &str) -> Option<&str> {
        let size = id.strip_prefix("whisper-")?;
        WHISPER_SIZES.contains(&size).then_some(size)
    }

    pub fn installed_models(&self) -> Vec<&'static Asset> {
        ASSETS
            .iter()
            .filter(|asset| asset.kind == AssetKind::Model && self.downloader.is_installed(asset))
            .collect()
    }

    /// Parakeet needs every one of its files and the ONNX Runtime library.
    pub fn parakeet_ready(&self) -> bool {
        self.onnxruntime_library().is_some()
            && PARAKEET_ASSET_IDS
                .iter()
                .all(|id| asset(id).is_some_and(|asset| self.downloader.is_installed(asset)))
    }

    /// Whether the Whisper model the configuration names is actually on disk.
    /// A faster-whisper model is a directory, not a file - and
    /// `whisper_model_path` has already checked that every file inside it
    /// arrived. Asking whether that path is a file said no to a complete
    /// installation, which is how a finished download still refused to run.
    pub fn whisper_model_ready(&self, config: &LyricsSyncConfig) -> bool {
        self.whisper_model_path(config).is_some_and(|path| path.is_dir())
    }

    pub fn parakeet_dir(&self) -> PathBuf {
        self.downloader.root().join("models").join("parakeet")
    }

    /// `ort` loads this at run time; linking it would tie the build to one
    /// toolchain and one machine's libraries.
    pub fn onnxruntime_library(&self) -> Option<PathBuf> {
        self.onnxruntime_library_of(OnnxFlavour::Auto)
    }

    /// The runtime to load, by name rather than by hope: the CUDA build and the
    /// processor build sit in their own directories, and "auto" prefers the one
    /// that uses the graphics card.
    pub fn onnxruntime_library_of(&self, flavour: OnnxFlavour) -> Option<PathBuf> {
        let cuda = self.downloader.runtime_dir("onnx-cuda").join("onnxruntime.dll");
        let cpu = self.downloader.runtime_dir("onnx").join("onnxruntime.dll");
        match flavour {
            OnnxFlavour::Cuda => cuda.is_file().then_some(cuda),
            OnnxFlavour::Cpu => cpu.is_file().then_some(cpu),
            OnnxFlavour::Auto => cuda.is_file().then_some(cuda).or_else(|| cpu.is_file().then_some(cpu)),
        }
    }

    pub fn has_cuda_runtime(&self) -> bool {
        self.downloader.runtime_dir("onnx-cuda").join("onnxruntime.dll").is_file()
    }

    /// The CUDA provider is a separate library, and it in turn needs cuBLAS and
    /// cuDNN beside it. Without all of them the provider refuses to load and the
    /// run silently lands on the processor.
    pub fn has_cuda_libraries(&self) -> bool {
        let dir = self.downloader.runtime_dir("onnx-cuda");
        ["onnxruntime_providers_cuda.dll", "cublasLt64_12.dll", "cudart64_12.dll", "cudnn64_9.dll"]
            .iter()
            .all(|name| dir.join(name).is_file())
    }

    /// A model counts as present only with every one of its files: a directory
    /// missing its tokenizer loads exactly as far as an error message.
    fn whisper_model_path(&self, config: &LyricsSyncConfig) -> Option<PathBuf> {
        let size = Self::whisper_size(config.whisper_model.as_deref()?)?;
        let prefix = format!("models/whisper/faster-whisper-{size}/");
        let parts: Vec<&'static Asset> = ASSETS.iter().filter(|asset| asset.relative_path.starts_with(&prefix)).collect();
        if parts.is_empty() || !parts.iter().all(|asset| self.downloader.is_installed(asset)) {
            return None;
        }
        Some(self.whisper_model_dir().join(format!("faster-whisper-{size}")))
    }

    pub async fn status(&self, config: &LyricsSyncConfig) -> SyncStatus {
        let whisper_binary = self.whisper_binary();
        let ready = match config.provider {
            AsrProvider::None => false,
            AsrProvider::Whisper => whisper_binary.is_some() && self.whisper_model_path(config).is_some(),
            AsrProvider::Parakeet => self.parakeet_ready(),
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
            assets: self.downloader.status_of(ASSETS),
            active_download: self.downloader.active_for("karaoke").await,
        }
    }

    /// Runs Parakeet in this process and returns an LRC built from its word
    /// timings. Same stack Dub Studio uses: parakeet-rs over ONNX Runtime,
    /// loaded from the DLL beside the models rather than linked in.
    pub fn parakeet_words(&self, audio: &Path) -> Result<Vec<(f64, String)>> {
        let library = self
            .onnxruntime_library()
            .ok_or_else(|| anyhow!("the ONNX Runtime library is not installed"))?;
        if !self.parakeet_ready() {
            bail!("the Parakeet model is not fully downloaded");
        }
        // Must be set before anything touches `ort`, or it binds to whatever
        // onnxruntime.dll the system happens to have.
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| unsafe { std::env::set_var("ORT_DYLIB_PATH", &library) });

        let samples = crate::audio_pcm::decode_mono_16k(audio)
            .with_context(|| format!("decode {} for recognition", audio.display()))?;
        let mut model = parakeet_rs::ParakeetTDT::from_pretrained(&self.parakeet_dir(), None)
            .map_err(|error| anyhow!("load Parakeet: {error}"))?;
        let result = model
            .transcribe_samples(samples, 16_000, 1, Some(parakeet_rs::TimestampMode::Words))
            .map_err(|error| anyhow!("Parakeet transcription failed: {error}"))?;

        let words: Vec<(f64, String)> = result
            .tokens
            .into_iter()
            .filter_map(|token| {
                let text = token.text.trim().to_string();
                (!text.is_empty()).then_some((token.start as f64, text))
            })
            .collect();
        if words.is_empty() {
            bail!(NO_WORDS);
        }
        Ok(words)
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
                let library = match self.onnxruntime_library() {
                    Some(library) => library,
                    None => {
                        failed(heard, anyhow!("the ONNX Runtime library is not installed"));
                        return Recognised::OnDevice;
                    }
                };
                if !self.parakeet_ready() {
                    failed(heard, anyhow!("the Parakeet model is not fully downloaded"));
                    return Recognised::OnDevice;
                }
                static ONCE: std::sync::Once = std::sync::Once::new();
                ONCE.call_once(|| unsafe { std::env::set_var("ORT_DYLIB_PATH", &library) });
                let mut model = match parakeet_rs::ParakeetTDT::from_pretrained(&self.parakeet_dir(), None) {
                    Ok(model) => model,
                    Err(error) => {
                        failed(heard, anyhow!("load Parakeet: {error}"));
                        return Recognised::OnDevice;
                    }
                };
                for (index, path) in audio.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        return Recognised::OnDevice;
                    }
                    let answer = (|| {
                        let samples = crate::audio_pcm::decode_mono_16k(path).with_context(|| format!("decode {} for recognition", path.display()))?;
                        let result = model
                            .transcribe_samples(samples, 16_000, 1, Some(parakeet_rs::TimestampMode::Words))
                            .map_err(|error| anyhow!("Parakeet transcription failed: {error}"))?;
                        let words: Vec<(f64, String)> = result
                            .tokens
                            .into_iter()
                            .filter_map(|token| {
                                let text = token.text.trim().to_string();
                                (!text.is_empty()).then_some((token.start as f64, text))
                            })
                            .collect();
                        if words.is_empty() {
                            bail!(NO_WORDS);
                        }
                        Ok(words)
                    })();
                    heard(index, answer);
                }
                Recognised::OnDevice
            }
            AsrProvider::Whisper => match self.whisper_many(config, audio, language, heard, cancel) {
                Ok(recognised) => recognised,
                Err(error) => {
                    failed(heard, error);
                    Recognised::OnDevice
                }
            },
            _ => {
                failed(heard, anyhow!("this recogniser does not run on this computer"));
                Recognised::OnDevice
            }
        }
    }

    /// One Whisper run over several tracks; the JSON it writes for each is
    /// named after the file it was given, and handed on as soon as it is whole.
    fn whisper_many(
        &self,
        config: &LyricsSyncConfig,
        audio: &[PathBuf],
        language: Option<&str>,
        heard: &mut dyn FnMut(usize, Result<Vec<(f64, String)>>),
        cancel: &AtomicBool,
    ) -> Result<Recognised> {
        let binary = self.whisper_binary().ok_or_else(|| anyhow!("the Whisper runtime is not installed"))?;
        let size = config
            .whisper_model
            .as_deref()
            .and_then(Self::whisper_size)
            .ok_or_else(|| anyhow!("no Whisper model is downloaded and selected"))?;
        if self.whisper_model_path(config).is_none() {
            bail!("the Whisper model {size} is not completely downloaded");
        }
        let work = self.downloader.root().join("work").join(format!("batch-{}", uuid::Uuid::now_v7()));
        let out_dir = work.join("out");
        fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;
        let mut wavs: Vec<(usize, PathBuf)> = Vec::with_capacity(audio.len());
        let mut answered = vec![false; audio.len()];
        for (index, path) in audio.iter().enumerate() {
            let wav = work.join(format!("{index}.wav"));
            match crate::audio_pcm::write_wav16k_mono(path, &wav) {
                Ok(()) => wavs.push((index, wav)),
                Err(error) => {
                    answered[index] = true;
                    heard(index, Err(error.context(format!("decode {} for recognition", path.display()))));
                }
            }
        }
        // A track's JSON is taken once it parses: Whisper writes it whole when
        // that track is done, and a half-written one is simply read next time
        let mut take = |answered: &mut Vec<bool>| {
            for index in 0..audio.len() {
                if answered[index] {
                    continue;
                }
                let Ok(text) = fs::read_to_string(out_dir.join(format!("{index}.json"))) else { continue };
                if serde_json::from_str::<serde_json::Value>(&text).is_err() {
                    continue;
                }
                answered[index] = true;
                let words = whisper_words_from_json(&text);
                heard(index, if words.is_empty() { Err(anyhow!(NO_WORDS)) } else { Ok(words) });
            }
        };
        let on_card = !matches!(config.runtime, OnnxFlavour::Cpu);
        let all: Vec<PathBuf> = wavs.iter().map(|(_, wav)| wav.clone()).collect();
        let mut outcome = self.run_whisper_many(&binary, size, &all, &out_dir, language, on_card, &mut || take(&mut answered), cancel);
        let mut recognised = Recognised::OnDevice;
        if outcome.as_ref().is_err_and(|error| error.to_string() != "cancelled") && on_card {
            // the card refused: the processor takes only what is still unheard,
            // and the page is told it ran there
            take(&mut answered);
            let refused = outcome.unwrap_err();
            let left: Vec<PathBuf> = wavs.iter().filter(|(index, _)| !answered[*index]).map(|(_, wav)| wav.clone()).collect();
            outcome = if left.is_empty() {
                Ok(())
            } else {
                recognised = Recognised::OnProcessor;
                self.run_whisper_many(&binary, size, &left, &out_dir, language, false, &mut || take(&mut answered), cancel)
                    .with_context(|| format!("the card was tried first and refused: {refused}"))
            };
        }
        take(&mut answered);
        let run_error = outcome.err().map(|error| format!("{error:#}"));
        for (index, done) in answered.iter().enumerate() {
            if !done {
                heard(index, Err(anyhow!("{}", run_error.clone().unwrap_or_else(|| "whisper wrote no JSON for this track".into()))));
            }
        }
        fs::remove_dir_all(&work).ok();
        Ok(recognised)
    }

    /// Runs faster-whisper over one track and returns the words it heard.
    ///
    /// Purfview's standalone build, asked for JSON with word timestamps - the
    /// same recogniser Dub Studio uses. It replaced whisper.cpp, which was
    /// asked for an LRC file: that file is written line by line, word times had
    /// to be guessed out of it, and when the run failed the binary exited zero
    /// and wrote nothing at all, so the only thing anyone ever saw was
    /// "whisper-cli produced no LRC file".
    pub fn whisper_words(&self, config: &LyricsSyncConfig, audio: &Path, language: Option<&str>, _lyrics: &str) -> Result<Vec<(f64, String)>> {
        let binary = self.whisper_binary().ok_or_else(|| anyhow!("the Whisper runtime is not installed"))?;
        let size = config
            .whisper_model
            .as_deref()
            .and_then(Self::whisper_size)
            .ok_or_else(|| anyhow!("no Whisper model is downloaded and selected"))?;
        if self.whisper_model_path(config).is_none() {
            bail!("the Whisper model {size} is not completely downloaded");
        }

        let work = self.downloader.root().join("work");
        fs::create_dir_all(&work).with_context(|| format!("create {}", work.display()))?;
        let stem = work.join(format!("sync-{}", uuid::Uuid::now_v7()));
        let wav = stem.with_extension("wav");
        crate::audio_pcm::write_wav16k_mono(audio, &wav)
            .with_context(|| format!("decode {} for recognition", audio.display()))?;
        let out_dir = stem.with_extension("out");
        fs::remove_dir_all(&out_dir).ok();
        fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;

        let on_card = !matches!(config.runtime, OnnxFlavour::Cpu);
        let mut outcome = self.run_whisper(&binary, size, &wav, &out_dir, language, on_card);
        // CTranslate2 fails inside itself on a machine without usable CUDA, so
        // the card is tried and the processor is the answer to its refusal -
        // once, and only in that direction.
        if outcome.is_err() && on_card {
            let refused = outcome.unwrap_err();
            fs::remove_dir_all(&out_dir).ok();
            fs::create_dir_all(&out_dir).ok();
            outcome = self
                .run_whisper(&binary, size, &wav, &out_dir, language, false)
                .with_context(|| format!("the card was tried first and refused: {refused}"));
        }
        fs::remove_file(&wav).ok();

        if let Err(error) = outcome {
            fs::remove_dir_all(&out_dir).ok();
            return Err(error);
        }
        let json = fs::read_dir(&out_dir)
            .with_context(|| format!("read {}", out_dir.display()))?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .ok_or_else(|| anyhow!("whisper wrote no JSON into {}", out_dir.display()))?;
        let text = fs::read_to_string(&json).with_context(|| format!("read {}", json.display()))?;
        let words = whisper_words_from_json(&text);
        fs::remove_dir_all(&out_dir).ok();
        Ok(words)
    }

    /// One run of the recogniser, with its complaints kept: a failure here is
    /// the only place that ever says why nothing was recognised.
    fn run_whisper(&self, binary: &Path, size: &str, wav: &Path, out_dir: &Path, language: Option<&str>, on_card: bool) -> Result<()> {
        self.run_whisper_many(binary, size, &[wav.to_path_buf()], out_dir, language, on_card, &mut || {}, &AtomicBool::new(false))
    }

    /// Whisper over every file in one process; `poll` is called while it
    /// works, to pick up what it has written, and `cancel` stops it between polls.
    #[allow(clippy::too_many_arguments)]
    fn run_whisper_many(
        &self,
        binary: &Path,
        size: &str,
        wavs: &[PathBuf],
        out_dir: &Path,
        language: Option<&str>,
        on_card: bool,
        poll: &mut dyn FnMut(),
        cancel: &AtomicBool,
    ) -> Result<()> {
        let mut command = Command::new(binary);
        command
            .args(wavs)
            .arg("--model")
            .arg(size)
            .arg("--model_dir")
            .arg(self.whisper_model_dir())
            .arg("--task")
            .arg("transcribe")
            .arg("--output_format")
            .arg("json")
            .arg("--output_dir")
            .arg(out_dir)
            .arg("--word_timestamps")
            .arg("True")
            .arg("--compute_type")
            .arg(if on_card { "float16" } else { "int8" })
            .arg("--device")
            .arg(if on_card { "cuda" } else { "cpu" })
            .arg("--beep_off")
            // A hallucination fed back as the next window's prompt is how one
            // subtitler's credit becomes eight; lyrics lose nothing by it
            .args(["--condition_on_previous_text", "False"]);
        // A language it was told beats one it has to guess, and "auto" is not a
        // language code - passing it as one is how a run comes back empty.
        if let Some(code) = language.map(str::trim).filter(|code| !code.is_empty() && *code != "auto") {
            command.arg("--language").arg(code);
        }
        command
            // The weights are on this disk; a recogniser that goes looking for
            // them on the network is one that fails without one.
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .stdin(Stdio::null());
        // Its output goes to a file: a pipe nobody reads while it works through
        // a whole dataset fills up and stalls it.
        let log_path = out_dir.with_extension("log");
        let log = fs::File::create(&log_path).with_context(|| format!("create {}", log_path.display()))?;
        command.stdout(log.try_clone()?).stderr(log);
        // CTranslate2 and the CUDA libraries sit beside the binary, and that is
        // where they are found from.
        if let Some(directory) = binary.parent() {
            command.current_dir(directory);
        }
        hide_console(&mut command);

        let mut child = command.spawn().with_context(|| format!("run {}", binary.display()))?;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if cancel.load(Ordering::Relaxed) {
                child.kill().ok();
                child.wait().ok();
                bail!("cancelled");
            }
            poll();
            std::thread::sleep(std::time::Duration::from_millis(500));
        };
        let output = fs::read_to_string(&log_path).unwrap_or_default();
        fs::remove_file(&log_path).ok();
        if status.success() {
            return Ok(());
        }
        let mut tail: Vec<&str> = output.lines().filter(|line| !line.trim().is_empty()).rev().take(8).collect();
        tail.reverse();
        bail!("whisper exited with {}: {}", status, tail.join(" | "))
    }
}

/// Groups a word stream into karaoke lines: a new line at a noticeable pause,
/// at sentence-ending punctuation, or once a line grows past comfortable
/// reading length. Ported from the segmentation Dub Studio uses for subtitles.
pub fn group_words(words: &[(f64, String)]) -> Vec<(f64, String)> {
    const PAUSE: f64 = 0.6;
    const MAX_CHARS: usize = 42;

    let mut lines: Vec<(f64, String)> = Vec::new();
    let mut start = 0.0;
    let mut current = String::new();
    let mut previous: Option<f64> = None;

    for (at, word) in words {
        let pause = previous.is_some_and(|last| at - last > PAUSE);
        let too_long = current.chars().count() + word.chars().count() + 1 > MAX_CHARS;
        if !current.is_empty() && (pause || too_long) {
            lines.push((start, std::mem::take(&mut current)));
        }
        if current.is_empty() {
            start = *at;
        } else {
            current.push(' ');
        }
        current.push_str(word);
        if word.ends_with(['.', '!', '?', '…']) {
            lines.push((start, std::mem::take(&mut current)));
        }
        previous = Some(*at);
    }
    if !current.is_empty() {
        lines.push((start, current));
    }
    merge_flashing_lines(lines)
}

/// ACE Step Studio merged LRC lines that begin less than two seconds apart so
/// they do not flash past unread. Karaoke here follows the same rule.
fn merge_flashing_lines(lines: Vec<(f64, String)>) -> Vec<(f64, String)> {
    const MIN_DISPLAY_SECONDS: f64 = 2.0;
    let mut merged: Vec<(f64, String)> = Vec::with_capacity(lines.len());
    for (start, text) in lines {
        match merged.last_mut() {
            Some((previous_start, previous_text)) if start - *previous_start < MIN_DISPLAY_SECONDS => {
                previous_text.push(' ');
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

impl TimedLine {
    pub fn text(&self) -> String {
        self.words.iter().map(|(_, word)| word.as_str()).collect::<Vec<_>>().join(" ")
    }
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
/// showing nonsense. Each written line claims the earliest recognised word
/// that resembles it, scanning forward so a repeated chorus consumes its
/// occurrences in order; lines nobody could place are filled in between their
/// neighbours rather than dropped.
pub fn align_lyrics(words: &[(f64, String)], lyrics: &str) -> Vec<(f64, String)> {
    let lines: Vec<&str> = lyrics
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !(line.starts_with('[') && line.ends_with(']')))
        .collect();
    if lines.is_empty() || words.is_empty() {
        return Vec::new();
    }

    let heard: Vec<(f64, String)> = words.iter().map(|(at, word)| (*at, normalise(word))).collect();
    let mut placed: Vec<Option<f64>> = vec![None; lines.len()];
    let mut cursor = 0usize;

    for (index, line) in lines.iter().enumerate() {
        let expected: Vec<String> = line.split_whitespace().map(|word| normalise(word)).filter(|word| !word.is_empty()).collect();
        if expected.is_empty() {
            continue;
        }
        let written = expected.concat();
        let mut best: Option<(f64, usize, f64)> = None;
        for start in cursor..heard.len() {
            // A little slack either side: the recogniser splits words
            // differently from the page.
            let window = (start + expected.len() + 2).min(heard.len());
            let spoken: String = heard[start..window].iter().map(|(_, word)| word.as_str()).collect();
            let score = similarity(&written, &spoken);
            if score > best.map(|(value, _, _)| value).unwrap_or(0.0) {
                best = Some((score, start, heard[start].0));
            }
            // A line rarely starts far past where the previous one ended.
            if start > cursor + 40 {
                break;
            }
        }
        if let Some((score, start, at)) = best {
            if score >= 0.45 {
                placed[index] = Some(at);
                cursor = (start + expected.len()).min(heard.len().saturating_sub(1));
            }
        }
    }

    interpolate(&lines, &mut placed, heard.first().map(|(at, _)| *at).unwrap_or(0.0), heard.last().map(|(at, _)| *at).unwrap_or(0.0));
    lines
        .iter()
        .zip(placed)
        .filter_map(|(line, at)| at.map(|at| (at, (*line).to_string())))
        .collect()
}

/// Lines the recogniser could not place are spread evenly between the ones it
/// could, so a karaoke file has no silent holes.
fn interpolate(lines: &[&str], placed: &mut [Option<f64>], first: f64, last: f64) {
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
        let before = gap_start.checked_sub(1).and_then(|previous| placed[previous]).unwrap_or(first);
        let after = placed.get(index).copied().flatten().unwrap_or(last.max(before));
        let steps = (index - gap_start + 1) as f64;
        for (offset, slot) in placed[gap_start..index].iter_mut().enumerate() {
            *slot = Some(before + (after - before) * ((offset + 1) as f64 / steps));
        }
    }
}

/// A word without its last character, for tolerating inflected endings.
fn stem(word: &str) -> String {
    let count = word.chars().count();
    word.chars().take(count.saturating_sub(1)).collect()
}

fn normalise(word: &str) -> String {
    word.chars().filter(|character| character.is_alphanumeric()).flat_map(char::to_lowercase).collect()
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

/// Turns timed segments into an LRC body. Used by the providers that return
/// structured timings rather than a file.
pub fn lrc_from_segments(segments: &[(f64, String)]) -> String {
    let mut out = String::new();
    for (start, text) in segments {
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let total = start.max(0.0);
        let minutes = (total as u64) / 60;
        let seconds = (total as u64) % 60;
        let hundredths = ((total - total.floor()) * 100.0).round() as u64;
        out.push_str(&format!("[{minutes:02}:{seconds:02}.{hundredths:02}]{text}\n"));
    }
    out
}

/// The opening lines of the lyrics, as a decoding hint. Whisper's prompt is
/// bounded, and the first lines are enough to tell it this is singing, in this
/// language, about these words.
fn prompt_from(lyrics: &str) -> String {
    let mut prompt = String::new();
    for line in lyrics.lines().map(str::trim).filter(|line| !line.is_empty() && !(line.starts_with('[') && line.ends_with(']'))) {
        if prompt.chars().count() + line.chars().count() > 400 {
            break;
        }
        if !prompt.is_empty() {
            prompt.push(' ');
        }
        prompt.push_str(line);
    }
    prompt
}

/// Reads a whisper.cpp LRC back as a word stream.
pub fn words_from_lrc(lrc: &str) -> Vec<(f64, String)> {
    let mut words = Vec::new();
    for line in lrc.lines() {
        let Some(rest) = line.strip_prefix('[') else { continue };
        let Some((stamp, text)) = rest.split_once(']') else { continue };
        let Some((minutes, seconds)) = stamp.split_once(':') else { continue };
        let (Ok(minutes), Ok(seconds)) = (minutes.trim().parse::<f64>(), seconds.trim().parse::<f64>()) else {
            continue;
        };
        let text = text.trim();
        if !text.is_empty() {
            words.push((minutes * 60.0 + seconds, text.to_string()));
        }
    }
    words
}

/// The timestamps in an LRC body, in seconds. Also the emptiness check: a file
/// with no timestamps is not karaoke, however much text it contains.
pub fn parse_lrc_times(lrc: &str) -> Vec<f64> {
    let mut times = Vec::new();
    for line in lrc.lines() {
        let Some(rest) = line.strip_prefix('[') else { continue };
        let Some((stamp, _)) = rest.split_once(']') else { continue };
        let Some((minutes, seconds)) = stamp.split_once(':') else { continue };
        let (Ok(minutes), Ok(seconds)) = (minutes.trim().parse::<f64>(), seconds.trim().parse::<f64>()) else {
            continue;
        };
        times.push(minutes * 60.0 + seconds);
    }
    times
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
        let json = r#"{"segments":[{"start":1.0,"text":" Субтитры создавал DimaTorzok","words":[{"start":1.0,"word":" Субтитры"}]},{"start":5.0,"text":" Тьма во мне","words":[{"start":5.0,"word":" Тьма"},{"start":5.4,"word":" во"},{"start":5.6,"word":" мне"}]}]}"#;
        let words = whisper_words_from_json(json);
        assert_eq!(words.iter().map(|(_, word)| word.as_str()).collect::<Vec<_>>(), ["Тьма", "во", "мне"]);
    }

    #[test]
    fn segments_become_a_playable_lrc_body() {
        let lrc = lrc_from_segments(&[
            (0.0, "neon on the glass".into()),
            (12.5, "driving home".into()),
            (75.25, "  ".into()),
            (81.5, "the engine dies".into()),
        ]);
        assert_eq!(lrc, "[00:00.00]neon on the glass\n[00:12.50]driving home\n[01:21.50]the engine dies\n");
        assert_eq!(parse_lrc_times(&lrc), vec![0.0, 12.5, 81.5]);
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

    #[test]
    fn text_without_timestamps_is_not_karaoke() {
        assert!(parse_lrc_times("just some lyrics\nand another line").is_empty());
    }

    #[test]
    fn the_whisper_runtime_is_pinned_and_every_asset_is_distinct() {
        for entry in ASSETS {
            assert!(entry.bytes > 0, "{} has no size", entry.id);
            assert!(entry.url.starts_with("https://"), "{} is not fetched over https", entry.id);
            assert_eq!(ASSETS.iter().filter(|other| other.id == entry.id).count(), 1);
            if entry.kind == AssetKind::Runtime {
                // Every runtime is pinned to an exact release - whisper.cpp to
                // its tag, ONNX Runtime to its version - so a working setup
                // keeps working.
                // NVIDIA's libraries are pinned by their own version in the
                // archive name, the same way the others are.
                let pinned = entry.url.contains(WHISPER_BUILD)
                    || entry.url.contains(WHISPER_CUBLAS_BUILD)
                    || entry.url.contains(WHISPER_CUDNN_BUILD)
                    || entry.url.contains(ONNXRUNTIME_BUILD)
                    || entry.url.contains(CUBLAS_BUILD)
                    || entry.url.contains(CUDART_BUILD)
                    || entry.url.contains(CUDNN_BUILD)
                    || entry.url.contains(CUFFT_BUILD);
                assert!(pinned, "{} is not pinned to a release", entry.id);
                assert!(!entry.marker.is_empty(), "{} has no proof of extraction", entry.id);
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
}

#[cfg(test)]
mod live_recognition {
    use super::*;

    /// The whole recognition path against a real installation, when one is
    /// pointed at: decode, run the recogniser, read its JSON, get words with
    /// times. Checking the download and the binary separately is what let a
    /// finished installation still refuse to run.
    #[test]
    fn recognising_a_real_track_end_to_end() {
        let (Some(root), Some(track)) = (std::env::var_os("STUDIO_TEST_DATA_ROOT"), std::env::var_os("STUDIO_TEST_TRACK")) else { return };
        let sync = LyricsSync::new(std::path::Path::new(&root));
        let config = LyricsSyncConfig {
            enabled: true,
            provider: AsrProvider::Whisper,
            whisper_model: Some("whisper-large-v3".into()),
            runtime: OnnxFlavour::Cuda,
            ..Default::default()
        };
        assert!(sync.whisper_binary().is_some(), "the recogniser is not installed");
        assert!(sync.whisper_model_ready(&config), "the model is not considered ready");
        let words = sync
            .whisper_words(&config, std::path::Path::new(&track), Some("ru"), "")
            .expect("recognition");
        eprintln!("words: {}", words.len());
        for (at, word) in words.iter().take(8) {
            eprintln!("  {at:.2}s {word}");
        }
        assert!(!words.is_empty(), "nothing was recognised");
    }
}
