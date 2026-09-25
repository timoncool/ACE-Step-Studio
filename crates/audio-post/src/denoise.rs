//! Noise reduction for generated audio: the steady hiss and fizz a decoder
//! leaves under the music.
//!
//! The noise floor of each frequency bin is estimated from the quietest frames
//! of the track itself (a low percentile of its magnitude over time), and a
//! Wiener-style spectral subtraction removes that much from every frame:
//! bins near the floor are pulled down hard, bins well above it barely move.
//! The gains are smoothed over time and frequency against musical noise and
//! applied to both channels alike.

use serde::{Deserialize, Serialize};

use crate::stft::{apply_mask, mid_magnitudes, Stft};
use crate::Stereo;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct DenoiseSettings {
    /// 0 off, 1 strongest.
    pub strength: f32,
    /// 0 sharp, 1 smooth gain changes over time and frequency.
    pub smoothing: f32,
    /// Dry/wet: 0 keeps the original, 1 is fully processed.
    pub mix: f32,
}

impl Default for DenoiseSettings {
    fn default() -> Self {
        Self { strength: 0.4, smoothing: 0.5, mix: 1.0 }
    }
}

const SIZE: usize = 8192;
const HOP: usize = SIZE / 4;
const FLOOR: f32 = 0.02;
const NOISE_PERCENTILE: f32 = 0.1;

pub fn denoise(audio: &Stereo, settings: &DenoiseSettings) -> Stereo {
    let strength = settings.strength.clamp(0.0, 1.0);
    let smoothing = settings.smoothing.clamp(0.0, 1.0);
    let mix = settings.mix.clamp(0.0, 1.0);
    if strength <= 0.0 || mix <= 0.0 || audio.frames() < SIZE {
        return audio.clone();
    }
    let stft = Stft::new(SIZE, HOP);
    let mut left = stft.analyze(&audio.left);
    let mut right = stft.analyze(&audio.right);
    let magnitudes = mid_magnitudes(&left, &right);
    let noise = noise_floor(&magnitudes, NOISE_PERCENTILE);

    let over_subtraction = 0.5 + 3.5 * strength;
    let mut mask: Vec<Vec<f32>> = magnitudes
        .iter()
        .map(|frame| {
            frame
                .iter()
                .zip(&noise)
                .map(|(&m, &n)| {
                    // power subtraction: |S| = sqrt(|X|^2 - |N|^2)
                    let signal = m * m;
                    let floor = (n * over_subtraction).powi(2);
                    if signal > 1e-12 { (1.0 - floor / signal).max(0.0).sqrt().max(FLOOR) } else { FLOOR }
                })
                .collect()
        })
        .collect();

    // Mostly over time: a wide blur across bins would spread the low gains of
    // the noise around a note onto the note's own narrow peak.
    smooth_over_time(&mut mask, 0.3, 0.5 + 0.49 * smoothing);
    blur_over_frequency(&mut mask, 1 + (2.0 * smoothing).round() as usize);
    apply_mask(&mut left, &mut right, &mask);

    let wet_left = stft.synthesize(&left);
    let wet_right = stft.synthesize(&right);
    let blend = |dry: &[f32], wet: &[f32]| dry.iter().zip(wet).map(|(d, w)| (1.0 - mix) * d + mix * w).collect();
    Stereo::new(blend(&audio.left, &wet_left), blend(&audio.right, &wet_right), audio.rate)
}

/// The noise floor of each bin: a low percentile of its magnitude over time,
/// then a median across neighbouring bins. The percentile alone would take a
/// held note for noise, since a steady tone is as constant as hiss; hiss is
/// broadband and survives the median, a tone is a narrow peak and does not.
pub(crate) fn noise_floor(magnitudes: &[Vec<f32>], percentile: f32) -> Vec<f32> {
    let frames = magnitudes.len();
    let bins = magnitudes[0].len();
    let mut over_time = vec![0.0f32; bins];
    let mut column = vec![0.0f32; frames];
    let k = ((frames as f32 * percentile) as usize).min(frames - 1);
    for b in 0..bins {
        for f in 0..frames {
            column[f] = magnitudes[f][b];
        }
        column.select_nth_unstable_by(k, |x, y| x.partial_cmp(y).unwrap());
        over_time[b] = column[k];
    }
    // the neighbourhood widens with frequency, about a sixth of an octave
    let mut out = vec![0.0f32; bins];
    let mut window = Vec::new();
    for b in 0..bins {
        let half = (b / 24).max(8);
        let (lo, hi) = (b.saturating_sub(half), (b + half).min(bins - 1));
        window.clear();
        window.extend_from_slice(&over_time[lo..=hi]);
        let mid = window.len() / 2;
        window.select_nth_unstable_by(mid, |x, y| x.partial_cmp(y).unwrap());
        out[b] = window[mid];
    }
    out
}

/// Attack/release smoothing forward, then a softer pass backward.
pub(crate) fn smooth_over_time(mask: &mut [Vec<f32>], attack: f32, release: f32) {
    let frames = mask.len();
    if frames < 2 {
        return;
    }
    let bins = mask[0].len();
    for b in 0..bins {
        let mut previous = mask[0][b];
        for frame in mask.iter_mut().skip(1) {
            let current = frame[b];
            let c = if current > previous { attack } else { release };
            previous = c * previous + (1.0 - c) * current;
            frame[b] = previous;
        }
        let mut previous = mask[frames - 1][b];
        for frame in mask.iter_mut().rev().skip(1) {
            let current = frame[b];
            previous = 0.5 * current + 0.5 * (release * previous + (1.0 - release) * current);
            frame[b] = previous;
        }
    }
}

/// Gaussian blur across bins, sigma in bins.
pub(crate) fn blur_over_frequency(mask: &mut [Vec<f32>], sigma: usize) {
    if sigma <= 1 {
        return;
    }
    let kernel: Vec<f32> =
        (0..=2 * sigma).map(|k| (-0.5 * ((k as f32 - sigma as f32) / sigma as f32).powi(2)).exp()).collect();
    for row in mask.iter_mut() {
        let source = row.clone();
        let bins = source.len();
        for b in 0..bins {
            let (mut sum, mut weight) = (0.0, 0.0);
            for (k, &g) in kernel.iter().enumerate() {
                let index = b as isize + k as isize - sigma as isize;
                if index >= 0 && (index as usize) < bins {
                    sum += source[index as usize] * g;
                    weight += g;
                }
            }
            row[b] = sum / weight;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noisy_tone(seconds: f32, rate: u32) -> (Stereo, Vec<f32>) {
        let n = (seconds * rate as f32) as usize;
        let mut seed = 12345u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
        };
        let clean: Vec<f32> = (0..n).map(|i| 0.4 * (2.0 * std::f32::consts::PI * 330.0 * i as f32 / rate as f32).sin()).collect();
        let left: Vec<f32> = clean.iter().map(|&c| c + 0.02 * noise()).collect();
        (Stereo::new(left.clone(), left, rate), clean)
    }

    #[test]
    fn hiss_goes_down_and_the_tone_stays() {
        let (noisy, clean) = noisy_tone(6.0, 48_000);
        let out = denoise(&noisy, &DenoiseSettings { strength: 0.6, smoothing: 0.5, mix: 1.0 });
        // split the output into the tone (best-fit gain) and what is left over
        let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (*x as f64) * (*y as f64)).sum::<f64>();
        let residual = |x: &[f32]| {
            let gain = dot(x, &clean) / dot(&clean, &clean);
            let power = x.iter().zip(&clean).map(|(a, b)| (*a as f64 - gain * *b as f64).powi(2)).sum::<f64>();
            (gain, (power / clean.len() as f64).sqrt())
        };
        let (_, before) = residual(&noisy.left);
        let (gain, after) = residual(&out.left);
        assert!(after < before * 0.7, "residual {before} -> {after}");
        assert!((gain - 1.0).abs() < 0.03, "the tone moved by {gain}");
        assert_eq!(out.frames(), noisy.frames());

        let strongest = denoise(&noisy, &DenoiseSettings { strength: 1.0, smoothing: 0.5, mix: 1.0 });
        let (gain, stronger) = residual(&strongest.left);
        assert!(stronger < after, "strength 1 leaves {stronger}, 0.6 left {after}");
        assert!((gain - 1.0).abs() < 0.05, "the tone moved by {gain} at full strength");
    }

    #[test]
    fn strength_zero_is_the_original() {
        let (noisy, _) = noisy_tone(2.0, 48_000);
        let out = denoise(&noisy, &DenoiseSettings { strength: 0.0, ..DenoiseSettings::default() });
        assert_eq!(out.left, noisy.left);
    }
}
