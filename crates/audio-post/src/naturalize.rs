//! Vocal naturaliser: five band-targeted stages on the full mix that loosen
//! the machine steadiness of a generated voice, from ComfyUI_MusicTools
//! (Jean Kassio) by way of HOT-Step. No separation: every stage works in the
//! vocal bands directly, so nothing is lost to a split and a remix.

use serde::{Deserialize, Serialize};

use crate::Stereo;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct NaturalizeSettings {
    /// Master intensity, 0 to 1.
    pub amount: f32,
    /// Vibrato rate in Hz, 3 to 7.
    pub vibrato_rate: f32,
    pub vibrato_depth: f32,
    pub formant_strength: f32,
    /// Attenuation of the 6 to 10 kHz metallic band.
    pub metallic_reduction: f32,
    /// Shaped 1 to 4 kHz noise that masks quantisation; raises the noise floor, off by default.
    pub quantization_mask: f32,
    pub transition_smooth: f32,
    /// Seed of the formant and masking noise, so a result can be made again.
    pub seed: u32,
}

impl Default for NaturalizeSettings {
    fn default() -> Self {
        Self {
            amount: 0.5,
            vibrato_rate: 4.5,
            vibrato_depth: 1.0,
            formant_strength: 1.0,
            metallic_reduction: 1.0,
            quantization_mask: 0.0,
            transition_smooth: 1.0,
            seed: 1,
        }
    }
}

#[derive(Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    fn lowpass(cutoff: f64, rate: f64) -> Self {
        let wc = (std::f64::consts::PI * cutoff / rate).tan();
        let wc2 = wc * wc;
        let norm = 1.0 / (1.0 + std::f64::consts::SQRT_2 * wc + wc2);
        Self {
            b0: wc2 * norm,
            b1: 2.0 * wc2 * norm,
            b2: wc2 * norm,
            a1: 2.0 * (wc2 - 1.0) * norm,
            a2: (1.0 - std::f64::consts::SQRT_2 * wc + wc2) * norm,
        }
    }

    fn bandpass(low: f64, high: f64, rate: f64) -> Self {
        let wl = (std::f64::consts::PI * low / rate).tan();
        let wh = (std::f64::consts::PI * high / rate).tan();
        let w0 = (wl * wh).sqrt();
        let q = w0 / (wh - wl);
        let alpha = (2.0 * w0.atan()).sin() / (2.0 * q);
        let cos_w0 = (1.0 - w0 * w0) / (1.0 + w0 * w0);
        let norm = 1.0 / (1.0 + alpha);
        Self { b0: alpha * norm, b1: 0.0, b2: -alpha * norm, a1: -2.0 * cos_w0 * norm, a2: (1.0 - alpha) * norm }
    }

    fn run(&self, input: &[f64]) -> Vec<f64> {
        let (mut z1, mut z2) = (0.0, 0.0);
        input
            .iter()
            .map(|&x| {
                let y = self.b0 * x + z1;
                z1 = self.b1 * x - self.a1 * y + z2;
                z2 = self.b2 * x - self.a2 * y;
                y
            })
            .collect()
    }
}

/// Gaussian noise from a small seeded generator (SplitMix64 and Box-Muller).
fn gaussian(length: usize, seed: u64) -> Vec<f64> {
    let mut state = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut out = vec![0.0; length];
    let mut i = 0;
    while i < length {
        let u1 = next().max(1e-12);
        let u2 = next();
        let magnitude = (-2.0 * u1.ln()).sqrt();
        out[i] = magnitude * (2.0 * std::f64::consts::PI * u2).cos();
        if i + 1 < length {
            out[i + 1] = magnitude * (2.0 * std::f64::consts::PI * u2).sin();
        }
        i += 2;
    }
    out
}

fn channel(audio: &[f32], rate: u32, s: &NaturalizeSettings, seed: u64) -> Vec<f32> {
    let amount = s.amount as f64;
    let n = audio.len();
    let source: Vec<f64> = audio.iter().map(|&v| v as f64).collect();
    let mut result = source.clone();
    let rate_f = rate as f64;

    // 1. pitch variation, a slow amplitude wobble at the vibrato rate
    let depth_setting = s.vibrato_depth as f64;
    if depth_setting > 0.01 {
        let depth = 0.002 * amount * depth_setting;
        for i in 0..n {
            let t = i as f64 / rate_f;
            let phase = (2.0 * std::f64::consts::PI * s.vibrato_rate as f64 * t).sin() * depth * 2.0 * std::f64::consts::PI;
            let modulated = source[i] * (1.0 + phase.sin() * 0.01 * amount * depth_setting);
            result[i] = result[i] * 0.7 + modulated * 0.3;
        }
    }

    // 2. formant variation, noise-modulated 200 to 3000 Hz band
    let formant = s.formant_strength as f64;
    if formant > 0.01 {
        let noise = gaussian(n, seed);
        let band = Biquad::bandpass(200.0, 3000.0, rate_f).run(&source);
        for i in 0..n {
            result[i] += band[i] * noise[i] * 0.005 * amount * formant * 0.15 * amount * formant;
        }
    }

    // 3. metallic artefacts, 6 to 10 kHz pulled back
    let metallic = s.metallic_reduction as f64;
    if metallic > 0.01 && rate > 12_000 {
        let band = Biquad::bandpass(6000.0, 10_000f64.min(rate_f * 0.45), rate_f).run(&source);
        for i in 0..n {
            result[i] -= band[i] * 0.3 * amount * metallic;
        }
    }

    // 4. quantisation masking, shaped 1 to 4 kHz noise
    let masking = s.quantization_mask as f64;
    if masking > 0.01 {
        let raw: Vec<f64> = gaussian(n, seed.wrapping_add(42)).into_iter().map(|v| v * 0.002 * amount * masking).collect();
        let shaped = Biquad::bandpass(1000.0, 4000.0, rate_f).run(&raw);
        for i in 0..n {
            result[i] += shaped[i];
        }
    }

    // 5. transition smoothing, the sample-to-sample differential low-passed at 80 Hz
    let smooth = s.transition_smooth as f64;
    if smooth > 0.01 {
        let mut diff = vec![0.0; n];
        for i in 1..n {
            diff[i] = result[i] - result[i - 1];
        }
        let smoothed = Biquad::lowpass(80.0, rate_f).run(&diff);
        let blend = 0.4 * amount * smooth;
        for i in 0..n {
            result[i] = result[i] - diff[i] * blend + smoothed[i] * blend;
        }
    }

    result.into_iter().map(|v| v as f32).collect()
}

pub fn naturalize(audio: &Stereo, settings: &NaturalizeSettings) -> Stereo {
    if settings.amount < 0.01 {
        return audio.clone();
    }
    let seed = settings.seed as u64;
    let mut out = Stereo::new(
        channel(&audio.left, audio.rate, settings, seed),
        channel(&audio.right, audio.rate, settings, seed + 1),
        audio.rate,
    );
    // one gain for both channels, so the balance holds
    out.keep_below(0.95);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_result_and_amount_zero_changes_nothing() {
        let rate = 44_100;
        let left: Vec<f32> = (0..rate as usize * 2).map(|i| 0.5 * ((i as f32) * 0.07).sin()).collect();
        let audio = Stereo::new(left.clone(), left, rate);
        let a = naturalize(&audio, &NaturalizeSettings::default());
        let b = naturalize(&audio, &NaturalizeSettings::default());
        assert_eq!(a.left, b.left);
        assert!(a.peak() <= 0.95 + 1e-6);
        let off = naturalize(&audio, &NaturalizeSettings { amount: 0.0, ..NaturalizeSettings::default() });
        assert_eq!(off.left, audio.left);
    }
}
