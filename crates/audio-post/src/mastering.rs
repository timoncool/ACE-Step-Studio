//! Mastering to a reference track, the matchering 2 algorithm.
//!
//! The target is brought to the reference's loudness and tonal balance: the
//! loudest pieces of both, in mid and side, set an RMS gain and a smoothed
//! spectral correction applied as a FIR filter; four passes of RMS correction
//! follow, then the Hyrax brickwall limiter. The target keeps its own sample
//! rate; the reference is resampled to it.

use anyhow::{bail, Result};
use realfft::RealFftPlanner;

use crate::limiter::{self, LimiterConfig};
use crate::{convolve, lowess, spline::CubicSpline, Stereo};

#[derive(Debug, Clone)]
pub struct MasteringConfig {
    pub max_piece_seconds: f64,
    pub threshold: f64,
    pub min_value: f64,
    pub fft_size: usize,
    pub lin_log_oversampling: usize,
    pub rms_correction_steps: usize,
    pub lowess_frac: f64,
    pub lowess_delta: f64,
    pub limiter: LimiterConfig,
}

impl Default for MasteringConfig {
    fn default() -> Self {
        Self {
            max_piece_seconds: 15.0,
            threshold: (32768.0 - 61.0) / 32768.0,
            min_value: 1e-6,
            fft_size: 4096,
            lin_log_oversampling: 4,
            rms_correction_steps: 4,
            lowess_frac: 0.0375,
            lowess_delta: 0.001,
            limiter: LimiterConfig::default(),
        }
    }
}

fn rms(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    (values.iter().map(|v| v * v).sum::<f64>() / values.len() as f64).sqrt()
}

fn rms32(values: &[f32]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    (values.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / values.len() as f64).sqrt()
}

/// Mid, side, and the pieces the level analysis is made of.
struct Levels {
    mid: Vec<f32>,
    side: Vec<f32>,
    loud_mid: Vec<Vec<f32>>,
    loud_side: Vec<Vec<f32>>,
    match_rms: f64,
    divisions: usize,
    piece: usize,
}

/// Piece RMS values of `array` over `divisions` pieces of `piece` samples,
/// their RMS, and the RMS of the pieces at or above it.
fn piece_levels(array: &[f32], piece: usize, divisions: usize) -> (Vec<f64>, Vec<usize>, f64) {
    let rmses: Vec<f64> = (0..divisions).map(|d| rms32(&array[d * piece..(d + 1) * piece])).collect();
    let average = rms(&rmses);
    let loud: Vec<usize> = (0..divisions).filter(|&d| rmses[d] >= average).collect();
    let loud_rmses: Vec<f64> = loud.iter().map(|&d| rmses[d]).collect();
    let match_rms = rms(&loud_rmses);
    (rmses, loud, match_rms)
}

fn analyze(audio: &Stereo, config: &MasteringConfig) -> Levels {
    let n = audio.frames();
    let mid: Vec<f32> = (0..n).map(|i| (audio.left[i] + audio.right[i]) * 0.5).collect();
    let side: Vec<f32> = (0..n).map(|i| (audio.left[i] - audio.right[i]) * 0.5).collect();
    let max_piece = (config.max_piece_seconds * audio.rate as f64) as usize;
    let divisions = n / max_piece + 1;
    let piece = n / divisions;
    let (_, loud, match_rms) = piece_levels(&mid, piece, divisions);
    let loud_mid = loud.iter().map(|&d| mid[d * piece..(d + 1) * piece].to_vec()).collect();
    let loud_side = loud.iter().map(|&d| side[d * piece..(d + 1) * piece].to_vec()).collect();
    Levels { mid, side, loud_mid, loud_side, match_rms, divisions, piece }
}

/// Mean magnitude spectrum over every non-overlapping boxcar frame of every
/// piece, scaled like SciPy's STFT (by the window sum).
fn average_spectrum(pieces: &[Vec<f32>], fft_size: usize) -> Vec<f64> {
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(fft_size);
    let bins = fft_size / 2 + 1;
    let mut sum = vec![0.0; bins];
    let mut frames = 0usize;
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    for piece in pieces {
        let count = if piece.len() >= fft_size { (piece.len() - fft_size) / fft_size + 1 } else { 0 };
        for f in 0..count {
            for (slot, &v) in input.iter_mut().zip(&piece[f * fft_size..(f + 1) * fft_size]) {
                *slot = v as f64;
            }
            fft.process(&mut input, &mut output).expect("spectrum");
            for (s, c) in sum.iter_mut().zip(&output) {
                *s += c.norm() / fft_size as f64;
            }
            frames += 1;
        }
    }
    if frames > 0 {
        sum.iter_mut().for_each(|s| *s /= frames as f64);
    }
    sum
}

/// The correction FIR from the target's to the reference's spectrum, smoothed
/// on a logarithmic frequency grid.
fn correction_fir(target: &[Vec<f32>], reference: &[Vec<f32>], rate: u32, config: &MasteringConfig) -> Vec<f64> {
    let fft_size = config.fft_size;
    let bins = fft_size / 2 + 1;
    let target_spectrum = average_spectrum(target, fft_size);
    let reference_spectrum = average_spectrum(reference, fft_size);
    let matching: Vec<f64> = (0..bins).map(|b| reference_spectrum[b] / target_spectrum[b].max(config.min_value)).collect();

    let nyquist = rate as f64 * 0.5;
    let linear: Vec<f64> = (0..bins).map(|b| nyquist * b as f64 / (bins - 1) as f64).collect();
    let log_points = (fft_size / 2) * config.lin_log_oversampling + 1;
    let start = (4.0 / fft_size as f64).log10();
    let logarithmic: Vec<f64> =
        (0..log_points).map(|i| nyquist * 10f64.powf(start + (0.0 - start) * i as f64 / (log_points - 1) as f64)).collect();

    let to_log = CubicSpline::new(&linear, &matching);
    let on_log: Vec<f64> = logarithmic.iter().map(|&f| to_log.at(f)).collect();
    let unit: Vec<f64> = (0..log_points).map(|i| i as f64 / (log_points - 1) as f64).collect();
    let smoothed = lowess::smooth(&unit, &on_log, config.lowess_frac, config.lowess_delta);
    let back = CubicSpline::new(&logarithmic, &smoothed);
    let mut filtered: Vec<f64> = linear.iter().map(|&f| back.at(f)).collect();
    filtered[0] = 0.0;
    filtered[1] = matching[1];

    // real inverse FFT of the zero-phase response, centred, Hann windowed
    let mut planner = RealFftPlanner::<f64>::new();
    let inverse = planner.plan_fft_inverse(fft_size);
    let mut spectrum: Vec<realfft::num_complex::Complex<f64>> =
        filtered.iter().map(|&v| realfft::num_complex::Complex::new(v, 0.0)).collect();
    let mut time = inverse.make_output_vec();
    inverse.process(&mut spectrum, &mut time).expect("fir inverse");
    let scale = 1.0 / fft_size as f64;
    let half = fft_size / 2;
    (0..fft_size)
        .map(|i| {
            let shifted = time[(i + half) % fft_size] * scale;
            let window = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (fft_size - 1) as f64).cos();
            shifted * window
        })
        .collect()
}

/// Masters `target` to sound like `reference`.
pub fn master(target: &Stereo, reference: &Stereo, config: &MasteringConfig) -> Result<Stereo> {
    let rate = target.rate;
    let mut reference = reference.resampled(rate)?;
    let min_len = config.fft_size * 2;
    if target.frames() < min_len || reference.frames() < min_len {
        bail!("both tracks need at least {:.2} s of audio", min_len as f64 / rate as f64);
    }

    // the reference, normalised when it peaks below the threshold; the target
    // follows the same amplitude change at the end
    let peak = reference.peak() as f64;
    let mut final_gain = 1.0;
    if peak < config.threshold {
        let coefficient = (peak / config.threshold).max(config.min_value);
        let inverse = (1.0 / coefficient) as f32;
        reference.left.iter_mut().chain(reference.right.iter_mut()).for_each(|s| *s *= inverse);
        final_gain = coefficient;
    }

    let mut target_levels = analyze(target, config);
    let reference_levels = analyze(&reference, config);
    if target_levels.match_rms <= 0.0 {
        bail!("the track is silent");
    }

    // match levels
    let gain = (reference_levels.match_rms / target_levels.match_rms.max(config.min_value)) as f32;
    let amplify = |v: &mut Vec<f32>, g: f32| v.iter_mut().for_each(|s| *s *= g);
    amplify(&mut target_levels.mid, gain);
    amplify(&mut target_levels.side, gain);
    target_levels.loud_mid.iter_mut().for_each(|p| amplify(p, gain));
    target_levels.loud_side.iter_mut().for_each(|p| amplify(p, gain));

    // match frequencies
    let mid_fir = correction_fir(&target_levels.loud_mid, &reference_levels.loud_mid, rate, config);
    let side_fir = correction_fir(&target_levels.loud_side, &reference_levels.loud_side, rate, config);
    let mut mid = convolve::same(&target_levels.mid, &mid_fir);
    let side = convolve::same(&target_levels.side, &side_fir);
    let mut left: Vec<f32> = mid.iter().zip(&side).map(|(m, s)| m + s).collect();
    let mut right: Vec<f32> = mid.iter().zip(&side).map(|(m, s)| m - s).collect();

    // correct levels against the clipped mid
    let (piece, divisions) = (target_levels.piece, target_levels.divisions);
    for _ in 0..config.rms_correction_steps {
        let clipped: Vec<f32> = mid.iter().map(|v| v.clamp(-1.0, 1.0)).collect();
        let (_, _, clipped_match) = piece_levels(&clipped, piece, divisions);
        let correction = (reference_levels.match_rms / clipped_match.max(config.min_value)) as f32;
        amplify(&mut mid, correction);
        amplify(&mut left, correction);
        amplify(&mut right, correction);
    }

    limiter::limit(&mut left, &mut right, rate, config.threshold, &config.limiter);
    let final_gain = final_gain as f32;
    amplify(&mut left, final_gain);
    amplify(&mut right, final_gain);
    Ok(Stereo::new(left, right, rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frequency: f32, amplitude: f32, seconds: f32, rate: u32) -> Stereo {
        let n = (seconds * rate as f32) as usize;
        let left: Vec<f32> = (0..n)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * frequency * i as f32 / rate as f32).sin())
            .collect();
        Stereo::new(left.clone(), left, rate)
    }

    #[test]
    fn a_quiet_track_is_brought_to_the_reference_loudness() {
        let target = tone(440.0, 0.05, 20.0, 48_000);
        let reference = tone(440.0, 0.5, 20.0, 44_100);
        let out = master(&target, &reference, &MasteringConfig::default()).unwrap();
        let level = rms32(&out.left[48_000..out.frames() - 48_000]);
        let wanted = 0.5 / 2f64.sqrt();
        assert!((level / wanted - 1.0).abs() < 0.1, "rms {level} vs {wanted}");
        assert!(out.peak() <= 1.0);
    }
}
