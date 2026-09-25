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
}

impl Default for SeparationConfig {
    fn default() -> Self {
        Self {
            runtime: crate::lyrics_sync::OnnxFlavour::default(),
            stems: STEMS.iter().map(|stem| (*stem).to_string()).collect(),
            overlap: OVERLAP,
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

/// Loads the model, on the card when asked.
pub fn load(model: &Path, on_gpu: bool) -> Result<Loaded> {
    // Ask for the card, and say so if it is not there rather than quietly
    // spending ten times as long on the processor.
    let mut builder = Session::builder().context("prepare an ONNX session")?;
    let mut used_gpu = false;
    if on_gpu {
        // `error_on_failure` matters: without it the runtime accepts the
        // provider, quietly runs on the processor anyway, and the only clue is
        // that the fans never spin up.
        match builder.clone().with_execution_providers([
            ort::ep::CUDA::default().build().error_on_failure(),
        ]) {
            Ok(with_cuda) => {
                builder = with_cuda;
                used_gpu = true;
            }
            Err(error) => eprintln!("the card provider did not register, falling back to the processor: {error}"),
        }
    }
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
    on_gpu: bool,
    progress: impl FnMut(f64),
) -> Result<Separated> {
    let mut loaded = load(model, on_gpu)?;
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
        let path = std::env::temp_dir().join(format!("mm3-stem-{}.wav", uuid::Uuid::now_v7()));
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
