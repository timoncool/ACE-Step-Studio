//! A technical check of a generated track for three failures generation is
//! prone to: a metallic top end (spectral roll-off), cut or glitched words
//! (spectral flux spikes), and hiss (zero-crossing rate). From the
//! AudioQualityEvaluator of JK-AceStep-Nodes with HOT-Step's continuous
//! scoring curves. It measures artefacts, not taste.

use realfft::RealFftPlanner;
use serde::Serialize;

use crate::Stereo;

#[derive(Debug, Clone, Serialize)]
pub struct QualityReport {
    /// Weighted total, 0 to 1.
    pub score: f32,
    pub metallic: f32,
    pub word_cuts: f32,
    pub noise: f32,
    pub rolloff_hz: f32,
    pub severe_cuts: usize,
    pub moderate_cuts: usize,
    /// Zero-crossing rate expressed at 48 kHz, whatever the track's rate.
    pub zcr: f32,
}

fn sigmoid(x: f64, centre: f64, k: f64) -> f64 {
    1.0 / (1.0 + (k * (x - centre)).exp())
}

pub fn evaluate(audio: &Stereo) -> QualityReport {
    let mono: Vec<f64> = audio.left.iter().zip(&audio.right).map(|(&l, &r)| (l as f64 + r as f64) * 0.5).collect();
    const N: usize = 2048;
    const HOP: usize = 512;
    let rate = audio.rate as f64;

    let mut frames: Vec<Vec<f64>> = Vec::new();
    if mono.len() >= N {
        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(N);
        let window: Vec<f64> =
            (0..N).map(|i| 0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / (N - 1) as f64).cos())).collect();
        let mut input = fft.make_input_vec();
        let mut output = fft.make_output_vec();
        for start in (0..=mono.len() - N).step_by(HOP) {
            for i in 0..N {
                input[i] = mono[start + i] * window[i];
            }
            fft.process(&mut input, &mut output).expect("quality frame");
            frames.push(output.iter().map(|c| c.norm()).collect());
        }
    }

    // 1. metallic: mean frequency under which 85 % of each frame's energy lies
    let (metallic, rolloff) = if frames.is_empty() {
        (0.5, 0.0)
    } else {
        let bins = frames[0].len();
        let mut sum = 0.0;
        for frame in &frames {
            let total: f64 = frame.iter().map(|m| m * m).sum();
            let target = total * 0.85;
            let mut cumulative = 0.0;
            let mut bin = bins - 1;
            for (b, m) in frame.iter().enumerate() {
                cumulative += m * m;
                if cumulative >= target {
                    bin = b;
                    break;
                }
            }
            sum += bin as f64 * rate / N as f64;
        }
        let mean = sum / frames.len() as f64;
        (sigmoid(mean, 3500.0, 0.0015), mean)
    };

    // 2. word cuts: frame-to-frame spectral change far above its usual size
    let (word_cuts, severe, moderate) = if frames.len() < 2 {
        (0.5, 0, 0)
    } else {
        let flux: Vec<f64> = frames
            .windows(2)
            .map(|pair| pair[0].iter().zip(&pair[1]).map(|(a, b)| (b - a).powi(2)).sum::<f64>().sqrt())
            .collect();
        let mean = flux.iter().sum::<f64>() / flux.len() as f64;
        let std = (flux.iter().map(|f| (f - mean).powi(2)).sum::<f64>() / flux.len() as f64).sqrt();
        if std < 1e-6 {
            (0.0, 0, 0)
        } else {
            let (mut severe, mut moderate) = (0usize, 0usize);
            for f in &flux {
                let z = (f - mean) / std;
                if z > 4.0 {
                    severe += 1;
                } else if z > 3.0 {
                    moderate += 1;
                }
            }
            let severe_pct = severe as f64 / flux.len() as f64 * 100.0;
            let moderate_pct = moderate as f64 / flux.len() as f64 * 100.0;
            (sigmoid(severe_pct, 0.04, 100.0) * 0.6 + sigmoid(moderate_pct, 0.5, 6.0) * 0.4, severe, moderate)
        }
    };

    // 3. noise: zero-crossing rate, scaled to 48 kHz where the curve was fitted
    let (noise, zcr) = if mono.len() < 2 {
        (0.5, 0.0)
    } else {
        let crossings = mono.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count();
        let zcr = crossings as f64 / (mono.len() - 1) as f64 * rate / 48_000.0;
        ((-(zcr - 0.065).powi(2) / (2.0 * 0.02f64.powi(2))).exp(), zcr)
    };

    let score = metallic * 0.4 + word_cuts * 0.4 + noise * 0.2;
    QualityReport {
        score: score as f32,
        metallic: metallic as f32,
        word_cuts: word_cuts as f32,
        noise: noise as f32,
        rolloff_hz: rolloff as f32,
        severe_cuts: severe,
        moderate_cuts: moderate,
        zcr: zcr as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_low_tone_scores_better_than_white_noise() {
        let rate = 48_000;
        let n = rate as usize * 3;
        let tone: Vec<f32> = (0..n).map(|i| 0.5 * (2.0 * std::f32::consts::PI * 200.0 * i as f32 / rate as f32).sin()).collect();
        let mut seed = 7u32;
        let noise: Vec<f32> = (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
            })
            .collect();
        let tone = evaluate(&Stereo::new(tone.clone(), tone, rate));
        let hiss = evaluate(&Stereo::new(noise.clone(), noise, rate));
        assert!(tone.metallic > hiss.metallic);
        assert!(tone.noise > hiss.noise);
    }
}
