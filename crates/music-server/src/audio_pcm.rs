//! Decoding a finished track into what a recogniser can read.
//!
//! Library audio is MP3 or 24-bit WAV at 44.1 kHz; every speech recogniser here
//! wants 16 kHz mono 16-bit PCM. Doing this in-process keeps the promise that
//! the local runtime needs no Python and no ffmpeg on the user's machine.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{bail, Context, Result};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

const TARGET_RATE: u32 = 16_000;

/// Decodes `input` to 16 kHz mono samples, which is what every recogniser here
/// expects; Parakeet reads them straight out of memory.
pub fn decode_mono_16k(input: &Path) -> Result<Vec<f32>> {
    let (samples, rate) = decode_mono(input)?;
    if samples.is_empty() {
        bail!("{} decoded to no audio", input.display());
    }
    resample(&samples, rate, TARGET_RATE)
}

/// Decodes `input`, mixes it to mono, resamples to 16 kHz and writes a WAV.
pub fn write_wav16k_mono(input: &Path, output: &Path) -> Result<()> {
    write_wav(output, &decode_mono_16k(input)?)
}

/// Decodes `input` to interleaved 44.1 kHz stereo, which is what the separation
/// model was trained on. Mono sources are doubled rather than refused: a mono
/// track still separates, it simply has the same signal in both channels.
pub fn decode_stereo_44k(input: &Path) -> Result<Vec<f32>> {
    let (channels, rate) = decode_channels(input)?;
    if channels.is_empty() || channels[0].is_empty() {
        bail!("{} decoded to no audio", input.display());
    }
    let left = resample(&channels[0], rate, 44_100)?;
    let right = match channels.get(1) {
        Some(samples) => resample(samples, rate, 44_100)?,
        None => left.clone(),
    };
    let frames = left.len().min(right.len());
    let mut interleaved = Vec::with_capacity(frames * 2);
    for frame in 0..frames {
        interleaved.push(left[frame]);
        interleaved.push(right[frame]);
    }
    Ok(interleaved)
}

/// Decodes `input` to stereo at its own sample rate, for processing that must
/// not resample: a mono source is doubled.
pub fn decode_stereo(input: &Path) -> Result<audio_post::Stereo> {
    let (mut channels, rate) = decode_channels(input)?;
    if channels.is_empty() || channels[0].is_empty() {
        bail!("{} decoded to no audio", input.display());
    }
    let left = channels.remove(0);
    let right = if channels.is_empty() { left.clone() } else { channels.remove(0) };
    Ok(audio_post::Stereo::new(left, right, rate))
}

/// Stereo as a WAV file in memory: wav16, wav24 (PCM) or wav32 (IEEE float).
pub fn wav_bytes(audio: &audio_post::Stereo, format: &str) -> Result<Vec<u8>> {
    let (bits, tag): (u16, u16) = match format {
        "wav16" => (16, 1),
        "wav24" => (24, 1),
        "wav32" => (32, 3),
        other => bail!("{other} is not a WAV format"),
    };
    let frames = audio.frames();
    let block = 2 * (bits / 8);
    let data_bytes = (frames * block as usize) as u32;
    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&audio.rate.to_le_bytes());
    out.extend_from_slice(&(audio.rate * block as u32).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for frame in 0..frames {
        for sample in [audio.left[frame], audio.right[frame]] {
            match bits {
                16 => out.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes()),
                24 => out.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32).to_le_bytes()[..3]),
                _ => out.extend_from_slice(&sample.to_le_bytes()),
            }
        }
    }
    Ok(out)
}

/// Writes stereo as 24-bit PCM WAV, the studio's lossless format.
pub fn write_wav24(path: &Path, audio: &audio_post::Stereo) -> Result<()> {
    let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut out = BufWriter::new(file);
    let frames = audio.frames();
    let data_bytes = (frames * 2 * 3) as u32;
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data_bytes).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?; // PCM
    out.write_all(&2u16.to_le_bytes())?; // stereo
    out.write_all(&audio.rate.to_le_bytes())?;
    out.write_all(&(audio.rate * 2 * 3).to_le_bytes())?; // byte rate
    out.write_all(&6u16.to_le_bytes())?; // block align
    out.write_all(&24u16.to_le_bytes())?; // bits per sample
    out.write_all(b"data")?;
    out.write_all(&data_bytes.to_le_bytes())?;
    const FULL: f32 = 8_388_607.0;
    for frame in 0..frames {
        for sample in [audio.left[frame], audio.right[frame]] {
            let value = (sample.clamp(-1.0, 1.0) * FULL).round() as i32;
            out.write_all(&value.to_le_bytes()[..3])?;
        }
    }
    out.flush().context("finish writing the WAV")?;
    Ok(())
}

/// Writes stereo as 32-bit IEEE float WAV, at its own rate and unclipped.
pub fn write_wav_f32(path: &Path, audio: &audio_post::Stereo) -> Result<()> {
    let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut out = BufWriter::new(file);
    let frames = audio.frames();
    let data_bytes = (frames * 2 * 4) as u32;
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data_bytes).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&3u16.to_le_bytes())?; // IEEE float
    out.write_all(&2u16.to_le_bytes())?; // stereo
    out.write_all(&audio.rate.to_le_bytes())?;
    out.write_all(&(audio.rate * 2 * 4).to_le_bytes())?; // byte rate
    out.write_all(&8u16.to_le_bytes())?; // block align
    out.write_all(&32u16.to_le_bytes())?; // bits per sample
    out.write_all(b"data")?;
    out.write_all(&data_bytes.to_le_bytes())?;
    for frame in 0..frames {
        out.write_all(&audio.left[frame].to_le_bytes())?;
        out.write_all(&audio.right[frame].to_le_bytes())?;
    }
    out.flush().context("finish writing the WAV")?;
    Ok(())
}

/// Every channel kept apart, at the file's own sample rate.
/// What a file's own tags say about it; empty where they say nothing.
#[derive(Debug, Clone, Default)]
pub struct FileTags {
    pub title: String,
    pub artist: String,
    /// Unsynchronised lyrics: ID3 USLT, Vorbis LYRICS.
    pub lyrics: String,
    pub genre: String,
    pub bpm: String,
    /// As written: "Am", "F# minor", or a Camelot code nobody here reads.
    pub key: String,
}

pub fn tags(path: &Path) -> FileTags {
    use symphonia::core::meta::StandardTag;
    let Ok(mut format) = open_format(path) else { return FileTags::default() };
    let mut found = FileTags::default();
    // Tags read ahead of the container (an ID3 block before MP3 frames) are
    // queued on the reader with the container's own; every revision is read.
    let mut metadata = format.metadata();
    loop {
        if let Some(revision) = metadata.current() {
            for tag in &revision.media.tags {
                let trimmed = |value: &str| value.trim().to_string();
                match &tag.std {
                    Some(StandardTag::TrackTitle(value)) if found.title.is_empty() => found.title = trimmed(value),
                    Some(StandardTag::Artist(value)) if found.artist.is_empty() => found.artist = trimmed(value),
                    Some(StandardTag::AlbumArtist(value)) if found.artist.is_empty() => found.artist = trimmed(value),
                    Some(StandardTag::Lyrics(value)) if found.lyrics.is_empty() => found.lyrics = trimmed(value),
                    Some(StandardTag::Genre(value)) if found.genre.is_empty() => found.genre = trimmed(value),
                    Some(StandardTag::Bpm(value)) if found.bpm.is_empty() => found.bpm = value.to_string(),
                    Some(StandardTag::InitialKey(value)) if found.key.is_empty() => found.key = trimmed(value),
                    None if found.key.is_empty() && matches!(tag.raw.key.to_ascii_uppercase().as_str(), "TKEY" | "INITIALKEY" | "KEY") => found.key = trimmed(&tag.raw.value.to_string()),
                    _ => {}
                }
            }
        }
        if metadata.is_latest() {
            break;
        }
        metadata.pop();
    }
    found
}

fn open_format(path: &Path) -> Result<Box<dyn FormatReader>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    probe(Box::new(file), path.extension().and_then(|value| value.to_str()))
}

fn probe(source: Box<dyn symphonia::core::io::MediaSource>, extension: Option<&str>) -> Result<Box<dyn FormatReader>> {
    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }
    symphonia::default::get_probe()
        .probe(&hint, MediaSourceStream::new(source, Default::default()), FormatOptions::default(), MetadataOptions::default())
        .context("recognise the audio format")
}

/// A file's length from its header, without decoding it.
#[cfg(test)]
pub fn duration_seconds(path: &Path) -> Option<f64> {
    let format = open_format(path).ok()?;
    let track = format.default_track(TrackType::Audio)?;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else { return None };
    Some(track.num_frames? as f64 / params.sample_rate? as f64)
}

fn decode_channels(path: &Path) -> Result<(Vec<Vec<f32>>, u32)> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let extension = path.extension().and_then(|value| value.to_str());
    decode_source(Box::new(file), extension).with_context(|| format!("decode {}", path.display()))
}

/// Decodes audio held in memory - a track as the engine sent it - to stereo
/// at its own rate; a mono source is doubled.
pub fn decode_stereo_bytes(bytes: Vec<u8>, extension: &str) -> Result<audio_post::Stereo> {
    let (mut channels, rate) = decode_source(Box::new(std::io::Cursor::new(bytes)), Some(extension))?;
    if channels.is_empty() || channels[0].is_empty() {
        bail!("the engine's audio decoded to nothing");
    }
    let left = channels.remove(0);
    let right = if channels.is_empty() { left.clone() } else { channels.remove(0) };
    Ok(audio_post::Stereo::new(left, right, rate))
}

fn decode_source(source: Box<dyn symphonia::core::io::MediaSource>, extension: Option<&str>) -> Result<(Vec<Vec<f32>>, u32)> {
    let mut format = probe(source, extension)?;
    let track = format.default_track(TrackType::Audio).context("the file carries no decodable audio track")?;
    let track_id = track.id;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        bail!("the file carries no decodable audio track");
    };
    let mut rate = params.sample_rate.unwrap_or(44_100);
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .context("no decoder for this audio")?;

    let mut planes: Vec<Vec<f32>> = Vec::new();
    let mut packet_planes: Vec<Vec<f32>> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // a file cut short still gives what it has
            Err(symphonia::core::errors::Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error).context("read audio packet"),
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(error) => return Err(error).context("decode audio packet"),
        };
        rate = decoded.spec().rate();
        decoded.copy_to_vecs_planar(&mut packet_planes);
        if planes.len() < packet_planes.len() {
            planes.resize(packet_planes.len(), Vec::new());
        }
        for (plane, samples) in planes.iter_mut().zip(&packet_planes) {
            plane.extend_from_slice(samples);
        }
    }

    Ok((planes, rate))
}

/// Every channel averaged into one, at the file's own sample rate.
fn decode_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
    let (planes, rate) = decode_channels(path)?;
    let Some(first) = planes.first() else { return Ok((Vec::new(), rate)) };
    let count = planes.len() as f32;
    let mono = (0..first.len()).map(|index| planes.iter().map(|plane| plane.get(index).copied().unwrap_or(0.0)).sum::<f32>() / count).collect();
    Ok((mono, rate))
}

/// Linear resampling. The recogniser mel-filters everything down to 80 bands
/// anyway, so a sharper filter would buy nothing here.
/// Band-limited: interpolating between samples folded everything above the
/// new Nyquist back into the band as noise, which recognisers heard as
/// distortion - MuScriptor transcribed a clean vocal as guitar and drums.
fn resample(samples: &[f32], from: u32, to: u32) -> Result<Vec<f32>> {
    if from == to || samples.len() < 2 {
        return Ok(samples.to_vec());
    }
    audio_post::resample::mono(samples, from, to)
}

fn write_wav(path: &Path, samples: &[f32]) -> Result<()> {
    let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut out = BufWriter::new(file);
    let data_bytes = (samples.len() * 2) as u32;

    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data_bytes).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?; // PCM
    out.write_all(&1u16.to_le_bytes())?; // mono
    out.write_all(&TARGET_RATE.to_le_bytes())?;
    out.write_all(&(TARGET_RATE * 2).to_le_bytes())?; // byte rate
    out.write_all(&2u16.to_le_bytes())?; // block align
    out.write_all(&16u16.to_le_bytes())?; // bits per sample
    out.write_all(b"data")?;
    out.write_all(&data_bytes.to_le_bytes())?;
    for sample in samples {
        let clamped = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.write_all(&clamped.to_le_bytes())?;
    }
    out.flush().context("finish writing the decoded WAV")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_the_band_and_drops_what_lies_above_it() {
        let tone = |hz: f32| -> Vec<f32> { (0..44_100).map(|index| (2.0 * std::f32::consts::PI * hz * index as f32 / 44_100.0).sin() * 0.5).collect() };
        let rms = |samples: &[f32]| (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt();
        let kept = resample(&tone(1_000.0), 44_100, 16_000).unwrap();
        assert!((kept.len() as i64 - 16_000).abs() <= 1, "{}", kept.len());
        assert!((rms(&kept[1000..15000]) - 0.3535).abs() < 0.02, "a tone in the band keeps its level: {}", rms(&kept));
        // 12 kHz is above 16 kHz's Nyquist: it must not fold back to 4 kHz
        let folded = resample(&tone(12_000.0), 44_100, 16_000).unwrap();
        assert!(rms(&folded[1000..15000]) < 0.01, "a tone above the band is filtered out: {}", rms(&folded));
    }

    #[test]
    fn a_written_wav_carries_a_readable_header() {
        let path = std::env::temp_dir().join(format!("pcm-{}.wav", uuid::Uuid::now_v7()));
        write_wav(&path, &[0.0, 0.5, -0.5]).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 16_000);
        assert_eq!(bytes.len(), 44 + 6);
        std::fs::remove_file(&path).ok();
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Decodes an actual generated track, when one is there to decode. The
    /// karaoke path failed with "whisper-cli produced no LRC file", and the
    /// only way that happens with a successful exit is a WAV whisper cannot
    /// read - so this is where that would show.
    #[test]
    fn decoding_a_real_track_gives_whisper_something_to_read() {
        let Some(track) = std::env::var_os("STUDIO_TEST_TRACK").map(std::path::PathBuf::from) else { return };
        let output = std::env::temp_dir().join("studio-decode-check.wav");
        write_wav16k_mono(&track, &output).expect("decode the track");
        let size = std::fs::metadata(&output).expect("the wav exists").len();
        assert!(size > 44, "the wav is nothing but a header: {size} bytes");
        eprintln!("wav: {} bytes at {}", size, output.display());
    }
}
