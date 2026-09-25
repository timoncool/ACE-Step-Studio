//! Tempo and key measured from the recording itself.
//!
//! A captioning model hears a song well and its numbers badly: MOSS called a
//! 90 BPM song in E major 102 BPM in C# minor. So the numbers come from models
//! built for them, run through ONNX on the processor:
//!
//! - tempo: Beat This! (CPJKU, MIT), via the ONNX export of beat_this_cpp
//!   (mosynthkey, MIT). Its log-mel front end, chunked inference and DBN
//!   post-processing (beat_dbn) are ported from that port; the tempo is a
//!   grid fit through the beats.
//! - key: S-KEY (Deezer, ICASSP 2025, MIT), via the ONNX export
//!   aaatmy/skey-onnx, which takes raw 22.05 kHz audio and returns 24 scores.

use std::path::Path;

use anyhow::{bail, Context, Result};
use ort::session::Session;
use ort::value::Value;
use realfft::RealFftPlanner;
use serde::Serialize;

const RATE: u32 = 22_050;
const N_FFT: usize = 1024;
const HOP: usize = 441;
const N_MELS: usize = 128;
const F_MIN: f64 = 30.0;
const F_MAX: f64 = 11_000.0;
const CHUNK: usize = 1500;
const BORDER: usize = 6;

/// S-KEY's classes, in the order of its output.
const KEYS: [&str; 24] = [
    "A major", "Bb major", "B major", "C major", "C# major", "D major", "D# major", "E major", "F major", "F# major",
    "G major", "G# major", "B minor", "C minor", "C# minor", "D minor", "D# minor", "E minor", "F minor", "F# minor",
    "G minor", "G# minor", "A minor", "Bb minor",
];

pub const BEAT_MODEL: &str = "beat_this.onnx";
pub const KEY_MODEL: &str = "skey.onnx";

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Facts {
    /// Beats per minute, rounded.
    pub bpm: u32,
    /// Tonic and mode, "C minor".
    pub key: String,
}

impl Facts {
    pub fn tonic(&self) -> &str {
        self.key.split_whitespace().next().unwrap_or_default()
    }

    pub fn mode(&self) -> &str {
        self.key.split_whitespace().nth(1).unwrap_or_default()
    }
}

/// Beat This! and S-KEY loaded once for a whole batch of songs.
pub struct Measurer {
    beat: Session,
    key: Session,
    /// True when the graphics card does the work.
    pub on_gpu: bool,
}

impl Measurer {
    /// Loads both models, on the card when asked and when it takes them.
    pub fn load(models: &Path, on_gpu: bool) -> Result<Self> {
        let (beat, beat_gpu) = session(&models.join(BEAT_MODEL), on_gpu)?;
        let (key, key_gpu) = session(&models.join(KEY_MODEL), on_gpu)?;
        Ok(Self { beat, key, on_gpu: beat_gpu && key_gpu })
    }

    /// Measures decoded audio, mono at 22.05 kHz.
    pub fn measure_mono(&mut self, mono: &[f32]) -> Result<Facts> {
        if mono.len() < RATE as usize * 3 {
            bail!("the song is shorter than three seconds, too short to measure");
        }
        let bpm = tempo(mono, &mut self.beat)?;
        let key = key(mono, &mut self.key)?;
        Ok(Facts { bpm, key })
    }
}

/// The song decoded for measuring: mono at 22.05 kHz.
pub fn decode(audio: &Path) -> Result<Vec<f32>> {
    let stereo = crate::audio_pcm::decode_stereo(audio)?.resampled(RATE)?;
    Ok(stereo.left.iter().zip(&stereo.right).map(|(left, right)| 0.5 * (left + right)).collect())
}

/// A session on the card when asked; says whether the card took it.
fn session(model: &Path, on_gpu: bool) -> Result<(Session, bool)> {
    let mut builder = Session::builder().context("prepare an ONNX session")?;
    if on_gpu {
        // `error_on_failure`: without it the runtime quietly runs on the processor
        if let Ok(mut with_cuda) = builder.clone().with_execution_providers([ort::ep::CUDA::default().build().error_on_failure()]) {
            if let Ok(session) = with_cuda.commit_from_file(model) {
                return Ok((session, true));
            }
        }
    }
    let session = builder.commit_from_file(model).with_context(|| format!("load {}", model.display()))?;
    Ok((session, false))
}

// ---- key ------------------------------------------------------------------

fn key(mono: &[f32], session: &mut Session) -> Result<String> {
    let input = Value::from_array(([1usize, mono.len()], mono.to_vec())).context("wrap the audio for S-KEY")?;
    let outputs = session.run(ort::inputs!["audio" => input]).context("run S-KEY")?;
    let (_, scores) = outputs["scores"].try_extract_tensor::<f32>().context("read the S-KEY scores")?;
    if scores.len() != KEYS.len() {
        bail!("S-KEY returned {} scores, expected {}", scores.len(), KEYS.len());
    }
    let best = scores
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(index, _)| index)
        .context("S-KEY returned no scores")?;
    Ok(KEYS[best].to_string())
}

// ---- tempo ----------------------------------------------------------------

fn tempo(mono: &[f32], session: &mut Session) -> Result<u32> {
    let spect = log_mel(mono);
    if spect.len() < 2 * BORDER + 2 {
        bail!("too little audio for beat tracking");
    }
    let (beat, downbeat) = beat_logits(session, &spect)?;
    let times = crate::beat_dbn::beat_times(&beat, &downbeat, &crate::beat_dbn::Config::default());
    if times.len() < 4 {
        bail!("no steady beat was found");
    }
    let period = beat_period(&times);
    if period <= 0.0 {
        bail!("the beats found have no spacing");
    }
    Ok((60.0 / period).round() as u32)
}

/// The beat period: a least-squares grid through the beat times, as the Key &
/// Tempo project fits Beat This! beats (frontend/inference/tempo.js). Without
/// a DBN pass the tracker now and then misses a beat, and a missed beat is a
/// double interval that drags a plain average down - 110 BPM read as 103 - so
/// each interval first counts the beats it spans at the median spacing, so a
/// skipped beat takes the index after it.
fn beat_period(times: &[f64]) -> f64 {
    let mut intervals: Vec<f64> = times.windows(2).map(|pair| pair[1] - pair[0]).collect();
    intervals.sort_by(f64::total_cmp);
    let median = intervals[intervals.len() / 2];
    if median <= 0.0 {
        return 0.0;
    }
    // Each interval counts the beats it spans, so rounding never accumulates
    // along the song
    let mut indices = vec![0.0f64];
    for pair in times.windows(2) {
        let spans = ((pair[1] - pair[0]) / median).round().max(1.0);
        indices.push(indices.last().expect("the first index") + spans);
    }
    let count = times.len() as f64;
    let mean_index = indices.iter().sum::<f64>() / count;
    let mean_time = times.iter().sum::<f64>() / count;
    let (mut numerator, mut denominator) = (0.0, 0.0);
    for (index, time) in indices.iter().zip(times) {
        numerator += (index - mean_index) * (time - mean_time);
        denominator += (index - mean_index).powi(2);
    }
    if denominator > 0.0 { numerator / denominator } else { median }
}

/// torchaudio-compatible log-mel spectrogram, `[frames][128]`, as Beat This!
/// was trained on: reflect padding, periodic Hann, magnitude / sqrt(n_fft),
/// Slaney mel scale, log1p(1000 x).
fn log_mel(audio: &[f32]) -> Vec<[f32; N_MELS]> {
    let pad = N_FFT / 2;
    if audio.len() <= pad {
        return Vec::new();
    }
    let mut padded = Vec::with_capacity(audio.len() + 2 * pad);
    padded.extend((1..=pad).rev().map(|index| audio[index]));
    padded.extend_from_slice(audio);
    padded.extend((1..=pad).map(|index| audio[audio.len() - 1 - index]));
    let frames = (padded.len() - N_FFT) / HOP + 1;

    let window: Vec<f32> =
        (0..N_FFT).map(|index| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * index as f32 / N_FFT as f32).cos())).collect();
    let bank = mel_bank();
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N_FFT);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let norm = (N_FFT as f64).sqrt();

    let mut out = Vec::with_capacity(frames);
    let mut magnitude = vec![0.0f64; N_FFT / 2 + 1];
    for frame in 0..frames {
        let start = frame * HOP;
        for (index, value) in input.iter_mut().enumerate() {
            *value = padded[start + index] * window[index];
        }
        fft.process(&mut input, &mut spectrum).expect("fixed-size FFT");
        for (bin, value) in spectrum.iter().enumerate() {
            magnitude[bin] = (value.re as f64).hypot(value.im as f64) / norm;
        }
        let mut row = [0.0f32; N_MELS];
        for (mel, cell) in row.iter_mut().enumerate() {
            let energy: f64 = magnitude.iter().zip(&bank).map(|(m, weights)| m * weights[mel]).sum();
            *cell = (1000.0 * energy.max(1e-10)).ln_1p() as f32;
        }
        out.push(row);
    }
    out
}

fn hz_to_mel(hz: f64) -> f64 {
    let f_sp = 200.0 / 3.0;
    let min_log_hz = 1000.0;
    let min_log_mel = min_log_hz / f_sp;
    let logstep = 6.4f64.ln() / 27.0;
    if hz >= min_log_hz {
        min_log_mel + (hz / min_log_hz).ln() / logstep
    } else {
        hz / f_sp
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    let f_sp = 200.0 / 3.0;
    let min_log_hz = 1000.0;
    let min_log_mel = min_log_hz / f_sp;
    let logstep = 6.4f64.ln() / 27.0;
    if mel >= min_log_mel {
        min_log_hz * (logstep * (mel - min_log_mel)).exp()
    } else {
        f_sp * mel
    }
}

/// `[n_fft/2+1][128]` triangular filters, Slaney scale, unnormalised.
fn mel_bank() -> Vec<[f64; N_MELS]> {
    let (low, high) = (hz_to_mel(F_MIN), hz_to_mel(F_MAX));
    let points: Vec<f64> = (0..N_MELS + 2).map(|index| mel_to_hz(low + (high - low) * index as f64 / (N_MELS + 1) as f64)).collect();
    (0..N_FFT / 2 + 1)
        .map(|bin| {
            let hz = bin as f64 * RATE as f64 / N_FFT as f64;
            let mut row = [0.0f64; N_MELS];
            for (mel, cell) in row.iter_mut().enumerate() {
                let (left, center, right) = (points[mel], points[mel + 1], points[mel + 2]);
                let rising = if center > left { (hz - left) / (center - left) } else { 0.0 };
                let falling = if right > center { (right - hz) / (right - center) } else { 0.0 };
                *cell = rising.min(falling).max(0.0);
            }
            row
        })
        .collect()
}

/// Runs the model over overlapping chunks and keeps each chunk's centre, as
/// Beat This! does: chunks of 1500 frames, 6 frames of border thrown away.
fn beat_logits(session: &mut Session, spect: &[[f32; N_MELS]]) -> Result<(Vec<f32>, Vec<f32>)> {
    let length = spect.len() as isize;
    let (chunk, border) = (CHUNK as isize, BORDER as isize);
    let mut starts: Vec<isize> = Vec::new();
    let mut start = -border;
    while start < length - border {
        starts.push(start);
        start += chunk - 2 * border;
    }
    if length > chunk - 2 * border {
        *starts.last_mut().expect("at least one chunk") = length - (chunk - border);
    }

    let mut logits = vec![-1000.0f32; spect.len()];
    let mut down_logits = vec![-1000.0f32; spect.len()];
    for &start in starts.iter().rev() {
        let from = start.max(0);
        let to = (start + chunk).min(length);
        let left_pad = (-start).max(0) as usize;
        let right_pad = (start + chunk - length).clamp(0, border) as usize;
        let rows = (to - from).max(0) as usize + left_pad + right_pad;
        let mut data = vec![0.0f32; rows * N_MELS];
        for (offset, row) in spect[from as usize..to as usize].iter().enumerate() {
            let at = (left_pad + offset) * N_MELS;
            data[at..at + N_MELS].copy_from_slice(row);
        }
        let input = Value::from_array(([1usize, rows, N_MELS], data)).context("wrap the spectrogram for Beat This!")?;
        let outputs = session.run(ort::inputs!["input_spectrogram" => input]).context("run Beat This!")?;
        let (_, beat) = outputs["beat"].try_extract_tensor::<f32>().context("read the beat activations")?;
        let (_, down) = outputs["downbeat"].try_extract_tensor::<f32>().context("read the downbeat activations")?;
        let keep = if beat.len() < 2 * BORDER { 0..beat.len() } else { BORDER..beat.len() - BORDER };
        for index in keep {
            let target = start + index as isize;
            if target >= 0 && target < length {
                logits[target as usize] = beat[index];
                down_logits[target as usize] = down[index];
            }
        }
    }
    Ok((logits, down_logits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mel_scale_round_trips() {
        for hz in [30.0, 440.0, 1000.0, 4000.0, 11_000.0] {
            assert!((mel_to_hz(hz_to_mel(hz)) - hz).abs() < 1e-6);
        }
    }

    #[test]
    #[ignore]
    fn measures_real_songs() {
        let models = std::path::PathBuf::from(std::env::var("AUDIO_FACTS_MODELS").expect("AUDIO_FACTS_MODELS"));
        let mut measurer = Measurer::load(&models, std::env::var("AUDIO_FACTS_GPU").is_ok()).expect("load the models");
        println!("on the card: {}", measurer.on_gpu);
        for song in std::env::var("AUDIO_FACTS_SONGS").expect("AUDIO_FACTS_SONGS").split(';') {
            let started = std::time::Instant::now();
            let facts = measurer.measure_mono(&decode(Path::new(song)).expect("decode")).expect("measure");
            println!("{song}: {} BPM, {} ({:.1} s)", facts.bpm, facts.key, started.elapsed().as_secs_f32());
        }
    }

    #[test]
    fn a_steady_beat_gives_its_tempo() {
        let times: Vec<f64> = (0..40).map(|beat| beat as f64 * 0.5).collect();
        assert!((beat_period(&times) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn missed_beats_do_not_slow_the_tempo() {
        // 110 BPM with every seventh beat missed
        let times: Vec<f64> = (0..200).filter(|beat| beat % 7 != 3).map(|beat| beat as f64 * 60.0 / 110.0).collect();
        assert_eq!((60.0 / beat_period(&times)).round(), 110.0);
    }

    #[test]
    fn the_key_splits_into_tonic_and_mode() {
        let facts = Facts { bpm: 120, key: "C# minor".into() };
        assert_eq!((facts.tonic(), facts.mode()), ("C#", "minor"));
    }
}
