//! Short-time Fourier analysis and synthesis for whole tracks.
//!
//! The signal is padded by one window on both sides before framing, so every
//! sample, the first and the last included, sits under fully overlapped
//! windows and comes back from synthesis unchanged when nothing is changed.

use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

pub struct Stft {
    pub size: usize,
    pub hop: usize,
    window: Vec<f32>,
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
}

/// Frames of one channel: `frames[f][bin]`.
pub struct Spectrogram {
    pub frames: Vec<Vec<Complex32>>,
    len: usize,
}

impl Spectrogram {
    pub fn bins(&self) -> usize {
        self.frames.first().map_or(0, Vec::len)
    }
}

impl Stft {
    pub fn new(size: usize, hop: usize) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let window = (0..size)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / size as f32).cos())
            .collect();
        Self { size, hop, window, forward: planner.plan_fft_forward(size), inverse: planner.plan_fft_inverse(size) }
    }

    pub fn analyze(&self, x: &[f32]) -> Spectrogram {
        let n = self.size;
        let padded_len = x.len() + 2 * n;
        let count = (padded_len - n) / self.hop + 1;
        let mut frames = Vec::with_capacity(count);
        let mut input = self.forward.make_input_vec();
        for f in 0..count {
            let start = f * self.hop;
            for (i, slot) in input.iter_mut().enumerate() {
                let p = start + i;
                let v = if p >= n && p - n < x.len() { x[p - n] } else { 0.0 };
                *slot = v * self.window[i];
            }
            let mut output = self.forward.make_output_vec();
            self.forward.process(&mut input, &mut output).expect("stft frame");
            frames.push(output);
        }
        Spectrogram { frames, len: x.len() }
    }

    pub fn synthesize(&self, spectrogram: &Spectrogram) -> Vec<f32> {
        let n = self.size;
        let padded_len = spectrogram.len + 2 * n;
        let mut out = vec![0.0f32; padded_len + n];
        let mut weight = vec![0.0f32; padded_len + n];
        let mut time = self.inverse.make_output_vec();
        let scale = 1.0 / n as f32;
        for (f, frame) in spectrogram.frames.iter().enumerate() {
            let mut spectrum = frame.clone();
            // the DC and Nyquist bins of a real signal carry no imaginary part
            spectrum[0].im = 0.0;
            if let Some(last) = spectrum.last_mut() {
                last.im = 0.0;
            }
            self.inverse.process(&mut spectrum, &mut time).expect("istft frame");
            let start = f * self.hop;
            for i in 0..n {
                out[start + i] += time[i] * scale * self.window[i];
                weight[start + i] += self.window[i] * self.window[i];
            }
        }
        (n..n + spectrogram.len).map(|p| if weight[p] > 1e-8 { out[p] / weight[p] } else { 0.0 }).collect()
    }
}

/// Magnitudes of the mid signal, `(L + R) / 2`, from the two channel spectrograms.
pub fn mid_magnitudes(left: &Spectrogram, right: &Spectrogram) -> Vec<Vec<f32>> {
    left.frames
        .iter()
        .zip(&right.frames)
        .map(|(l, r)| l.iter().zip(r).map(|(a, b)| ((a + b) * 0.5).norm()).collect())
        .collect()
}

/// Multiplies both channels by one gain per frame and bin, so the stereo image holds.
pub fn apply_mask(left: &mut Spectrogram, right: &mut Spectrogram, mask: &[Vec<f32>]) {
    for ((l, r), m) in left.frames.iter_mut().zip(right.frames.iter_mut()).zip(mask) {
        for ((a, b), g) in l.iter_mut().zip(r.iter_mut()).zip(m) {
            *a *= *g;
            *b *= *g;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_then_synthesis_returns_the_signal_edges_included() {
        let x: Vec<f32> = (0..10_000).map(|i| ((i as f32) * 0.031).sin() * 0.5 + ((i % 17) as f32 - 8.0) * 0.01).collect();
        for (size, hop) in [(2048, 512), (8192, 4096)] {
            let stft = Stft::new(size, hop);
            let back = stft.synthesize(&stft.analyze(&x));
            assert_eq!(back.len(), x.len());
            let worst = back.iter().zip(&x).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            assert!(worst < 1e-4, "size {size}: {worst}");
        }
    }
}
