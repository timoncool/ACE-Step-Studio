//! Splitting a finished track into stems: HT-Demucs 6s on audio.cpp's server
//! (six stems of 44.1 kHz stereo, the server cuts and joins the segments
//! itself), or Mel-Band RoFormer through Dub Studio's separator for the vocals
//! and the rest.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{bail, Context, Result};

use serde::{Deserialize, Serialize};

use crate::downloads::{Asset, AssetKind, Downloader};

/// What a separation run should produce, and how carefully.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SeparationConfig {
    /// Graphics card or processor. "auto" takes the card when its runtime is
    /// installed, which is the difference between a minute and ten.
    pub runtime: crate::lyrics_sync::OnnxFlavour,
    /// Which stems to keep. Every one still costs the same run - the model
    /// returns all six - but writing only what the user wants keeps the
    /// library from filling with silence they never asked for.
    pub stems: Vec<String>,
    /// Which separator runs: HT-Demucs for six stems, Mel-Band RoFormer for the
    /// vocals and the rest. It also separates the vocals the vocals-only mode
    /// and training take.
    pub engine: Engine,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    #[default]
    Htdemucs,
    Roformer,
}

impl Default for SeparationConfig {
    fn default() -> Self {
        Self {
            // the card when its runtime is installed, the processor otherwise:
            // what the panel shows before anything is chosen
            runtime: crate::lyrics_sync::OnnxFlavour::Auto,
            stems: STEMS.iter().map(|stem| (*stem).to_string()).collect(),
            engine: Engine::Htdemucs,
        }
    }
}

impl SeparationConfig {
}



/// The separation model, and where it comes from.
///
/// One file, one download, pinned to the revision it was measured at: audio.cpp's own GGUF of HT-Demucs 6s, f16 as
/// it is published. Like everything else here it is fetched only when the user asks for it.
pub const MODEL: Asset = Asset {
    id: "htdemucs-6s",
    label: "HT-Demucs 6 stems",
    kind: AssetKind::Model,
    url: "https://huggingface.co/audio-cpp/audio.cpp-gguf/resolve/462b4cec041e06ffb3d8fcf0d8c655051b1cd9f5/HTDemucs-6stems-GGUF/htdemucs-6stems-f16.gguf",
    relative_path: "models/htdemucs/htdemucs-6stems-f16.gguf",
    bytes: 54_919_200,
    unzip_into: None,
    marker: "",
    pick: &[],
    vram_gb: Some(2),
    note: "Six stems: drums, bass, other, vocals, guitar, piano. MIT-licensed, runs on audio.cpp's server.",
};

/// The stems Mel-Band RoFormer leaves: the voice, and the mix without it.
pub const ROFORMER_STEMS: [&str; 2] = ["vocals", "instrumental"];

/// Mel-Band RoFormer voc_fv6 (GaboxR67) in GGUF Q8_0, as Dub Studio separates with it.
pub const ROFORMER_MODEL: Asset = Asset {
    id: "roformer",
    label: "Mel-Band RoFormer voc_fv6 (Q8_0)",
    kind: AssetKind::Model,
    url: "https://huggingface.co/chenmozhijin/BSRoformer-GGUF/resolve/df802a6773d25ba6ef785ff619daa3e510503168/GaboxR67/MelBandRoformers/melbandroformers/vocals/voc_fv6-Q8_0.gguf",
    relative_path: "models/bsroformer/voc_fv6-Q8_0.gguf",
    bytes: 251_707_744,
    unzip_into: None,
    marker: "",
    pick: &[],
    vram_gb: Some(2),
    note: "The vocals and the rest, cleaner than HT-Demucs: Dub Studio's separator, run by BSRoformer.cpp.",
};

/// The builds of BSRoformer.cpp, as Dub Studio names them.
pub use dub_sep::asset::Build as RoformerBuild;

/// BSRoformer.cpp as Dub Studio pins it (`dub_sep::asset`): timoncool studio-0.1.1 on Windows and Linux - CUDA 13,
/// CUDA 12.9, Vulkan and the processor, each with a ggml-cpu per instruction level so any x86-64 runs it - and the
/// author's Apple silicon build. The component ids and folders are the other programs' too. None where the platform
/// has no such build.
pub fn roformer_asset(build: RoformerBuild) -> Option<&'static Asset> {
    static ASSETS: OnceLock<Vec<(RoformerBuild, Asset)>> = OnceLock::new();
    ASSETS
        .get_or_init(|| RoformerBuild::ALL.into_iter().filter_map(|build| build.engine().map(|pinned| (build, roformer_download(build, pinned)))).collect())
        .iter()
        .find(|(of, _)| *of == build)
        .map(|(_, asset)| asset)
}

fn roformer_download(build: RoformerBuild, pinned: dub_sep::asset::Asset) -> Asset {
    let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
    let folder = pinned.archive.rsplit('/').nth(1).unwrap_or("bsroformer");
    let extension = pinned.archive.rsplit_once("_engine.").map_or("zip", |(_, extension)| extension);
    // the CUDA 13 build's archive keeps the name it had before the other builds came
    let archive = if build == RoformerBuild::Cuda13 { "bsroformer-cuda" } else { folder };
    let (label, note) = match build {
        RoformerBuild::Cuda13 => ("BSRoformer.cpp (CUDA)", "Runs Mel-Band RoFormer on an NVIDIA card, with the engine's CUDA 13 libraries."),
        RoformerBuild::Cuda12 => ("BSRoformer.cpp (CUDA 12)", "Runs Mel-Band RoFormer on an NVIDIA card CUDA 13 does not run - Maxwell to Volta, or a driver older than 580 - with the engine's CUDA 12 libraries."),
        RoformerBuild::Vulkan => ("BSRoformer.cpp (Vulkan)", "Runs Mel-Band RoFormer on an AMD or Intel card."),
        RoformerBuild::Cpu if cfg!(target_os = "macos") => ("BSRoformer.cpp (Metal)", "Runs Mel-Band RoFormer on Apple silicon: Metal, and the processor where Metal cannot."),
        RoformerBuild::Cpu => ("BSRoformer.cpp (processor)", "Runs Mel-Band RoFormer on the processor: slower, the same result."),
    };
    Asset {
        id: build.component(),
        label,
        kind: AssetKind::Runtime,
        url: pinned.url,
        relative_path: leak(format!("{archive}.{extension}")),
        bytes: pinned.size,
        unzip_into: Some(leak(folder.to_string())),
        marker: "bs_roformer-cli",
        pick: &[],
        vram_gb: None,
        note,
    }
}

/// The build RoFormer runs, as Dub Studio chooses it: on the card (`gpu`) the CUDA build its card and driver run (13
/// or 12), Vulkan on a card of another make (`other_make`), the processor on an NVIDIA card no CUDA build runs; the
/// processor otherwise. A Mac has its one build.
pub fn roformer_build(gpu: bool, card: Option<crate::cuda_build::CudaBuild>, other_make: bool) -> RoformerBuild {
    let card = card.map(|build| match build {
        crate::cuda_build::CudaBuild::Cuda13 => RoformerBuild::Cuda13,
        crate::cuda_build::CudaBuild::Cuda12 => RoformerBuild::Cuda12,
    });
    RoformerBuild::for_device(if gpu { "auto" } else { "cpu" }, card, other_make).unwrap_or(RoformerBuild::Cpu)
}

/// The build this machine runs RoFormer on, `gpu` or not.
pub fn machine_roformer_build(gpu: bool) -> RoformerBuild {
    roformer_build(gpu, crate::cuda_build::card(), crate::presets::display_card().is_some() && !crate::cuda_build::nvidia_card())
}

/// The libraries a Windows CUDA build loads from beside it: cuBLAS of its CUDA, and for CUDA 12 its runtime, which
/// CUDA 13's links in. None for the other builds, and off Windows, where they come from `LD_LIBRARY_PATH`.
pub fn roformer_cuda_libraries(build: RoformerBuild) -> &'static [&'static str] {
    const CUDA12: [&str; 3] = [crate::engine_runtime::CUDA12_LIBRARIES[0], crate::engine_runtime::CUDA12_LIBRARIES[1], "cudart64_12.dll"];
    match build {
        _ if !cfg!(windows) => &[],
        RoformerBuild::Cuda13 => &crate::engine_runtime::CUDA13_LIBRARIES,
        RoformerBuild::Cuda12 => &CUDA12,
        RoformerBuild::Vulkan | RoformerBuild::Cpu => &[],
    }
}

/// Owns the separation model's directory and the runs that use it.
pub struct Separator {
    downloader: Downloader,
}

impl Separator {
    pub fn new(data_root: &Path) -> Self {
        Self { downloader: Downloader::new(data_root.join("separation")) }
    }

    pub fn downloader(&self) -> &Downloader {
        &self.downloader
    }

    pub fn model_path(&self) -> PathBuf {
        self.downloader.path_of(&MODEL)
    }

    pub fn is_installed(&self) -> bool {
        self.downloader.is_installed(&MODEL)
    }

    /// Apollo's weights, kept with the separator's.
    pub fn restore_model_path(&self) -> PathBuf {
        self.downloader.path_of(&crate::restore::MODEL)
    }

    pub fn restore_installed(&self) -> bool {
        self.downloader.is_installed(&crate::restore::MODEL)
    }

    /// The BSRoformer.cpp build for the card or the processor on this machine, where this platform has one.
    pub fn roformer_engine(gpu: bool) -> Option<&'static Asset> {
        roformer_asset(machine_roformer_build(gpu))
    }

    pub fn roformer_cli(&self, gpu: bool) -> Option<PathBuf> {
        let engine = Self::roformer_engine(gpu)?;
        let name = if cfg!(windows) { "bs_roformer-cli.exe" } else { "bs_roformer-cli" };
        Some(self.downloader.runtime_dir(engine.unzip_into?).join(name))
    }

    /// Mel-Band RoFormer can run: its model, the engine build for the card or the processor, and for a CUDA build the
    /// libraries that build loads.
    pub fn roformer_installed(&self, gpu: bool) -> bool {
        self.downloader.is_installed(&ROFORMER_MODEL)
            && Self::roformer_engine(gpu).is_some_and(|engine| self.downloader.is_installed(engine))
            && (!gpu || self.cuda_libraries_found())
    }

    /// The CUDA build's libraries where the Windows loader looks: beside the program, or on PATH.
    fn cuda_libraries_found(&self) -> bool {
        let libraries = roformer_cuda_libraries(machine_roformer_build(true));
        if libraries.is_empty() {
            return true;
        }
        let Some(folder) = self.roformer_cli(true).and_then(|cli| cli.parent().map(Path::to_path_buf)) else { return false };
        let on_path = |name: &str| std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()));
        libraries.iter().all(|name| folder.join(name).is_file() || on_path(name))
    }

    /// The CUDA build imports the CUDA libraries the music engine already has (cuBLAS beside it or in the data folder,
    /// CUDA 12's runtime beside it); Windows looks for a library in the program's folder, so it gets links to those
    /// files from the first of `from` that has each, no copies.
    pub fn link_cuda_libraries(&self, from: &[PathBuf]) -> Result<()> {
        let Some(cli) = self.roformer_cli(true) else { return Ok(()) };
        let Some(folder) = cli.parent() else { return Ok(()) };
        for name in roformer_cuda_libraries(machine_roformer_build(true)) {
            let target = folder.join(name);
            let Some(source) = from.iter().map(|directory| directory.join(name)).find(|source| source.is_file()) else { continue };
            if target.is_file() {
                continue;
            }
            if std::fs::hard_link(&source, &target).is_err() {
                std::fs::copy(&source, &target).with_context(|| format!("copy {} beside BSRoformer.cpp", source.display()))?;
            }
        }
        Ok(())
    }

    /// The vocals and the rest of interleaved 44.1 kHz stereo, through Dub Studio's separator.
    pub fn separate_roformer(&self, samples: &[f32], gpu: bool) -> Result<(Vec<f32>, Vec<f32>)> {
        let cli = self.roformer_cli(gpu).context("Mel-Band RoFormer has no build for this platform")?;
        let work = tempfile::tempdir()?;
        let mix = work.path().join("mix.wav");
        let stereo = audio_post::Stereo {
            rate: SAMPLE_RATE,
            left: samples.iter().step_by(CHANNELS).copied().collect(),
            right: samples.iter().skip(1).step_by(CHANNELS).copied().collect(),
        };
        crate::audio_pcm::write_wav_f32(&mix, &stereo)?;
        let separated = dub_sep::separate(&mix, &work.path().join("stems"), &cli, &self.downloader.path_of(&ROFORMER_MODEL))
            .map_err(|error| anyhow::anyhow!("Mel-Band RoFormer: {error}"))?;
        Ok((crate::audio_pcm::decode_stereo_44k(&separated.vocals)?, crate::audio_pcm::decode_stereo_44k(&separated.instrumental)?))
    }
}

pub const SAMPLE_RATE: u32 = 44_100;
const CHANNELS: usize = 2;

/// The stems the six-source model returns, in the order it returns them.
pub const STEMS: [&str; 6] = ["drums", "bass", "other", "vocals", "guitar", "piano"];

/// One separated track: a name and interleaved stereo samples.
pub struct Stem {
    pub name: &'static str,
    pub samples: Vec<f32>,
}

/// What a finished run has to say about itself.
pub struct Separated {
    pub stems: Vec<Stem>,
    /// True when the graphics card actually did the work.
    pub used_gpu: bool,
}

/// The six stems of interleaved 44.1 kHz stereo, through audio.cpp's HT-Demucs on `build`. `stop` is asked while it
/// runs; a stop ends the server and the call says "stopped".
pub fn separate_htdemucs(
    acpp: &crate::acpp::Acpp,
    model: &Path,
    audio: &[f32],
    build: acpp_server::asset::Build,
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<Separated> {
    if audio.is_empty() {
        bail!("nothing to separate: the track decoded to no audio");
    }
    let work = tempfile::tempdir()?;
    let mix = work.path().join("mix.wav");
    write_wav_stereo(&mix, audio)?;
    let entry = acpp_server::ModelEntry::htdemucs(crate::acpp::HTDEMUCS, model);
    let result = crate::acpp::run(acpp, build, entry, vec![model.to_path_buf()], serde_json::json!({ "audio": mix }), stop)?;
    let outputs = acpp_server::task_audio(&result).map_err(|error| anyhow::anyhow!("HT-Demucs: {}", crate::acpp::describe(&error)))?;
    let mut stems = Vec::new();
    for name in STEMS {
        let output = outputs.iter().find(|output| output.id.as_deref() == Some(name)).with_context(|| format!("HT-Demucs returned no {name}"))?;
        if output.sample_rate != SAMPLE_RATE || usize::from(output.channels) != CHANNELS {
            bail!("HT-Demucs returned {name} at {} Hz in {} channels, not {SAMPLE_RATE} Hz stereo", output.sample_rate, output.channels);
        }
        stems.push(Stem { name, samples: output.samples.clone() });
    }
    Ok(Separated { stems, used_gpu: crate::acpp::on_card(build) })
}

/// Writes interleaved stereo samples as a 16-bit WAV, which every player and
/// every editor reads without being asked twice.
pub fn write_wav_stereo(path: &Path, samples: &[f32]) -> Result<()> {
    use std::io::Write;
    let frames = samples.len() / CHANNELS;
    let data_bytes = frames * CHANNELS * 2;
    let mut file = std::io::BufWriter::new(std::fs::File::create(path).with_context(|| format!("create {}", path.display()))?);

    file.write_all(b"RIFF")?;
    file.write_all(&((36 + data_bytes) as u32).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&(CHANNELS as u16).to_le_bytes())?;
    file.write_all(&SAMPLE_RATE.to_le_bytes())?;
    file.write_all(&(SAMPLE_RATE * CHANNELS as u32 * 2).to_le_bytes())?;
    file.write_all(&((CHANNELS * 2) as u16).to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&(data_bytes as u32).to_le_bytes())?;
    for sample in samples {
        let clamped = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        file.write_all(&clamped.to_le_bytes())?;
    }
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_carries_the_header_a_player_expects() {
        let path = std::env::temp_dir().join(format!("studio-stem-{}.wav", uuid::Uuid::now_v7()));
        write_wav_stereo(&path, &[0.0, 0.5, -0.5, 1.0]).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2);
        assert_eq!(u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]), SAMPLE_RATE);
        // Two frames of stereo, sixteen bits each.
        assert_eq!(bytes.len(), 44 + 8);
        let _ = std::fs::remove_file(path);
    }


    /// RoFormer takes the build of the card it runs on: the CUDA build of the card and driver, Vulkan on a card of
    /// another make, the processor without one or when asked.
    #[test]
    #[cfg(not(target_os = "macos"))]
    fn roformer_takes_the_build_of_the_card() {
        use crate::cuda_build::CudaBuild;
        assert_eq!(roformer_build(true, Some(CudaBuild::Cuda13), false), RoformerBuild::Cuda13);
        assert_eq!(roformer_build(true, Some(CudaBuild::Cuda12), false), RoformerBuild::Cuda12);
        assert_eq!(roformer_build(true, None, true), RoformerBuild::Vulkan, "an AMD or Intel card");
        assert_eq!(roformer_build(true, None, false), RoformerBuild::Cpu, "no card, or an NVIDIA card no CUDA build runs");
        assert_eq!(roformer_build(false, Some(CudaBuild::Cuda13), false), RoformerBuild::Cpu);
        for (build, folder) in [(RoformerBuild::Cuda13, "bsroformer"), (RoformerBuild::Cuda12, "bsroformer-cuda12"), (RoformerBuild::Vulkan, "bsroformer-vulkan"), (RoformerBuild::Cpu, "bsroformer-cpu")] {
            let asset = roformer_asset(build).expect("every build on Windows and Linux");
            assert_eq!((asset.id, asset.unzip_into), (build.component(), Some(folder)));
            assert!(crate::pin_sha::sha256(asset.url).is_some(), "{} has its pin's SHA-256", asset.url);
            assert!(asset.relative_path.ends_with(if cfg!(windows) { ".zip" } else { ".tar.gz" }), "{}", asset.relative_path);
        }
        assert_eq!(roformer_asset(RoformerBuild::Cuda13).unwrap().relative_path, if cfg!(windows) { "bsroformer-cuda.zip" } else { "bsroformer-cuda.tar.gz" });
        assert_eq!(roformer_cuda_libraries(RoformerBuild::Cuda12).len(), if cfg!(windows) { 3 } else { 0 });
    }
}
