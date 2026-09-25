//! Processing for finished tracks: mastering to a reference track, noise
//! reduction, the Spectral Lifter chain, vocal naturalising, and a quality
//! check. It works on decoded stereo, whatever engine made the song, so every
//! studio built on this workspace gets the same tools.
//!
//! Every stage is plain DSP on the processor, deterministic for the same input
//! and settings.

pub mod denoise;
pub mod encode;
pub mod lifter;
pub mod mastering;
pub mod naturalize;
pub mod quality;

mod convolve;
mod limiter;
mod lowess;
pub mod resample;
mod spline;
mod stft;

/// Decoded stereo audio.
#[derive(Debug, Clone)]
pub struct Stereo {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub rate: u32,
}

impl Stereo {
    pub fn new(left: Vec<f32>, right: Vec<f32>, rate: u32) -> Self {
        let frames = left.len().min(right.len());
        let (mut left, mut right) = (left, right);
        left.truncate(frames);
        right.truncate(frames);
        Self { left, right, rate }
    }

    pub fn frames(&self) -> usize {
        self.left.len()
    }

    pub fn peak(&self) -> f32 {
        self.left.iter().chain(&self.right).fold(0.0f32, |peak, sample| peak.max(sample.abs()))
    }

    /// The same audio at another sample rate.
    pub fn resampled(&self, rate: u32) -> anyhow::Result<Stereo> {
        if rate == self.rate {
            return Ok(self.clone());
        }
        let (left, right) = resample::stereo(&self.left, &self.right, self.rate, rate)?;
        Ok(Stereo::new(left, right, rate))
    }

    /// Scales the whole track down when it would clip.
    pub fn keep_below(&mut self, ceiling: f32) {
        let peak = self.peak();
        if peak > ceiling {
            let gain = ceiling / peak;
            self.left.iter_mut().chain(self.right.iter_mut()).for_each(|sample| *sample *= gain);
        }
    }
}
