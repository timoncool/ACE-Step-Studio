//! Splitting a finished track into stems.
//!
//! ACE-Step Studio did this in the browser: an ONNX build of htdemucs running
//! under WebAssembly on a page of its own. That is the arrangement this studio
//! replaced everywhere else, and it is replaced here too - the model runs in
//! this process, on the ONNX Runtime the studio already ships for the karaoke
//! recogniser, with no Python and no second window.
//!
//! The model takes 7.8 seconds of 44.1 kHz stereo at a time and returns the
//! same span once per stem. A song is longer than that, so it is cut into
//! overlapping segments and cross-faded back together: without the overlap the
//! seams are audible, which is the usual way a separator sounds broken.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use ort::session::Session;
use ort::value::Value;

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
    /// How much neighbouring segments share. More overlap means more passes
    /// and a smoother seam; a quarter is the model's own reference setting.
    pub overlap: f64,
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
            overlap: OVERLAP,
            engine: Engine::Htdemucs,
        }
    }
}

impl SeparationConfig {
    /// The overlap, kept inside what the segment length can actually support.
    pub fn sane_overlap(&self) -> f64 {
        self.overlap.clamp(0.0, 0.5)
    }

    pub fn wants(&self, stem: &str) -> bool {
        self.stems.iter().any(|wanted| wanted == stem)
    }
}



/// The separation model, and where it comes from.
///
/// One file, one download, pinned to the revision it was measured at. Like
/// everything else here it is fetched only when the user asks for it: a studio
/// that never separates a track never spends the 136 MB.
pub const MODEL: Asset = Asset {
    id: "htdemucs-6s",
    label: "HT-Demucs 6 stems",
    kind: AssetKind::Model,
    url: "https://huggingface.co/StemSplitio/htdemucs-6s-onnx/resolve/49df9b6989cf2150840ea65b0bef77a2e471b678/htdemucs_6s_fp16weights.onnx",
    relative_path: "models/htdemucs/htdemucs_6s_fp16.onnx",
    bytes: 136_428_532,
    unzip_into: None,
    marker: "",
    pick: &[],
    vram_gb: Some(2),
    note: "Six stems: drums, bass, other, vocals, guitar, piano. MIT-licensed, runs on the studio's ONNX Runtime.",
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

/// BSRoformer.cpp (chenmozhijin), the CUDA build and the processor build, as Dub Studio ships them.
#[cfg(windows)]
pub const ROFORMER_ENGINE: Asset = Asset {
    id: "bsroformer-engine",
    label: "BSRoformer.cpp (CUDA)",
    kind: AssetKind::Runtime,
    url: "https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-windows-cuda-13.1.0.zip",
    relative_path: "bsroformer-cuda.zip",
    bytes: 164_990_561,
    unzip_into: Some("bsroformer"),
    marker: "bs_roformer-cli",
    pick: &[],
    vram_gb: None,
    note: "Runs Mel-Band RoFormer on an NVIDIA card, with the engine's cuBLAS 13.",
};
#[cfg(windows)]
pub const ROFORMER_ENGINE_CPU: Asset = Asset {
    id: "bsroformer-engine-cpu",
    label: "BSRoformer.cpp (processor)",
    kind: AssetKind::Runtime,
    url: "https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-windows-x64-msvc.zip",
    relative_path: "bsroformer-cpu.zip",
    bytes: 671_031,
    unzip_into: Some("bsroformer-cpu"),
    marker: "bs_roformer-cli",
    pick: &[],
    vram_gb: None,
    note: "Runs Mel-Band RoFormer on the processor: slower, the same result.",
};
#[cfg(target_os = "linux")]
pub const ROFORMER_ENGINE: Asset = Asset {
    id: "bsroformer-engine",
    label: "BSRoformer.cpp (CUDA)",
    kind: AssetKind::Runtime,
    url: "https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-linux-cuda-13.1.0.tar.xz",
    relative_path: "bsroformer-cuda.tar.xz",
    bytes: 239_527_692,
    unzip_into: Some("bsroformer"),
    marker: "bs_roformer-cli",
    pick: &[],
    vram_gb: None,
    note: "Runs Mel-Band RoFormer on an NVIDIA card.",
};
#[cfg(target_os = "linux")]
pub const ROFORMER_ENGINE_CPU: Asset = Asset {
    id: "bsroformer-engine-cpu",
    label: "BSRoformer.cpp (processor)",
    kind: AssetKind::Runtime,
    url: "https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-linux-x64-cpu.tar.xz",
    relative_path: "bsroformer-cpu.tar.xz",
    bytes: 640_204,
    unzip_into: Some("bsroformer-cpu"),
    marker: "bs_roformer-cli",
    pick: &[],
    vram_gb: None,
    note: "Runs Mel-Band RoFormer on the processor: slower, the same result.",
};

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

    /// True when a run could start right now: the model is here and so is the
    /// runtime that executes it.
    pub fn ready(&self, runtime: Option<&Path>) -> bool {
        self.is_installed() && runtime.is_some()
    }


    /// The BSRoformer.cpp build for the card or the processor, where this platform has one.
    pub fn roformer_engine(gpu: bool) -> Option<&'static Asset> {
        #[cfg(any(windows, target_os = "linux"))]
        {
            Some(if gpu { &ROFORMER_ENGINE } else { &ROFORMER_ENGINE_CPU })
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = gpu;
            None
        }
    }

    pub fn roformer_cli(&self, gpu: bool) -> Option<PathBuf> {
        let engine = Self::roformer_engine(gpu)?;
        let name = if cfg!(windows) { "bs_roformer-cli.exe" } else { "bs_roformer-cli" };
        Some(self.downloader.runtime_dir(engine.unzip_into?).join(name))
    }

    /// Mel-Band RoFormer can run: its model, the engine build for the card or the processor, and for the card the
    /// cuBLAS that build loads.
    pub fn roformer_installed(&self, gpu: bool) -> bool {
        self.downloader.is_installed(&ROFORMER_MODEL)
            && Self::roformer_engine(gpu).is_some_and(|engine| self.downloader.is_installed(engine))
            && (!gpu || self.cuda_libraries_found())
    }

    /// The CUDA build's cuBLAS 13 where the Windows loader looks: beside the program, or on PATH.
    fn cuda_libraries_found(&self) -> bool {
        if !cfg!(windows) {
            return true;
        }
        let Some(folder) = self.roformer_cli(true).and_then(|cli| cli.parent().map(Path::to_path_buf)) else { return false };
        let on_path = |name: &str| std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()));
        crate::engine_runtime::CUDA13_LIBRARIES.iter().all(|name| folder.join(name).is_file() || on_path(name))
    }

    /// The CUDA build imports the cuBLAS 13 the music engine already has beside it; Windows
    /// looks for a library in the program's folder, so it gets links to those files, no copies.
    pub fn link_cuda_libraries(&self, from: &Path) -> Result<()> {
        let Some(cli) = self.roformer_cli(true) else { return Ok(()) };
        let Some(folder) = cli.parent() else { return Ok(()) };
        for name in crate::engine_runtime::CUDA13_LIBRARIES {
            let (source, target) = (from.join(name), folder.join(name));
            if target.is_file() || !source.is_file() {
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
        crate::cpu::tool_runs_here("Mel-Band RoFormer separator")?;
        let separated = dub_sep::separate(&mix, &work.path().join("stems"), &cli, &self.downloader.path_of(&ROFORMER_MODEL))
            .map_err(|error| anyhow::anyhow!("Mel-Band RoFormer: {error}"))?;
        Ok((crate::audio_pcm::decode_stereo_44k(&separated.vocals)?, crate::audio_pcm::decode_stereo_44k(&separated.instrumental)?))
    }
}

/// What the exported graph expects: 7.8 s of stereo at 44.1 kHz.
pub const SEGMENT_SAMPLES: usize = 343_980;
pub const SAMPLE_RATE: u32 = 44_100;
const CHANNELS: usize = 2;

/// How much of each segment is shared with the one before it. A quarter is what
/// the model's own reference inference uses, and it is enough for the crossfade
/// to hide the boundary.
const OVERLAP: f64 = 0.25;

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

/// The model on its runtime, loaded once and run over as many tracks as there
/// are: a dataset of twenty songs loads it once, not twenty times.
pub struct Loaded {
    session: Session,
    /// True when the graphics card actually does the work.
    pub used_gpu: bool,
}

/// Loads the model, on the card a path reaches when one is given.
pub fn load(model: &Path, card: Option<crate::lyrics_sync::OnnxCard>) -> Result<Loaded> {
    let builder = Session::builder().context("prepare an ONNX session")?;
    let (mut builder, used_gpu) = crate::lyrics_sync::with_card(builder, card);
    let session = builder
        .commit_from_file(model)
        .with_context(|| {
            if used_gpu {
                format!("the graphics card could not load the separation model {}; choose the processor in the separation settings to separate without it", model.display())
            } else {
                format!("load the separation model {}", model.display())
            }
        })?;
    Ok(Loaded { session, used_gpu })
}

/// Loads the model and runs it over one track.
pub fn separate(
    model: &Path,
    audio: &[f32],
    stem_count: usize,
    overlap: f64,
    card: Option<crate::lyrics_sync::OnnxCard>,
    progress: impl FnMut(f64),
) -> Result<Separated> {
    let mut loaded = load(model, card)?;
    separate_with(&mut loaded, audio, stem_count, overlap, progress)
}

/// Runs a loaded model over a whole track.
///
/// `progress` is called with a fraction between 0 and 1 after each segment, so
/// a long song can say how far along it is instead of appearing to hang.
pub fn separate_with(
    loaded: &mut Loaded,
    audio: &[f32],
    stem_count: usize,
    overlap: f64,
    mut progress: impl FnMut(f64),
) -> Result<Separated> {
    if audio.is_empty() {
        bail!("nothing to separate: the track decoded to no audio");
    }
    if stem_count > STEMS.len() {
        bail!("this model was declared with {stem_count} stems, more than the {} known", STEMS.len());
    }
    let (session, used_gpu) = (&mut loaded.session, loaded.used_gpu);

    let frames = audio.len() / CHANNELS;
    let overlap = overlap.clamp(0.0, 0.5);
    let step = (((SEGMENT_SAMPLES as f64) * (1.0 - overlap)) as usize).max(1);
    let mut sums = vec![vec![0f32; frames * CHANNELS]; stem_count];
    let mut weights = vec![0f32; frames];

    let window = fade_window(SEGMENT_SAMPLES, (SEGMENT_SAMPLES as f64 * overlap) as usize);
    let segments = frames.div_ceil(step).max(1);

    for (index, start) in (0..frames).step_by(step).enumerate() {
        // The tail of the song is shorter than a segment; the model still wants
        // a full one, so the rest is silence and is discarded afterwards.
        let mut planar = vec![0f32; SEGMENT_SAMPLES * CHANNELS];
        let taken = SEGMENT_SAMPLES.min(frames - start);
        for frame in 0..taken {
            for channel in 0..CHANNELS {
                planar[channel * SEGMENT_SAMPLES + frame] = audio[(start + frame) * CHANNELS + channel];
            }
        }

        let input = Value::from_array(([1usize, CHANNELS, SEGMENT_SAMPLES], planar))
            .context("build the model input")?;
        let outputs = session.run(ort::inputs!["mix" => input]).context("run the separation model")?;
        let (shape, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .context("read the separated stems")?;
        let produced = shape.get(1).copied().unwrap_or(0) as usize;
        if produced < stem_count {
            bail!("the model returned {produced} stems, fewer than the {stem_count} expected");
        }

        for stem in 0..stem_count {
            for frame in 0..taken {
                let gain = window[frame];
                for channel in 0..CHANNELS {
                    let offset = ((stem * CHANNELS) + channel) * SEGMENT_SAMPLES + frame;
                    sums[stem][(start + frame) * CHANNELS + channel] += data[offset] * gain;
                }
            }
        }
        for frame in 0..taken {
            weights[start + frame] += window[frame];
        }

        progress(((index + 1) as f64 / segments as f64).min(1.0));
    }

    // Undo the crossfade weighting, so a sample covered by two segments is not
    // twice as loud as one covered by a single segment.
    for stem in sums.iter_mut() {
        for frame in 0..frames {
            let weight = weights[frame];
            if weight <= f32::EPSILON {
                continue;
            }
            for channel in 0..CHANNELS {
                stem[frame * CHANNELS + channel] /= weight;
            }
        }
    }

    Ok(Separated {
        stems: sums
            .into_iter()
            .enumerate()
            .map(|(index, samples)| Stem { name: STEMS[index], samples })
            .collect(),
        used_gpu,
    })
}

/// A segment's gain curve: full in the middle, fading in and out across the
/// overlap so two neighbouring segments sum to one.
fn fade_window(length: usize, fade: usize) -> Vec<f32> {
    let mut window = vec![1f32; length];
    if fade == 0 {
        return window;
    }
    for index in 0..fade.min(length / 2) {
        let gain = (index as f32 + 0.5) / fade as f32;
        window[index] = gain;
        window[length - 1 - index] = gain;
    }
    window
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
    fn neighbouring_windows_sum_to_one_across_the_overlap() {
        let fade = (SEGMENT_SAMPLES as f64 * OVERLAP) as usize;
        let window = fade_window(SEGMENT_SAMPLES, fade);
        let step = ((SEGMENT_SAMPLES as f64) * (1.0 - OVERLAP)) as usize;
        // Where two segments overlap, their gains must add up to full volume,
        // or the seam is a dip the listener hears.
        for offset in 0..fade {
            let leaving = window[step + offset];
            let arriving = window[offset];
            assert!((leaving + arriving - 1.0).abs() < 0.05, "seam at {offset}: {leaving} + {arriving}");
        }
    }

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
}
