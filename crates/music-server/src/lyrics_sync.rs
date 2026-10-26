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
        .filter(|word| !made_up.iter().any(|(start, end)| word.start >= *start && word.start <= *end))
        .map(|word| (word.start, word.text.clone()))
        .collect()
}

/// The build of audio.cpp's server karaoke runs on for `runtime`: the processor when it is chosen; else the card's
/// CUDA build (on Linux CUDA 13 alone, as the engine ships it), Vulkan on a card CUDA does not run (AMD, Intel), the
/// processor without a card; a Mac its Metal program.
pub fn crisper_build(runtime: OnnxFlavour) -> Result<acpp_server::asset::Build> {
    use acpp_server::asset::Build;
    if let Some(mac) = Build::for_this_mac() {
        return Ok(mac);
    }
    if cfg!(target_os = "macos") {
        bail!("the audio.cpp server is published for Apple silicon only");
    }
    if matches!(runtime, OnnxFlavour::Cpu) {
        return Ok(Build::Cpu);
    }
    Ok(match crate::cuda_build::current() {
        Some(crate::cuda_build::CudaBuild::Cuda13) => Build::Cuda13,
        Some(crate::cuda_build::CudaBuild::Cuda12) if cfg!(windows) => Build::Cuda12,
        _ if crate::presets::display_card().is_some() => Build::Vulkan,
        _ => Build::Cpu,
    })
}

/// What CrisperWhisper needs on `runtime`'s build: the server and the CUDA libraries it does not carry, and every file
/// of the model.
pub fn crisper_assets(runtime: OnnxFlavour) -> Vec<&'static str> {
    use acpp_server::asset::Build;
    let Ok(build) = crisper_build(runtime) else { return Vec::new() };
    let server: &[&str] = match build {
        Build::Cuda13 => &["audiocpp-cuda13", "audiocpp-cuda13-cufft"],
        Build::Cuda12 => &["audiocpp-cuda12", "audiocpp-cuda12-cudart", "audiocpp-cuda12-cublas", "audiocpp-cuda12-cufft"],
        Build::Vulkan => &["audiocpp-vulkan"],
        Build::Cpu => &["audiocpp-cpu"],
        Build::Metal => &["audiocpp-metal"],
    };
    let model = ASSETS.iter().filter(|asset| asset.relative_path.starts_with(CRISPERWHISPER_DIR)).map(|asset| asset.id);
    server.iter().copied().chain(model).collect()
}

/// CrisperWhisper's memory freed on every running server; its next request loads it again.
fn unload_crisper() {
    for (pid, error) in acpp_server::SERVERS.unload(&[CRISPERWHISPER]) {
        studio_log!("[ERROR] audiocpp_server {pid} did not unload CrisperWhisper: {error}");
    }
}

pub const ASSETS: &[Asset] = &[
    // CrisperWhisper runs in audio.cpp's server, the build of the card or the processor; the archives and CUDA
    // libraries are the shared client's pins (acpp_server::asset), one folder a build as Dub Studio and Speech Studio
    // lay them out.
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda13",
        label: "audio.cpp server (CUDA 13)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_WIN_CUDA13.url,
        relative_path: "runtime/audiocpp-cuda13.zip",
        bytes: acpp_server::asset::SERVER_WIN_CUDA13.size,
        unzip_into: Some(acpp_server::asset::Build::Cuda13.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on an NVIDIA card from the GTX 16 and RTX 20 series on.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda13-cufft",
        label: "NVIDIA cuFFT 12.4 (for the audio.cpp server)",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-12.4.0.43-archive.zip",
        relative_path: "runtime/audiocpp-cuda13-cufft.zip",
        bytes: 159_602_693,
        unzip_into: Some(acpp_server::asset::Build::Cuda13.folder()),
        marker: "cufft64_12.dll",
        pick: &["cufft64_12.dll"],
        vram_gb: None,
        note: "The Fourier transforms of the CUDA 13 server; its cuBLAS is the music engine's.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda12",
        label: "audio.cpp server (CUDA 12)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_WIN_CUDA12.url,
        relative_path: "runtime/audiocpp-cuda12.zip",
        bytes: acpp_server::asset::SERVER_WIN_CUDA12.size,
        unzip_into: Some(acpp_server::asset::Build::Cuda12.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on the NVIDIA cards and drivers CUDA 13 does not.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda12-cudart",
        label: "NVIDIA CUDA runtime 12.9 (for the audio.cpp server)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::CUDA12_RUNTIME[0].url,
        relative_path: "runtime/audiocpp-cuda12-cudart.whl",
        bytes: acpp_server::asset::CUDA12_RUNTIME[0].size,
        unzip_into: Some(acpp_server::asset::Build::Cuda12.folder()),
        marker: "cudart64_12.dll",
        pick: &["cudart64_12.dll"],
        vram_gb: None,
        note: "Part of the CUDA 12 server.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda12-cublas",
        label: "NVIDIA cuBLAS 12.9 (for the audio.cpp server)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::CUDA12_RUNTIME[1].url,
        relative_path: "runtime/audiocpp-cuda12-cublas.whl",
        bytes: acpp_server::asset::CUDA12_RUNTIME[1].size,
        unzip_into: Some(acpp_server::asset::Build::Cuda12.folder()),
        marker: "cublasLt64_12.dll",
        pick: &["cublas64_12.dll", "cublasLt64_12.dll"],
        vram_gb: None,
        note: "Part of the CUDA 12 server.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda12-cufft",
        label: "NVIDIA cuFFT 11.4 (for the audio.cpp server)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::CUDA12_RUNTIME[2].url,
        relative_path: "runtime/audiocpp-cuda12-cufft.whl",
        bytes: acpp_server::asset::CUDA12_RUNTIME[2].size,
        unzip_into: Some(acpp_server::asset::Build::Cuda12.folder()),
        marker: "cufft64_11.dll",
        pick: &["cufft64_11.dll"],
        vram_gb: None,
        note: "Part of the CUDA 12 server.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-vulkan",
        label: "audio.cpp server (Vulkan)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_WIN_VULKAN.url,
        relative_path: "runtime/audiocpp-vulkan.zip",
        bytes: acpp_server::asset::SERVER_WIN_VULKAN.size,
        unzip_into: Some(acpp_server::asset::Build::Vulkan.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on an AMD or Intel card.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cpu",
        label: "audio.cpp server (processor)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_WIN_CPU.url,
        relative_path: "runtime/audiocpp-cpu.zip",
        bytes: acpp_server::asset::SERVER_WIN_CPU.size,
        unzip_into: Some(acpp_server::asset::Build::Cpu.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on the processor: slower, the same words.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "audiocpp-cuda13",
        label: "audio.cpp server (CUDA 13)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_LINUX_CUDA13.url,
        relative_path: "runtime/audiocpp-cuda13.tar.gz",
        bytes: acpp_server::asset::SERVER_LINUX_CUDA13.size,
        unzip_into: Some(acpp_server::asset::Build::Cuda13.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on an NVIDIA card from the GTX 16 and RTX 20 series on.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "audiocpp-cuda13-cufft",
        label: "NVIDIA cuFFT 12.4 (for the audio.cpp server)",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/libcufft/linux-x86_64/libcufft-linux-x86_64-12.4.0.43-archive.tar.xz",
        relative_path: "runtime/audiocpp-cuda13-cufft.tar.xz",
        bytes: 249_327_988,
        unzip_into: Some(acpp_server::asset::Build::Cuda13.folder()),
        marker: "libcufft.so.12",
        pick: &["libcufft.so.12"],
        vram_gb: None,
        note: "The Fourier transforms of the CUDA 13 server; its cuBLAS is the music engine's.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "audiocpp-vulkan",
        label: "audio.cpp server (Vulkan)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_LINUX_VULKAN.url,
        relative_path: "runtime/audiocpp-vulkan.tar.gz",
        bytes: acpp_server::asset::SERVER_LINUX_VULKAN.size,
        unzip_into: Some(acpp_server::asset::Build::Vulkan.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on an AMD or Intel card.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "audiocpp-cpu",
        label: "audio.cpp server (processor)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_LINUX_CPU.url,
        relative_path: "runtime/audiocpp-cpu.tar.gz",
        bytes: acpp_server::asset::SERVER_LINUX_CPU.size,
        unzip_into: Some(acpp_server::asset::Build::Cpu.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on the processor: slower, the same words.",
    },
    #[cfg(target_os = "macos")]
    Asset {
        id: "audiocpp-metal",
        label: "audio.cpp server (Metal)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_MACOS.url,
        relative_path: "runtime/audiocpp-metal.tar.gz",
        bytes: acpp_server::asset::SERVER_MACOS.size,
        unzip_into: Some(acpp_server::asset::Build::Metal.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on Apple silicon: Metal and the processor in one program.",
    },
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
        unzip_into: None,
        marker: "",
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
        unzip_into: None,
        marker: "",
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
        unzip_into: None,
        marker: "",
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
        id: "onnxruntime-cuda",
        label: "ONNX Runtime 1.30.0 · CUDA",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-win-x64-gpu_cuda12-1.30.0.zip",
        relative_path: "runtime/onnxruntime-cuda.zip",
        bytes: 379_723_801,
        unzip_into: Some("onnx-cuda"),
        marker: "onnxruntime_providers_cuda.dll",
        pick: &["onnxruntime.dll", "onnxruntime_providers_shared.dll", "onnxruntime_providers_cuda.dll"],
        vram_gb: Some(2),
        note: "Runs the separator on an NVIDIA card instead of the processor. Needs CUDA 12.",
    },
    #[cfg(windows)]
    Asset {
        id: "cuda-cublas",
        label: "NVIDIA cuBLAS 12.9",
        kind: AssetKind::Runtime,
        // NVIDIA's PyPI wheel (a zip): its redistributable archive of 12.9.2.10 has no published SHA-256
        url: "https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl",
        relative_path: "runtime/cuda-cublas.whl",
        bytes: 553_162_896,
        unzip_into: Some("onnx-cuda"),
        marker: "cublasLt64_12.dll",
        pick: &["cublasLt64_12.dll", "cublas64_12.dll"],
        vram_gb: None,
        note: "The linear algebra the CUDA provider is built on.",
    },
    #[cfg(windows)]
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
    #[cfg(windows)]
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
    #[cfg(windows)]
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
    #[cfg(windows)]
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
    #[cfg(target_os = "macos")]
    Asset {
        id: "onnxruntime",
        label: "ONNX Runtime 1.30.0",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-osx-arm64-1.30.0.tgz",
        relative_path: "runtime/onnxruntime.tgz",
        bytes: 42_373_116,
        unzip_into: Some("onnx"),
        marker: "libonnxruntime",
        pick: &[],
        vram_gb: None,
        note: "Parakeet runs on this; it is loaded at run time, not linked in.",
    },
    // The card path for every card that is not an NVIDIA one running CUDA:
    // DirectML over DirectX 12. 1.24.4 is the last DirectML build Microsoft
    // publishes; it speaks the same API level (24) as the builds above.
    #[cfg(windows)]
    Asset {
        id: "onnxruntime-directml",
        label: "ONNX Runtime 1.24.4 · DirectML",
        kind: AssetKind::Runtime,
        url: "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.24.4/microsoft.ml.onnxruntime.directml.1.24.4.nupkg",
        relative_path: "runtime/onnxruntime-directml.nupkg",
        bytes: 12_458_649,
        unzip_into: Some("onnx-dml"),
        marker: "onnxruntime.dll",
        pick: &["runtimes/win-x64/native/onnxruntime.dll", "runtimes/win-x64/native/onnxruntime_providers_shared.dll"],
        vram_gb: None,
        note: "Runs karaoke's Parakeet and the tempo model on an AMD or Intel card through DirectX 12.",
    },
    #[cfg(windows)]
    Asset {
        id: "directml",
        label: "DirectML 1.15.4",
        kind: AssetKind::Runtime,
        url: "https://api.nuget.org/v3-flatcontainer/microsoft.ai.directml/1.15.4/microsoft.ai.directml.1.15.4.nupkg",
        relative_path: "runtime/directml.nupkg",
        bytes: 202_292_617,
        unzip_into: Some("onnx-dml"),
        marker: "DirectML.dll",
        // the package carries the library for Xbox too, under the same name
        pick: &["bin/x64-win/DirectML.dll"],
        vram_gb: None,
        note: "The DirectML the runtime above is built for; the copy inside Windows is older.",
    },
    // Linux: the same versions as the Windows builds above, from the same
    // places - Microsoft's tarballs and NVIDIA's linux-x86_64 redistributables.
    #[cfg(target_os = "linux")]
    Asset {
        id: "onnxruntime-cuda",
        label: "ONNX Runtime 1.30.0 · CUDA",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-x64-gpu_cuda12-1.30.0.tgz",
        relative_path: "runtime/onnxruntime-cuda.tgz",
        bytes: 439_354_926,
        unzip_into: Some("onnx-cuda"),
        marker: "libonnxruntime_providers_cuda.so",
        pick: &[],
        vram_gb: Some(2),
        note: "Runs the separator on an NVIDIA card instead of the processor. Needs CUDA 12.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "cuda-cublas",
        label: "NVIDIA cuBLAS 12.9",
        kind: AssetKind::Runtime,
        // NVIDIA's PyPI wheel (a zip): its redistributable archive of 12.9.2.10 has no published SHA-256
        url: "https://files.pythonhosted.org/packages/cb/c0/0a517bfe63ccd3b92eb254d264e28fca3c7cab75d07daea315250fb1bf73/nvidia_cublas_cu12-12.9.2.10-py3-none-manylinux_2_27_x86_64.whl",
        relative_path: "runtime/cuda-cublas.whl",
        bytes: 581_240_110,
        unzip_into: Some("onnx-cuda"),
        marker: "libcublasLt.so.12",
        pick: &["libcublasLt.so.12", "libcublas.so.12"],
        vram_gb: None,
        note: "The linear algebra the CUDA provider is built on.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "cuda-cudart",
        label: "NVIDIA CUDA runtime 12.9",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/cuda_cudart/linux-x86_64/cuda_cudart-linux-x86_64-12.9.79-archive.tar.xz",
        relative_path: "runtime/cuda-cudart.tar.xz",
        bytes: 1_514_676,
        unzip_into: Some("onnx-cuda"),
        marker: "libcudart.so.12",
        pick: &["libcudart.so.12"],
        vram_gb: None,
        note: "The CUDA runtime itself.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "cuda-cufft",
        label: "NVIDIA cuFFT 11.4",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/libcufft/linux-x86_64/libcufft-linux-x86_64-11.4.1.4-archive.tar.xz",
        relative_path: "runtime/cuda-cufft.tar.xz",
        bytes: 470_942_192,
        unzip_into: Some("onnx-cuda"),
        marker: "libcufft.so.11",
        pick: &["libcufft.so.11"],
        vram_gb: None,
        note: "The transforms the provider uses for spectral work.",
    },
    // The Linux provider links cuRAND by name; the Windows one does not.
    #[cfg(target_os = "linux")]
    Asset {
        id: "cuda-curand",
        label: "NVIDIA cuRAND 10.3",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/libcurand/linux-x86_64/libcurand-linux-x86_64-10.3.10.19-archive.tar.xz",
        relative_path: "runtime/cuda-curand.tar.xz",
        bytes: 89_582_256,
        unzip_into: Some("onnx-cuda"),
        marker: "libcurand.so.10",
        pick: &["libcurand.so.10"],
        vram_gb: None,
        note: "Random numbers, which the CUDA provider is linked against.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "cuda-cudnn",
        label: "NVIDIA cuDNN 9.25",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/linux-x86_64/cudnn-linux-x86_64-9.25.0.15_cuda12-archive.tar.xz",
        relative_path: "runtime/cuda-cudnn.tar.xz",
        bytes: 987_863_504,
        unzip_into: Some("onnx-cuda"),
        marker: "libcudnn.so.9",
        pick: &[
            "libcudnn.so.9",
            "libcudnn_graph.so.9",
            "libcudnn_ops.so.9",
            "libcudnn_cnn.so.9",
            "libcudnn_heuristic.so.9",
            "libcudnn_engines_precompiled.so.9",
            "libcudnn_engines_runtime_compiled.so.9",
            "libcudnn_engines_tensor_ir.so.9",
            "libcudnn_ext.so.9",
        ],
        vram_gb: None,
        note: "The convolution kernels the separator spends its time in.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "onnxruntime",
        label: "ONNX Runtime 1.30.0",
        kind: AssetKind::Runtime,
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-x64-1.30.0.tgz",
        relative_path: "runtime/onnxruntime.tgz",
        bytes: 11_306_877,
        unzip_into: Some("onnx"),
        marker: "libonnxruntime.so",
        pick: &[],
        vram_gb: None,
        note: "Parakeet runs on this; it is loaded at run time, not linked in.",
    },
];

/// The file names of this platform's ONNX Runtime and of the libraries its
/// CUDA provider loads.
#[cfg(target_os = "macos")]
mod libraries {
    pub const ORT: &str = "libonnxruntime.dylib";
    pub const ORT_SHARED: &str = "libonnxruntime_providers_shared.dylib";
    pub const CUDA: &[&str] = &[];
}

#[cfg(windows)]
mod libraries {
    pub const ORT: &str = "onnxruntime.dll";
    pub const ORT_SHARED: &str = "onnxruntime_providers_shared.dll";
    pub const CUDA: &[&str] = &["onnxruntime_providers_cuda.dll", "cublasLt64_12.dll", "cudart64_12.dll", "cudnn64_9.dll"];
}

#[cfg(target_os = "linux")]
mod libraries {
    pub const ORT: &str = "libonnxruntime.so";
    pub const ORT_SHARED: &str = "libonnxruntime_providers_shared.so";
    pub const CUDA: &[&str] =
        &["libonnxruntime_providers_cuda.so", "libcublasLt.so.12", "libcublas.so.12", "libcudart.so.12", "libcufft.so.11", "libcurand.so.10", "libcudnn.so.9"];
}

/// The CUDA provider's parts, in the order they are used: `karaoke_set` and the
/// separator build their card path out of these.
#[cfg(target_os = "macos")]
pub const CUDA_ASSETS: &[&str] = &[];
#[cfg(windows)]
pub const CUDA_ASSETS: &[&str] = &["onnxruntime-cuda", "cuda-cudart", "cuda-cublas", "cuda-cufft", "cuda-cudnn"];
#[cfg(target_os = "linux")]
pub const CUDA_ASSETS: &[&str] = &["onnxruntime-cuda", "cuda-cudart", "cuda-cublas", "cuda-cufft", "cuda-curand", "cuda-cudnn"];

/// Everything the DirectML path needs, in the order it is used.
pub const DIRECTML_ASSETS: [&str; 2] = ["onnxruntime-directml", "directml"];

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

    /// Whether work runs on the card through DirectML: every card CUDA does
    /// not run on - AMD, Intel, an NVIDIA card whose driver is too old - as
    /// long as there is a card at all.
    pub fn uses_directml(self) -> bool {
        cfg!(windows) && !matches!(self, OnnxFlavour::Cpu) && crate::cuda_build::current().is_none() && crate::presets::display_card().is_some()
    }

    /// The card path this choice takes on this machine; none is the processor.
    pub fn card(self) -> Option<OnnxCard> {
        if self.uses_cuda() {
            Some(OnnxCard::Cuda)
        } else if self.uses_directml() {
            Some(OnnxCard::DirectMl)
        } else {
            None
        }
    }
}

/// How an ONNX Runtime build reaches the graphics card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnnxCard {
    Cuda,
    DirectMl,
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

/// A DXGI adapter tests pin DirectML to - the integrated Radeon beside an
/// NVIDIA card - where the studio takes the fastest card.
#[cfg(test)]
static DIRECTML_ADAPTER: std::sync::OnceLock<i32> = std::sync::OnceLock::new();

/// Puts a session on the card a path reaches: CUDA, or DirectML with what it
/// requires - no memory pattern, one operator at a time - on the fastest card
/// DirectX reports. Says whether the card took it: a card that refuses runs
/// the work on the processor and the caller reports that, instead of it
/// quietly taking ten times as long.
pub fn with_card(builder: ort::session::builder::SessionBuilder, card: Option<OnnxCard>) -> (ort::session::builder::SessionBuilder, bool) {
    let attempt = match card {
        None => return (builder, false),
        Some(OnnxCard::Cuda) => builder.clone().with_execution_providers([ort::ep::CUDA::default().build().error_on_failure()]),
        Some(OnnxCard::DirectMl) => {
            let provider = ort::ep::DirectML::default()
                .with_performance_preference(ort::ep::directml::PerformancePreference::HighPerformance)
                .with_device_filter(ort::ep::directml::DeviceFilter::Gpu);
            #[cfg(test)]
            let provider = match DIRECTML_ADAPTER.get() {
                Some(&adapter) => provider.with_device_id(adapter),
                None => provider,
            };
            builder
                .clone()
                .with_memory_pattern(false)
                .and_then(|builder| builder.with_parallel_execution(false))
                .and_then(|builder| builder.with_execution_providers([provider.build().error_on_failure()]))
        }
    };
    match attempt {
        Ok(on_card) => (on_card, true),
        Err(error) => {
            studio_log!("[ERROR] the {card:?} provider did not register, the processor runs this instead: {error}");
            (builder, false)
        }
    }
}

fn point_ort_at(runtime: &Path, card: Option<OnnxCard>) {
    unsafe { std::env::set_var("ORT_DYLIB_PATH", runtime) };
    let Some(directory) = runtime.parent() else { return };
    #[cfg(windows)]
    {
        let existing = std::env::var("PATH").unwrap_or_default();
        unsafe { std::env::set_var("PATH", format!("{};{existing}", directory.display())) };
        if card == Some(OnnxCard::DirectMl) {
            preload(&directory.join("DirectML.dll"));
        }
    }
    #[cfg(target_os = "linux")]
    if card == Some(OnnxCard::Cuda) {
        preload_cuda(directory);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = (directory, card);
}

/// Linux reads LD_LIBRARY_PATH once, at process start, and the CUDA provider
/// carries no path to its libraries. A library already loaded is what the
/// loader hands every later request for its name, so each one in the folder
/// is loaded here by its full path first, dependencies before dependants -
/// in passes, because which needs which is the libraries' business.
#[cfg(target_os = "linux")]
fn preload_cuda(directory: &Path) {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn dlopen(filename: *const std::ffi::c_char, flag: std::ffi::c_int) -> *mut std::ffi::c_void;
        fn dlerror() -> *const std::ffi::c_char;
    }
    const RTLD_NOW: std::ffi::c_int = 2;
    const RTLD_GLOBAL: std::ffi::c_int = 0x100;
    let mut pending: Vec<PathBuf> = fs::read_dir(directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    // the loader's names only: `libcudnn_ops.so.9`, not the versioned file it links to
                    !name.starts_with("libonnxruntime") && name.split_once(".so.").is_some_and(|(_, version)| !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit()))
                })
                .collect()
        })
        .unwrap_or_default();
    while !pending.is_empty() {
        let before = pending.len();
        let mut errors = Vec::new();
        pending.retain(|library| {
            let Ok(name) = std::ffi::CString::new(library.as_os_str().as_bytes()) else { return false };
            if !unsafe { dlopen(name.as_ptr(), RTLD_NOW | RTLD_GLOBAL) }.is_null() {
                return false;
            }
            let error = unsafe { dlerror() };
            errors.push(if error.is_null() { library.display().to_string() } else { unsafe { std::ffi::CStr::from_ptr(error) }.to_string_lossy().into_owned() });
            true
        });
        if pending.len() == before {
            studio_log!("[ERROR] the CUDA libraries did not load, the provider will not register: {}", errors.join("; "));
            return;
        }
    }
}

#[cfg(windows)]
fn preload(library: &Path) {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryExW(name: *const u16, file: *mut std::ffi::c_void, flags: u32) -> *mut std::ffi::c_void;
    }
    const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x8;
    let wide: Vec<u16> = library.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) }.is_null() {
        studio_log!("[ERROR] could not load {}: {}", library.display(), std::io::Error::last_os_error());
    }
}

/// The dropdown's Parakeet ids, as settings have held them since the ONNX builds: the v3 model, Ultra, and the
/// fp32 choice of before, which is the v3 model now that full weights are not kept.
pub const PARAKEET_V3: &str = "parakeet-tdt-int8";
pub const PARAKEET_ULTRA: &str = "parakeet-ultra-int8";
pub const PARAKEET_FP32: &str = "parakeet-tdt-fp32";

/// The folders of the ONNX Parakeet of 3.6 and earlier, under the karaoke folder.
/// Parakeet on ONNX Runtime, before transcribe.cpp: removed by the update screen.
const RETIRED: &[&str] = &["models/parakeet", "models/parakeet-fp32", "models/parakeet-ultra"];

/// Whisper-Faster (CTranslate2) with its CUDA 11 cuBLAS and cuDNN 8, and its models, before CrisperWhisper on
/// audio.cpp: removed by the update screen once CrisperWhisper is in place or karaoke hears with another recogniser.
pub const RETIRED_WHISPER: &[&str] = &[
    "runtime/whisper",
    "models/whisper",
    "runtime/whisper-faster.zip",
    "runtime/whisper-faster.zip.json",
    "runtime/whisper-faster.part",
    "runtime/whisper-cublas.zip",
    "runtime/whisper-cublas.zip.json",
    "runtime/whisper-cublas.part",
    "runtime/whisper-cublas.tar.xz",
    "runtime/whisper-cublas.tar.xz.json",
    "runtime/whisper-cublas.tar.part",
    "runtime/whisper-cudnn.zip",
    "runtime/whisper-cudnn.zip.json",
    "runtime/whisper-cudnn.part",
    "runtime/whisper-cudnn.tar.xz",
    "runtime/whisper-cudnn.tar.xz.json",
    "runtime/whisper-cudnn.tar.part",
];

/// Where each transcribe.cpp build lands in the karaoke folder: `dub_tcpp::engine_dir` and its library.
#[cfg(windows)]
const TCPP_CUDA: &str = "models/transcribe-engine/transcribe_engine.dll";
#[cfg(windows)]
const TCPP_VULKAN: &str = "models/transcribe-engine-vulkan/transcribe_engine.dll";
#[cfg(windows)]
const TCPP_CPU: &str = "models/transcribe-engine-cpu/transcribe_engine.dll";
#[cfg(target_os = "linux")]
const TCPP_CUDA: &str = "models/transcribe-engine/libtranscribe_engine.so";
#[cfg(target_os = "linux")]
const TCPP_VULKAN: &str = "models/transcribe-engine-vulkan/libtranscribe_engine.so";
#[cfg(target_os = "linux")]
const TCPP_CPU: &str = "models/transcribe-engine-cpu/libtranscribe_engine.so";
#[cfg(target_os = "macos")]
const TCPP_CPU: &str = "models/transcribe-engine-cpu/libtranscribe_engine.dylib";

/// The transcribe.cpp model the dropdown names.
pub fn parakeet_model(variant: Option<&str>) -> &'static dub_tcpp::Model {
    let id = if variant == Some(PARAKEET_ULTRA) { "parakeet-ultra" } else { "parakeet-v3" };
    dub_tcpp::model(id).expect("the shared binding carries both Parakeets")
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
    /// Where the music engine's cuBLAS 13 is, which transcribe.cpp's CUDA build imports.
    cuda_libraries: std::sync::OnceLock<PathBuf>,
}

impl LyricsSync {
    pub fn new(data_root: &Path) -> Self {
        Self { downloader: Downloader::new(data_root.join("karaoke")), cuda_libraries: std::sync::OnceLock::new() }
    }

    /// What former releases left in the karaoke folder, by the names the update screen's table of changes knows:
    /// the ONNX Parakeet and Faster-Whisper with its CUDA 11 libraries and models.
    pub fn leftovers(&self) -> Vec<crate::update_screen::Leftover> {
        let found = |listed: &[&str]| -> Vec<PathBuf> {
            listed.iter().map(|relative| self.downloader.root().join(relative)).filter(|path| fs::symlink_metadata(path).is_ok()).collect()
        };
        vec![
            crate::update_screen::Leftover { what: "parakeet-onnx", name: "Parakeet (ONNX)", paths: found(RETIRED) },
            crate::update_screen::Leftover { what: "faster-whisper", name: "Faster-Whisper", paths: found(RETIRED_WHISPER) },
        ]
    }

    /// Removes the leftovers named in `what`: the update screen's last step, after the downloads that take their
    /// place. A link (another program's folder linked in) loses only the link; a hard-linked file stays with the
    /// program that shares it.
    pub fn remove_leftovers(&self, what: &[&str]) {
        for leftover in self.leftovers().into_iter().filter(|leftover| what.contains(&leftover.what)) {
            for path in leftover.paths {
                let Ok(meta) = fs::symlink_metadata(&path) else { continue };
                let removed = if meta.file_type().is_symlink() {
                    fs::remove_dir(&path).or_else(|_| fs::remove_file(&path))
                } else if meta.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
                match removed {
                    Ok(()) => studio_log!("removed the retired {}", path.display()),
                    Err(error) => studio_log!("[ERROR] could not remove the retired {}: {error}", path.display()),
                }
            }
        }
    }

    /// The folder of the cuBLAS 13 the music engine runs on: transcribe.cpp's CUDA build takes links to it.
    pub fn set_cuda_libraries(&self, folder: PathBuf) {
        self.cuda_libraries.set(folder).ok();
    }

    /// Windows looks for a library's imports beside it: the CUDA build gets links to the engine's cuBLAS 13, no
    /// copies, unless the files are on PATH already.
    fn link_cuda13(&self, engine_dir: &Path, who: &str) -> Result<()> {
        if !cfg!(windows) {
            return Ok(());
        }
        let on_path = |name: &str| std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()));
        for name in crate::engine_runtime::CUDA13_LIBRARIES {
            let target = engine_dir.join(name);
            if target.is_file() || on_path(name) {
                continue;
            }
            let source = self.cuda_libraries.get().map(|folder| folder.join(name)).filter(|source| source.is_file());
            let Some(source) = source else {
                bail!("{who} on the card needs {name}, which comes with the music engine's CUDA 13 runtime: download it in Settings - Local engine, or choose the processor");
            };
            if fs::hard_link(&source, &target).is_err() {
                fs::copy(&source, &target).with_context(|| format!("copy {} beside transcribe.cpp", source.display()))?;
            }
        }
        Ok(())
    }

    pub fn downloader(&self) -> &Downloader {
        &self.downloader
    }

    /// The audio.cpp server of the build `runtime` runs on, when it is downloaded.
    pub fn crisper_server_exe(&self, runtime: OnnxFlavour) -> Option<PathBuf> {
        let build = crisper_build(runtime).ok()?;
        let exe = self.downloader.runtime_dir(build.folder()).join(if cfg!(windows) { "audiocpp_server.exe" } else { "audiocpp_server" });
        exe.is_file().then_some(exe)
    }

    /// CrisperWhisper runs here: the server of this choice's build, the CUDA libraries it does not carry and every
    /// file of the model are on disk.
    pub fn crisper_ready(&self, runtime: OnnxFlavour) -> bool {
        let wanted = crisper_assets(runtime);
        !wanted.is_empty() && wanted.iter().all(|id| asset(id).is_some_and(|asset| self.downloader.is_installed(asset)))
    }

    /// The running server of `runtime`'s build carrying CrisperWhisper: the one there is, or a new one. An error, before
    /// the program starts, when the processor lacks the server's floor or a part of it is missing.
    fn crisper_server(&self, runtime: OnnxFlavour) -> Result<std::sync::Arc<acpp_server::Server>> {
        let build = crisper_build(runtime)?;
        dub_tcpp::cpu_floor("audio.cpp").map_err(|error| anyhow!(error))?;
        let folder = self.downloader.runtime_dir(build.folder());
        let exe = self.crisper_server_exe(runtime).ok_or_else(|| anyhow!("the audio.cpp server ({}) is not downloaded: download CrisperWhisper in Settings - Models", build.folder()))?;
        let model = self.downloader.root().join(CRISPERWHISPER_DIR);
        if !crisper_assets(runtime).iter().all(|id| asset(id).is_some_and(|asset| self.downloader.is_installed(asset))) {
            bail!("CrisperWhisper is not completely downloaded into {}", model.display());
        }
        if build == acpp_server::asset::Build::Cuda13 {
            self.link_cuda13(&folder, "the audio.cpp server")?;
        }
        // Windows looks beside the program, then on PATH, where the engine's folder and its cuBLAS 13 are
        let path: Vec<PathBuf> = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect()).unwrap_or_default();
        let searched: Vec<&Path> = std::iter::once(folder.as_path()).chain(path.iter().map(PathBuf::as_path)).collect();
        let missing = acpp_server::asset::missing_libraries(build, &searched);
        if !missing.is_empty() {
            bail!("the audio.cpp server misses {} beside it in {}", missing.join(", "), folder.display());
        }
        let config = acpp_server::ServerConfig {
            exe,
            backend: build.backend(),
            device: 0,
            threads: std::thread::available_parallelism().map(|threads| threads.get() as u32).unwrap_or(8).min(16),
            models: vec![acpp_server::ModelEntry::crisperwhisper(CRISPERWHISPER, &model)],
            data_dir: self.downloader.root().join("logs").join(build.folder()),
            log: false,
        };
        // a Linux loader finds the engine's cuBLAS 13 for the CUDA build where the engine keeps it
        let libraries = [folder.clone(), crate::engine_bundle_root()];
        let prepare = move |command: &mut std::process::Command| {
            // the engine's folder lies inside the image too: it goes on after the image's folders are taken off
            dub_core::proc::outside_appimage(command);
            if cfg!(target_os = "linux") {
                let inherited = match command.get_envs().find(|(key, _)| *key == "LD_LIBRARY_PATH") {
                    Some((_, value)) => value.map(std::ffi::OsStr::to_os_string),
                    None => std::env::var_os("LD_LIBRARY_PATH"),
                };
                if let Ok(path) = std::env::join_paths(libraries.iter().cloned().chain(inherited.iter().flat_map(std::env::split_paths))) {
                    command.env("LD_LIBRARY_PATH", path);
                }
            }
        };
        let server = acpp_server::SERVERS.get(&config, std::time::Duration::from_secs(60), &prepare).map_err(|error| anyhow!("the audio.cpp server did not start: {error}"))?;
        studio_log!("audiocpp_server {} ({}) carries CrisperWhisper", server.pid(), build.folder());
        Ok(server)
    }

    pub fn installed_models(&self) -> Vec<&'static Asset> {
        ASSETS
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
        let engine = dub_tcpp::engine_dir(&root, build);
        if build == dub_tcpp::Build::Cuda {
            self.link_cuda13(&engine, "transcribe.cpp")?;
        }
        let asr = dub_tcpp::shared(&engine, &dub_tcpp::model_path(&root, model)).map_err(|error| anyhow!("load Parakeet: {error}"))?;
        Ok((asr, model))
    }

    /// `ort` loads this at run time; linking it would tie the build to one
    /// toolchain and one machine's libraries.
    pub fn onnxruntime_library(&self) -> Option<PathBuf> {
        self.machine_runtime().map(|(library, _)| library)
    }

    /// The ONNX Runtime this process loads, and the card it reaches. A process
    /// binds one build on first use, so it is chosen by the machine, not by a
    /// setting: the build for this machine's card when every file of it is
    /// there - it carries the processor provider too - else the processor build.
    /// Only a build at today's pin counts: an earlier one left on disk is not loaded.
    pub fn machine_runtime(&self) -> Option<(PathBuf, Option<OnnxCard>)> {
        let cuda = self.downloader.runtime_dir("onnx-cuda").join(libraries::ORT);
        let directml = self.downloader.runtime_dir("onnx-dml").join(libraries::ORT);
        let cpu = self.downloader.runtime_dir("onnx").join(libraries::ORT);
        match OnnxFlavour::Auto.card() {
            Some(OnnxCard::Cuda) if self.has_cuda_libraries() => return Some((cuda, Some(OnnxCard::Cuda))),
            Some(OnnxCard::DirectMl) if self.has_directml_libraries() => return Some((directml, Some(OnnxCard::DirectMl))),
            _ => {}
        }
        [(cpu, "onnxruntime"), (cuda, "onnxruntime-cuda"), (directml, "onnxruntime-directml")]
            .into_iter()
            .find(|(library, id)| library.is_file() && self.pinned(&[id]))
            .map(|(library, _)| (library, None))
    }

    /// Whether every one of these downloads is installed at today's pin.
    fn pinned(&self, ids: &[&str]) -> bool {
        ids.iter().all(|id| asset(id).is_some_and(|asset| self.downloader.is_installed(asset)))
    }

    /// The DirectML build and the DirectML it is built for, side by side, at today's pins.
    pub fn has_directml_libraries(&self) -> bool {
        let dir = self.downloader.runtime_dir("onnx-dml");
        [libraries::ORT, libraries::ORT_SHARED, "DirectML.dll"].iter().all(|name| dir.join(name).is_file()) && self.pinned(&DIRECTML_ASSETS)
    }

    /// Whether every library the card's provider needs is installed.
    pub fn has_card_libraries(&self, card: OnnxCard) -> bool {
        match card {
            OnnxCard::Cuda => self.has_cuda_libraries(),
            OnnxCard::DirectMl => self.has_directml_libraries(),
        }
    }

    /// Binds `ort` to this machine's runtime - once per process, before
    /// anything touches `ort`, or it binds to whatever `onnxruntime.dll` the
    /// system happens to have - and says which card the bound build reaches.
    /// Every piece of work on ONNX asks here first, so all of it runs on the
    /// one build that was bound. The library is loaded here, not on first use:
    /// one this process cannot load (another system's build, a signature macOS
    /// refuses) is then an error of the work that needed it, where `ort` would
    /// panic and stay unusable until the studio restarts.
    pub fn bind_ort(&self) -> Result<Option<OnnxCard>> {
        static BOUND: std::sync::OnceLock<Option<OnnxCard>> = std::sync::OnceLock::new();
        if let Some(card) = BOUND.get() {
            return Ok(*card);
        }
        let (library, card) = self.machine_runtime().ok_or_else(|| anyhow!("the ONNX Runtime library is not installed"))?;
        point_ort_at(&library, card);
        // Microsoft's builds send usage telemetry unless the environment turns it off
        let configured = ort::init_from(&library).map_err(|error| anyhow!("load ONNX Runtime {}: {error}", library.display()))?.with_telemetry(false).commit();
        if !configured {
            bail!("ONNX Runtime was set up before its library was bound, so its telemetry could not be turned off");
        }
        Ok(*BOUND.get_or_init(|| card))
    }

    /// The card work set to `runtime` runs on, with the machine's runtime
    /// bound first; none is the processor.
    pub fn onnx_card(&self, runtime: OnnxFlavour) -> Result<Option<OnnxCard>> {
        let bound = self.bind_ort()?;
        Ok(if matches!(runtime, OnnxFlavour::Cpu) { None } else { bound })
    }

    /// The CUDA provider is a separate library, and it in turn needs cuBLAS and
    /// cuDNN beside it. Without all of them the provider refuses to load and the
    /// run silently lands on the processor. All of them at today's pins.
    pub fn has_cuda_libraries(&self) -> bool {
        let dir = self.downloader.runtime_dir("onnx-cuda");
        libraries::CUDA.iter().all(|name| dir.join(name).is_file()) && self.pinned(CUDA_ASSETS)
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
            assets: self.downloader.status_of(ASSETS),
            active_download: self.downloader.active_for("karaoke").await,
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
                let server = match self.crisper_server(config.runtime) {
                    Ok(server) => server,
                    Err(error) => {
                        failed(heard, error);
                        return Recognised::OnDevice;
                    }
                };
                for (index, path) in audio.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    heard(index, self.crisper_transcribe(&server, path, language));
                }
                // the card is the music engine's between recognitions
                unload_crisper();
                Recognised::OnDevice
            }
            _ => {
                failed(heard, anyhow!("this recogniser does not run on this computer"));
                Recognised::OnDevice
            }
        }
    }

    /// The words CrisperWhisper hears in one track, each with the second it starts, on `runtime`'s build of the
    /// audio.cpp server; the model leaves the card afterwards.
    pub fn crisper_words(&self, runtime: OnnxFlavour, audio: &Path, language: Option<&str>) -> Result<Vec<(f64, String)>> {
        let server = self.crisper_server(runtime)?;
        let words = self.crisper_transcribe(&server, audio, language);
        unload_crisper();
        words
    }

    /// One track through the running server: decoded to the 16 kHz mono WAV the server reads, told its language when
    /// it is known ("auto" or none lets the model hear it), and what a hallucinated segment holds left out.
    fn crisper_transcribe(&self, server: &acpp_server::Server, audio: &Path, language: Option<&str>) -> Result<Vec<(f64, String)>> {
        let work = self.downloader.root().join("work");
        fs::create_dir_all(&work).with_context(|| format!("create {}", work.display()))?;
        let wav = work.join(format!("sync-{}.wav", uuid::Uuid::now_v7()));
        crate::audio_pcm::write_wav16k_mono(audio, &wav).with_context(|| format!("decode {} for recognition", audio.display()))?;
        let told = match language.map(str::trim).filter(|code| !code.is_empty() && *code != "auto") {
            Some(code) => Some(acpp_server::language::whisper_code(code).map_err(|error| anyhow!("{error}"))?.to_string()),
            None => None,
        };
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

    // best[p]: the best (total, sum of starts) so far with the next line free
    // to start at word p or later. Each line is either placed at some start
    // >= p, which moves p past it, or left out. A clearly recognised line
    // counts the same wherever it is heard best, and between equal totals the
    // earlier placement wins: a chorus the model sang twice in a row shows its
    // lines as they are first sung, not where the recogniser heard them best.
    let credit = |score: f64| if score >= 0.7 { 1.0 } else { score };
    let better = |left: (f64, usize), right: (f64, usize)| left.0 > right.0 + 1e-9 || ((left.0 - right.0).abs() <= 1e-9 && left.1 < right.1);
    let mut best: Vec<Option<(f64, usize)>> = vec![None; count + 1];
    best[0] = Some((0.0, 0));
    let mut back: Vec<Vec<(usize, Option<usize>)>> = Vec::with_capacity(lines.len());
    for (index, &span) in spans.iter().enumerate() {
        let mut next: Vec<Option<(f64, usize)>> = vec![None; count + 1];
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
                let total = (so_far.0 + credit(score), so_far.1 + start);
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

    // A clearly recognised line was credited in full, so its start may sit a
    // word early; settle it where it matches best, between its neighbours.
    for index in 0..lines.len() {
        let Some(start) = starts[index] else { continue };
        let floor = starts[..index].iter().rev().flatten().next().map(|previous| previous + 1).unwrap_or(0);
        let ceiling = starts[index + 1..].iter().flatten().next().copied().unwrap_or(count);
        let low = start.saturating_sub(2).max(floor);
        let high = (start + 2).min(ceiling.saturating_sub(1)).min(count.saturating_sub(1));
        let settled = (low..=high).fold(start, |kept, candidate| if scores[index][candidate] > scores[index][kept] { candidate } else { kept });
        starts[index] = Some(settled);
    }
    let mut placed: Vec<Option<f64>> = starts.iter().map(|start| start.map(|start| heard[start].0)).collect();

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
        for asset in ASSETS.iter().filter(|asset| asset.url.contains("huggingface.co/")) {
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
    }

    // The releases every runtime download is pinned to.
    const AUDIOCPP_BUILD: &str = "/audio.cpp/releases/download/server-";
    const SERVER_CUFFT_BUILD: &str = "12.4.0.43";
    const CUBLAS_BUILD: &str = "12.9.2.10";
    const CUDART_BUILD: &str = "12.9.79";
    const CUFFT_BUILD: &str = "11.4.1.4";
    const CUDNN_BUILD: &str = "9.25.0.15";
    const CURAND_BUILD: &str = "10.3.10.19";
    const ONNXRUNTIME_BUILD: &str = "v1.30.0";
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
    fn every_runtime_is_pinned_and_every_asset_is_distinct() {
        for entry in ASSETS {
            assert!(entry.bytes > 0, "{} has no size", entry.id);
            assert!(entry.url.starts_with("https://"), "{} is not fetched over https", entry.id);
            assert_eq!(ASSETS.iter().filter(|other| other.id == entry.id).count(), 1);
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
}

#[cfg(test)]
mod live_recognition {
    use super::*;

    #[test]
    fn the_leftovers_wait_for_the_update_screen_and_go_by_name() {
        let root = tempfile::tempdir().unwrap();
        let sync = LyricsSync::new(root.path());
        let karaoke = root.path().join("karaoke");
        for folder in RETIRED {
            fs::create_dir_all(karaoke.join(folder)).unwrap();
            fs::write(karaoke.join(folder).join("model.bin"), b"x").unwrap();
        }
        let whisper = karaoke.join("models/whisper/faster-whisper-small/model.bin");
        let kept = [karaoke.join("models/asr-gguf-parakeet-v3/parakeet-tdt-0.6b-v3-Q8_0.gguf"), karaoke.join("models/crisperwhisper2-turbo/model.safetensors"), whisper.clone()];
        for file in &kept {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"x").unwrap();
        }
        let found: Vec<(&str, usize)> = sync.leftovers().iter().map(|leftover| (leftover.what, leftover.paths.len())).collect();
        assert_eq!(found, vec![("parakeet-onnx", RETIRED.len()), ("faster-whisper", 1)]);
        sync.remove_leftovers(&["parakeet-onnx"]);
        assert!(RETIRED.iter().all(|retired| !karaoke.join(retired).exists()));
        assert!(kept.iter().all(|file| file.is_file()), "Faster-Whisper waits for its own turn");
        sync.remove_leftovers(&["parakeet-onnx", "faster-whisper"]);
        assert!(!karaoke.join("models/whisper").exists());
        assert!(kept[..2].iter().all(|file| file.is_file()));
        assert!(sync.leftovers().iter().all(|leftover| leftover.paths.is_empty()));
    }

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
            let asset = asset(dub_tcpp::Build::Cpu.dir()).unwrap();
            assert_eq!(Path::new("karaoke-root").join(asset.relative_path), dub_tcpp::engine_dir(&root, dub_tcpp::Build::Cpu).join(dub_tcpp::LIBRARY));
        }
        #[cfg(any(windows, target_os = "linux"))]
        for build in dub_tcpp::Build::ALL {
            let asset = asset(build.dir()).unwrap();
            assert_eq!(Path::new("karaoke-root").join(asset.relative_path), dub_tcpp::engine_dir(&root, build).join(dub_tcpp::LIBRARY));
            assert_eq!((asset.url, asset.bytes), (build.download().url, build.download().size));
        }
    }

    /// The CUDA build of ONNX Runtime comes with its symbols, headers and import libraries, a gigabyte the studio
    /// never reads: only the libraries it loads are taken out of the archive.
    #[test]
    #[cfg(windows)]
    fn only_the_libraries_of_onnx_runtime_cuda_are_taken() {
        let runtime = asset("onnxruntime-cuda").unwrap();
        for library in [libraries::ORT, libraries::ORT_SHARED, libraries::CUDA[0]] {
            assert!(runtime.pick.contains(&library), "{library} is not taken");
        }
        assert!(runtime.pick.iter().all(|name| name.ends_with(".dll")), "{:?}", runtime.pick);
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
        assert!(sync.crisper_ready(runtime), "CrisperWhisper is not installed for {:?}", crisper_build(runtime));
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
mod directml_live {
    use super::*;
    use std::time::Instant;

    /// Where the DirectML.dll this process loaded came from.
    fn loaded_directml() -> Option<PathBuf> {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
            fn GetModuleFileNameW(module: *mut std::ffi::c_void, file: *mut u16, size: u32) -> u32;
        }
        let name: Vec<u16> = "DirectML.dll".encode_utf16().chain(Some(0)).collect();
        let module = unsafe { GetModuleHandleW(name.as_ptr()) };
        if module.is_null() {
            return None;
        }
        let mut path = vec![0u16; 1024];
        let length = unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) } as usize;
        Some(PathBuf::from(String::from_utf16_lossy(&path[..length])))
    }

    fn env_path(name: &str) -> PathBuf {
        PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")))
    }

    /// The DirectML build installed the way the studio installs it and bound,
    /// once for the process. `STUDIO_TEST_DML_ADAPTER` pins a DXGI adapter -
    /// the integrated Radeon beside an NVIDIA card - instead of the fastest card.
    fn directml() -> Option<OnnxCard> {
        static BOUND: std::sync::Once = std::sync::Once::new();
        BOUND.call_once(|| {
            let sync = LyricsSync::new(&env_path("STUDIO_TEST_DATA_ROOT"));
            let parts: Vec<&'static Asset> = DIRECTML_ASSETS.iter().filter_map(|id| asset(id)).collect();
            tokio::runtime::Runtime::new().unwrap().block_on(sync.downloader().install_all("test", &parts)).expect("install DirectML");
            assert!(sync.has_directml_libraries(), "the DirectML build is incomplete after installing it");
            if let Ok(adapter) = std::env::var("STUDIO_TEST_DML_ADAPTER") {
                DIRECTML_ADAPTER.set(adapter.parse().expect("a DXGI adapter index")).unwrap();
            }
            let runtime = sync.downloader().runtime_dir("onnx-dml");
            point_ort_at(&runtime.join("onnxruntime.dll"), Some(OnnxCard::DirectMl));
            assert_eq!(loaded_directml().as_deref(), Some(runtime.join("DirectML.dll").as_path()), "another DirectML.dll serves the process");
        });
        Some(OnnxCard::DirectMl)
    }

    /// Beat This! and S-KEY on the card beside the processor: the tempo and
    /// the key each hears.
    #[test]
    #[ignore = "downloads DirectML; needs STUDIO_TEST_DATA_ROOT, STUDIO_TEST_TRACK and STUDIO_TEST_AUDIO_FACTS"]
    fn tempo_and_key_run_through_directml() {
        let card = directml();
        let mono = crate::audio_facts::decode(&env_path("STUDIO_TEST_TRACK")).expect("decode the track");
        let models = env_path("STUDIO_TEST_AUDIO_FACTS");
        let measure = |card| {
            let started = Instant::now();
            let mut measurer = crate::audio_facts::Measurer::load(&models, card).expect("load the models");
            let facts = measurer.measure_mono(&mono).expect("measure");
            (facts, measurer.on_gpu, started.elapsed().as_secs_f64())
        };
        let (on_card, on_gpu, card_time) = measure(card);
        assert!(on_gpu, "DirectML did not take both models");
        let (on_processor, _, processor_time) = measure(None);
        studio_log!("card {on_card:?} in {card_time:.1} s, processor {on_processor:?} in {processor_time:.1} s");
        assert_eq!(on_card, on_processor, "the card hears another tempo or key");
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
