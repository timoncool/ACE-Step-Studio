//! The Hyrax brickwall limiter of matchering: a gain envelope from the
//! hard-clip gain, smoothed by a zero-phase attack filter and a hold/release
//! pair of first-order Butterworth low-passes.

#[derive(Debug, Clone, Copy)]
pub struct LimiterConfig {
    pub attack_ms: f64,
    pub hold_ms: f64,
    pub release_ms: f64,
    pub attack_filter_coefficient: f64,
    pub hold_filter_coefficient: f64,
    pub release_filter_coefficient: f64,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            attack_ms: 1.0,
            hold_ms: 1.0,
            release_ms: 3000.0,
            attack_filter_coefficient: -2.0,
            hold_filter_coefficient: 7.0,
            release_filter_coefficient: 800.0,
        }
    }
}

fn ms_to_samples(ms: f64, rate: u32) -> usize {
    (rate as f64 * ms * 1e-3) as usize
}

/// Maximum over a centred window of `size` (odd), edges clipped.
fn centred_max(x: &[f64], size: usize) -> Vec<f64> {
    let half = size / 2;
    let n = x.len();
    let mut out = vec![0.0; n];
    let mut deque: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    let mut next = 0;
    for i in 0..n {
        let hi = (i + half).min(n - 1);
        while next <= hi {
            while deque.back().is_some_and(|&b| x[b] <= x[next]) {
                deque.pop_back();
            }
            deque.push_back(next);
            next += 1;
        }
        let lo = i.saturating_sub(half);
        while deque.front().is_some_and(|&f| f < lo) {
            deque.pop_front();
        }
        out[i] = x[*deque.front().unwrap()];
    }
    out
}

/// Maximum over the `size` samples ending at each one.
fn trailing_max(x: &[f64], size: usize) -> Vec<f64> {
    let mut out = vec![0.0; x.len()];
    let mut deque: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for i in 0..x.len() {
        while deque.back().is_some_and(|&b| x[b] <= x[i]) {
            deque.pop_back();
        }
        deque.push_back(i);
        while deque.front().is_some_and(|&f| f + size <= i) {
            deque.pop_front();
        }
        // the window reaches before the start, where matchering pads with zeros
        out[i] = x[*deque.front().unwrap()].max(0.0);
    }
    out
}

/// First-order Butterworth low-pass, bilinear with pre-warping (SciPy `butter(1, fc, fs=rate)`).
fn butter1(cutoff: f64, rate: u32) -> ([f64; 2], [f64; 2]) {
    let k = (std::f64::consts::PI * cutoff / rate as f64).tan();
    let b = k / (1.0 + k);
    ([b, b], [1.0, (k - 1.0) / (k + 1.0)])
}

/// `lfilter` for a first-order section, from rest.
fn lfilter1(b: [f64; 2], a: [f64; 2], x: &[f64]) -> Vec<f64> {
    let mut y = Vec::with_capacity(x.len());
    let (mut x1, mut y1) = (0.0, 0.0);
    for &xi in x {
        let yi = b[0] * xi + b[1] * x1 - a[1] * y1;
        y.push(yi);
        x1 = xi;
        y1 = yi;
    }
    y
}

/// SciPy `filtfilt(b=[1-c], a=[1,-c], x)`: odd extension of 6 samples at both
/// ends, steady-state initial conditions, forward then backward.
fn filtfilt_one_pole(c: f64, x: &[f64]) -> Vec<f64> {
    let n = x.len();
    let pad = 6.min(n.saturating_sub(1));
    let mut ext = Vec::with_capacity(n + 2 * pad);
    for i in (1..=pad).rev() {
        ext.push(2.0 * x[0] - x[i]);
    }
    ext.extend_from_slice(x);
    for i in 1..=pad {
        ext.push(2.0 * x[n - 1] - x[n - 1 - i]);
    }
    let b0 = 1.0 - c;
    let run = |input: &[f64]| -> Vec<f64> {
        // lfilter_zi of this section is c: a unit step settles at 1 from sample one
        let mut state = c * input[0];
        input
            .iter()
            .map(|&xi| {
                let yi = b0 * xi + state;
                state = c * yi;
                yi
            })
            .collect()
    };
    let forward = run(&ext);
    let reversed: Vec<f64> = forward.into_iter().rev().collect();
    let backward = run(&reversed);
    let mut y: Vec<f64> = backward.into_iter().rev().collect();
    y.drain(..pad);
    y.truncate(n);
    y
}

/// Limits stereo audio in place to `threshold`.
pub fn limit(left: &mut [f32], right: &mut [f32], rate: u32, threshold: f64, config: &LimiterConfig) {
    let n = left.len().min(right.len());
    if n == 0 {
        return;
    }
    let rectified: Vec<f64> = (0..n)
        .map(|i| {
            let peak = (left[i].abs().max(right[i].abs())) as f64;
            peak.max(threshold) / threshold
        })
        .collect();
    if rectified.iter().all(|&r| (r - 1.0).abs() <= 1e-8 + 1e-5) {
        return;
    }
    let hard: Vec<f64> = rectified.iter().map(|&r| 1.0 - 1.0 / r).collect();

    let attack = ms_to_samples(config.attack_ms, rate).max(1);
    let odd = if attack % 2 == 0 { attack + 1 } else { attack };
    let slided = centred_max(&hard, 2 * odd - 1);
    let c = (config.attack_filter_coefficient / attack as f64).exp();
    let gain_attack = filtfilt_one_pole(c, &slided);

    let hold = ms_to_samples(config.hold_ms, rate).max(1);
    let held = trailing_max(&slided, hold);
    let (b, a) = butter1(config.hold_filter_coefficient, rate);
    let hold_out = lfilter1(b, a, &held);
    let (b, a) = butter1(config.release_filter_coefficient / config.release_ms, rate);
    let release_in: Vec<f64> = held.iter().zip(&hold_out).map(|(&h, &o)| h.max(o)).collect();
    let release_out = lfilter1(b, a, &release_in);

    for i in 0..n {
        let g = hard[i].max(gain_attack[i]).max(hold_out[i].max(release_out[i]));
        let gain = (1.0 - g) as f32;
        left[i] *= gain;
        right[i] *= gain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_changes_below_the_threshold() {
        let mut l = vec![0.5f32; 1000];
        let mut r = vec![-0.4f32; 1000];
        limit(&mut l, &mut r, 48_000, 0.998, &LimiterConfig::default());
        assert!(l.iter().all(|&v| v == 0.5));
    }

    #[test]
    fn peaks_come_down_to_the_threshold() {
        let n = 48_000;
        let mut l: Vec<f32> = (0..n).map(|i| 1.6 * ((i as f32) * 0.05).sin()).collect();
        let mut r = l.clone();
        limit(&mut l, &mut r, 48_000, 0.998, &LimiterConfig::default());
        let peak = l.iter().fold(0.0f32, |p, v| p.max(v.abs()));
        assert!(peak <= 0.999, "peak {peak}");
        assert!(peak > 0.8, "limited too hard: {peak}");
    }

    #[test]
    fn filtfilt_keeps_a_constant() {
        let x = vec![0.25; 300];
        let y = filtfilt_one_pole(0.96, &x);
        assert!(y.iter().all(|v| (v - 0.25).abs() < 1e-9));
    }
}
