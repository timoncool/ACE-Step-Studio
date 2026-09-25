//! Turning a finished track into the file the library keeps.
//!
//! Engines hand over their audio unencoded, at the rate and precision the model
//! produced. The level is set here, and MP3 is made by LAME, the reference MP3
//! encoder: one encoder for every engine, never one engine's own.

use anyhow::{anyhow, bail, Result};
use mp3lame_encoder::{Bitrate, Builder, DualPcm, FlushNoGap, Quality};

use crate::Stereo;

/// Scales the track so its loudest part reaches full scale: the level that
/// all but `peak_clip` samples per million stay under becomes 1.0, and those
/// few are clipped. The rule the engines apply to their own encoded output.
pub fn normalize_peak(audio: &mut Stereo, peak_clip: u32) {
    let peak_clip = peak_clip.min(999) as f64;
    let mut magnitudes: Vec<f32> = audio.left.iter().chain(&audio.right).map(|sample| sample.abs()).collect();
    if magnitudes.is_empty() {
        return;
    }
    let index = ((magnitudes.len() - 1) as f64 * (1.0 - peak_clip / 1_000_000.0)) as usize;
    let (_, reference, _) = magnitudes.select_nth_unstable_by(index, |a, b| a.total_cmp(b));
    let reference = *reference;
    if reference < 1e-6 {
        return;
    }
    let gain = 1.0 / reference;
    for sample in audio.left.iter_mut().chain(audio.right.iter_mut()) {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

/// The MPEG-1 Layer III rates LAME encodes at, the highest first.
const BITRATES: &[(u32, Bitrate)] = &[
    (320, Bitrate::Kbps320),
    (256, Bitrate::Kbps256),
    (224, Bitrate::Kbps224),
    (192, Bitrate::Kbps192),
    (160, Bitrate::Kbps160),
    (128, Bitrate::Kbps128),
    (112, Bitrate::Kbps112),
    (96, Bitrate::Kbps96),
    (80, Bitrate::Kbps80),
    (64, Bitrate::Kbps64),
    (48, Bitrate::Kbps48),
    (40, Bitrate::Kbps40),
    (32, Bitrate::Kbps32),
];

/// The standard rate a requested bitrate becomes: the highest one not above it.
/// Linear fades at both ends, in seconds; zero leaves that end as it is.
pub fn fade(audio: &mut Stereo, fade_in: f32, fade_out: f32) {
    let frames = audio.frames();
    let rate = audio.rate as f32;
    let fade_in = ((fade_in.max(0.0) * rate) as usize).min(frames);
    let fade_out = ((fade_out.max(0.0) * rate) as usize).min(frames);
    for i in 0..fade_in {
        let gain = i as f32 / fade_in as f32;
        audio.left[i] *= gain;
        audio.right[i] *= gain;
    }
    for i in 0..fade_out {
        let gain = i as f32 / fade_out as f32;
        let at = frames - 1 - i;
        audio.left[at] *= gain;
        audio.right[at] *= gain;
    }
}

pub fn mp3_bitrate(kbps: u32) -> u32 {
    BITRATES.iter().find(|(rate, _)| *rate <= kbps).map_or(32, |(rate, _)| *rate)
}

/// Encodes stereo to constant-bitrate MP3 at the track's own sample rate,
/// which must be one MPEG-1 carries (32, 44.1 or 48 kHz): nothing is
/// resampled on the way.
pub fn mp3(audio: &Stereo, kbps: u32) -> Result<Vec<u8>> {
    if !matches!(audio.rate, 32_000 | 44_100 | 48_000) {
        bail!("MP3 carries 32, 44.1 or 48 kHz; this track is {} Hz", audio.rate);
    }
    let bitrate = BITRATES.iter().find(|(rate, _)| *rate == mp3_bitrate(kbps)).map(|(_, bitrate)| *bitrate).expect("listed");
    let mut builder = Builder::new().ok_or_else(|| anyhow!("LAME could not start"))?;
    builder.set_num_channels(2).map_err(|error| anyhow!("LAME channels: {error}"))?;
    builder.set_sample_rate(audio.rate).map_err(|error| anyhow!("LAME sample rate: {error}"))?;
    builder.set_brate(bitrate).map_err(|error| anyhow!("LAME bitrate: {error}"))?;
    builder.set_quality(Quality::Best).map_err(|error| anyhow!("LAME quality: {error}"))?;
    builder.set_to_write_vbr_tag(false).map_err(|error| anyhow!("LAME tag: {error}"))?;
    let mut encoder = builder.build().map_err(|error| anyhow!("LAME: {error}"))?;

    const PIECE: usize = 1 << 16;
    let mut out = Vec::with_capacity(audio.frames() * kbps as usize / 8 / audio.rate as usize * 1000 + 8192);
    for start in (0..audio.frames()).step_by(PIECE) {
        let end = (start + PIECE).min(audio.frames());
        out.reserve(mp3lame_encoder::max_required_buffer_size(end - start));
        encoder
            .encode_to_vec(DualPcm { left: &audio.left[start..end], right: &audio.right[start..end] }, &mut out)
            .map_err(|error| anyhow!("LAME encode: {error}"))?;
    }
    out.reserve(8192);
    encoder.flush_to_vec::<FlushNoGap>(&mut out).map_err(|error| anyhow!("LAME flush: {error}"))?;
    Ok(out)
}

/// Encodes stereo to lossless 24-bit FLAC at the track's own sample rate.
pub fn flac(audio: &Stereo) -> Result<Vec<u8>> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

    const BITS: usize = 24;
    let full_scale = ((1i32 << (BITS - 1)) - 1) as f32;
    let mut samples = Vec::with_capacity(audio.frames() * 2);
    for frame in 0..audio.frames() {
        for sample in [audio.left[frame], audio.right[frame]] {
            samples.push((sample.clamp(-1.0, 1.0) * full_scale).round() as i32);
        }
    }
    let config = flacenc::config::Encoder::default().into_verified().map_err(|(_, error)| anyhow!("FLAC settings: {error:?}"))?;
    let source = flacenc::source::MemSource::from_samples(&samples, 2, BITS, audio.rate as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size).map_err(|error| anyhow!("FLAC encode: {error:?}"))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|error| anyhow!("FLAC write: {error:?}"))?;
    Ok(sink.as_slice().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f32, rate: u32, amplitude: f32) -> Stereo {
        let n = (seconds * rate as f32) as usize;
        let left: Vec<f32> = (0..n).map(|i| amplitude * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin()).collect();
        Stereo::new(left.clone(), left, rate)
    }

    #[test]
    fn normalizing_brings_the_peak_to_full_scale() {
        let mut audio = tone(1.0, 48_000, 0.25);
        normalize_peak(&mut audio, 10);
        let peak = audio.left.iter().fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        assert!((peak - 1.0).abs() < 1e-3, "peak {peak}");
    }

    #[test]
    fn bitrates_round_down_to_a_standard_one() {
        assert_eq!(mp3_bitrate(320), 320);
        assert_eq!(mp3_bitrate(300), 256);
        assert_eq!(mp3_bitrate(128), 128);
        assert_eq!(mp3_bitrate(10), 32);
    }

    #[test]
    fn lame_writes_a_stream_of_the_asked_size() {
        let audio = tone(3.0, 48_000, 0.5);
        let bytes = mp3(&audio, 320).unwrap();
        // 320 kbps for three seconds, give or take the padding frames
        let expected = 320_000 / 8 * 3;
        assert!(bytes.len() > expected * 9 / 10 && bytes.len() < expected * 12 / 10, "{} bytes", bytes.len());
        assert_eq!(&bytes[..2], &[0xFF, 0xFB], "an MPEG-1 Layer III frame header");
        assert!(mp3(&tone(1.0, 22_050, 0.5), 128).is_err());
    }

    #[test]
    fn flac_is_a_flac_stream() {
        let bytes = flac(&tone(1.0, 48_000, 0.5)).unwrap();
        assert_eq!(&bytes[..4], b"fLaC");
        assert!(bytes.len() < 48_000 * 2 * 3);
    }
}
