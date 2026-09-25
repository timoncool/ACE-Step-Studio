//! Spectral Lifter: cleanup for AI-generated audio.
//!
//! One analysis, then in order: a soft spectral gate against hiss, an optional
//! extension of the high band the model rolled off, an optional lift of the
//! percussive part, and multiband control of sibilance, shimmer and the top
//! octave; one synthesis at the end. Every gain is computed from the mid signal
//! and applied to both channels, so the stereo image does not move.

use serde::{Deserialize, Serialize};

use crate::denoise::{blur_over_frequency, noise_floor, smooth_over_time};
use crate::stft::{apply_mask, mid_magnitudes, Spectrogram, Stft};
use crate::Stereo;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct LifterSettings {
    /// 0 off, 1 strongest hiss gate.
    pub denoise_strength: f32,
    /// Lowest gain the gate leaves, 0.01 to 0.5.
    pub noise_floor: f32,
    /// How much mirrored high band to add above the roll-off, 0 to 0.5.
    pub hf_mix: f32,
    /// Percussive lift, 0 to 1.
    pub transient_boost: f32,
    /// Attenuation of the 10 to 14 kHz shimmer band, 0 to 12 dB.
    pub shimmer_reduction_db: f32,
}

impl Default for LifterSettings {
    fn default() -> Self {
        Self { denoise_strength: 0.3, noise_floor: 0.1, hf_mix: 0.0, transient_boost: 0.0, shimmer_reduction_db: 6.0 }
    }
}

const SIZE: usize = 2048;
const HOP: usize = 512;

fn hz_to_bin(hz: f32, rate: u32, bins: usize) -> usize {
    ((hz * SIZE as f32 / rate as f32) as usize).min(bins - 1)
}

/// The bin where the mean spectrum falls fastest between 12 and 16 kHz.
fn roll_off_bin(magnitudes: &[Vec<f32>], rate: u32) -> usize {
    let bins = magnitudes[0].len();
    let mut mean = vec![0.0f32; bins];
    for frame in magnitudes {
        for (m, v) in mean.iter_mut().zip(frame) {
            *m += v;
        }
    }
    let reference = mean.iter().cloned().fold(0.0f32, f32::max);
    let (lo, hi) = (hz_to_bin(12_000.0, rate, bins), hz_to_bin(16_000.0, rate, bins));
    if reference <= 1e-10 || lo + 1 >= hi {
        return hi;
    }
    let db = |b: usize| 20.0 * (mean[b].max(1e-10) / reference).log10();
    let mut steepest = (0.0f32, lo);
    for b in lo..hi {
        let drop = db(b + 1) - db(b);
        if drop < steepest.0 {
            steepest = (drop, b);
        }
    }
    steepest.1
}

fn gate(magnitudes: &[Vec<f32>], strength: f32, floor: f32) -> Vec<Vec<f32>> {
    let noise = noise_floor(magnitudes, 0.05);
    let over_subtraction = 0.5 + 3.5 * strength;
    let mut mask: Vec<Vec<f32>> = magnitudes
        .iter()
        .map(|frame| {
            frame
                .iter()
                .zip(&noise)
                .map(|(&m, &n)| {
                    let threshold = n * over_subtraction;
                    let softness = (threshold * 0.3).max(1e-8);
                    1.0 / (1.0 + (-(m - threshold) / softness).exp())
                })
                .collect()
        })
        .collect();
    smooth_over_time(&mut mask, 0.3, 0.5 + 0.49 * 0.7);
    blur_over_frequency(&mut mask, 4);
    for row in mask.iter_mut() {
        row.iter_mut().for_each(|g| *g = g.max(floor));
    }
    mask
}

/// Copies the 8 to 16 kHz band above the roll-off, fading out as it climbs.
fn extend_high_band(spectrogram: &mut Spectrogram, rate: u32, roll_off: usize, mix: f32) {
    let bins = spectrogram.bins();
    let source_lo = hz_to_bin(8_000.0, rate, bins).max(1);
    let source_hi = hz_to_bin(16_000.0, rate, bins);
    let roll_off_hz = roll_off as f32 * rate as f32 / SIZE as f32;
    let destination = hz_to_bin((roll_off_hz - 1000.0).max(12_000.0), rate, bins).max(1);
    let span = source_hi.saturating_sub(source_lo);
    if span == 0 {
        return;
    }
    for frame in spectrogram.frames.iter_mut() {
        let source: Vec<_> = frame[source_lo..source_hi].to_vec();
        for (i, value) in source.iter().enumerate() {
            let target = destination + i;
            if target >= bins {
                break;
            }
            let taper = (1.0 - i as f32 / span as f32).powi(2);
            frame[target] += *value * (mix * taper);
        }
    }
}

/// Gains above 1 where a bin stands out of its neighbours in frequency, the
/// percussive part; the result is added to the signal, so the mask is `1 + boost * p`.
fn percussive_mask(magnitudes: &[Vec<f32>], boost: f32) -> Vec<Vec<f32>> {
    const WIDTH: usize = 15;
    magnitudes
        .iter()
        .map(|frame| {
            let bins = frame.len();
            let mut window = [0.0f32; WIDTH];
            (0..bins)
                .map(|b| {
                    for (k, slot) in window.iter_mut().enumerate() {
                        let index = (b as isize + k as isize - (WIDTH / 2) as isize).clamp(0, bins as isize - 1) as usize;
                        *slot = frame[index];
                    }
                    window.sort_by(|x, y| x.partial_cmp(y).unwrap());
                    let median = window[WIDTH / 2];
                    let signal = frame[b];
                    let ratio = if signal > 1e-10 { (signal / (median + 1e-10)).min(4.0) } else { 0.0 };
                    1.0 + boost * (ratio - 1.0).clamp(0.0, 1.0)
                })
                .collect()
        })
        .collect()
}

/// Per-frame gain on three bands where their energy passes a percentile.
fn band_dynamics(magnitudes: &[Vec<f32>], rate: u32, shimmer_db: f32) -> Vec<Vec<f32>> {
    let frames = magnitudes.len();
    let bins = magnitudes[0].len();
    let mut mask = vec![vec![1.0f32; bins]; frames];
    let bands = [(5_000.0, 8_000.0, 3.0, 0.80), (10_000.0, 14_000.0, shimmer_db, 0.60), (18_000.0, 24_000.0, 12.0, 0.50)];
    for (lo_hz, hi_hz, reduction_db, percentile) in bands {
        if reduction_db <= 0.0 {
            continue;
        }
        let lo = hz_to_bin(lo_hz, rate, bins);
        let hi = ((hi_hz * SIZE as f32 / rate as f32) as usize).min(bins);
        if lo >= hi {
            continue;
        }
        let energy: Vec<f32> = magnitudes.iter().map(|frame| frame[lo..hi].iter().sum::<f32>() / (hi - lo) as f32).collect();
        let mut sorted = energy.clone();
        sorted.sort_by(|x, y| x.partial_cmp(y).unwrap());
        let threshold = sorted[((frames as f32 * percentile) as usize).min(frames - 1)];
        let reduced = 10f32.powf(-reduction_db / 20.0);
        let gains: Vec<f32> = energy
            .iter()
            .map(|&e| if e > threshold { reduced + (1.0 - reduced) * threshold / (e + 1e-10) } else { 1.0 })
            .collect();
        for f in 0..frames {
            let (from, to) = (f.saturating_sub(2), (f + 2).min(frames - 1));
            let smoothed = (gains[from..=to].iter().sum::<f32>() / (to - from + 1) as f32).clamp(reduced, 1.0);
            for g in &mut mask[f][lo..hi] {
                *g *= smoothed;
            }
        }
    }
    mask
}

pub fn lift(audio: &Stereo, settings: &LifterSettings) -> Stereo {
    let enabled = settings.denoise_strength > 0.0
        || settings.hf_mix > 0.0
        || settings.transient_boost > 0.0
        || settings.shimmer_reduction_db > 0.0;
    if !enabled || audio.frames() < SIZE {
        return audio.clone();
    }
    let stft = Stft::new(SIZE, HOP);
    let mut left = stft.analyze(&audio.left);
    let mut right = stft.analyze(&audio.right);
    let rate = audio.rate;

    let magnitudes = mid_magnitudes(&left, &right);
    let roll_off = roll_off_bin(&magnitudes, rate);

    if settings.denoise_strength > 0.0 {
        let mask = gate(&magnitudes, settings.denoise_strength.clamp(0.0, 1.0), settings.noise_floor.clamp(0.01, 0.5));
        apply_mask(&mut left, &mut right, &mask);
    }
    if settings.hf_mix > 0.0 {
        let mix = settings.hf_mix.clamp(0.0, 0.5);
        extend_high_band(&mut left, rate, roll_off, mix);
        extend_high_band(&mut right, rate, roll_off, mix);
    }
    if settings.transient_boost > 0.0 {
        let mask = percussive_mask(&mid_magnitudes(&left, &right), settings.transient_boost.clamp(0.0, 1.0));
        apply_mask(&mut left, &mut right, &mask);
    }
    if settings.shimmer_reduction_db > 0.0 {
        let mask = band_dynamics(&mid_magnitudes(&left, &right), rate, settings.shimmer_reduction_db.clamp(0.0, 12.0));
        apply_mask(&mut left, &mut right, &mask);
    }

    let mut out = Stereo::new(stft.synthesize(&left), stft.synthesize(&right), rate);
    out.keep_below(0.999);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_keeps_its_length_and_stays_finite() {
        let rate = 48_000;
        let n = rate as usize * 4;
        let left: Vec<f32> = (0..n).map(|i| 0.3 * ((i as f32) * 0.02).sin() + 0.05 * ((i as f32) * 1.7).sin()).collect();
        let right: Vec<f32> = left.iter().map(|v| v * 0.8).collect();
        let audio = Stereo::new(left, right, rate);
        let out = lift(&audio, &LifterSettings { hf_mix: 0.2, transient_boost: 0.5, ..LifterSettings::default() });
        assert_eq!(out.frames(), n);
        assert!(out.left.iter().chain(&out.right).all(|v| v.is_finite()));
        // the left/right balance survives: both channels were processed alike
        let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
        let ratio = energy(&out.right) / energy(&out.left);
        assert!((ratio - 0.64).abs() < 0.02, "{ratio}");
    }
}
