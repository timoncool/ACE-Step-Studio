//! Restoring music that was compressed to MP3: Apollo on audio.cpp's server (family `apollo`, task `s2s`,
//! docs/models/apollo.md). The model takes and gives 44.1 kHz and keeps the channels of what it is given; the server
//! cuts a long recording into windows and joins them itself.

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::downloads::{Asset, AssetKind};

/// Apollo's weights, audio.cpp's GGUF of the official checkpoint, F32 as it is published and pinned to the revision
/// it was measured at.
pub const MODEL: Asset = Asset {
    id: "apollo",
    label: "Apollo",
    kind: AssetKind::Model,
    url: "https://huggingface.co/audio-cpp/audio.cpp-gguf/resolve/73ce4162f6cce370dd533c6782b772135c1b2a8e/Apollo-GGUF/apollo-orig.gguf",
    relative_path: "models/apollo/apollo-orig.gguf",
    bytes: 66_398_592,
    unzip_into: None,
    marker: "",
    pick: &[],
    vram_gb: Some(2),
    note: "Restores music compressed to MP3: removes the artefacts of compression, 44.1 kHz, the channels kept. Apollo (JusperLee), weights CC BY-SA 4.0, runs on audio.cpp's server.",
};

/// The model's id in audio.cpp's server.
pub const ID: &str = "apollo";

pub const SAMPLE_RATE: u32 = 44_100;

/// What a restored track is called in the library: its source's title, marked.
pub fn restored_title(title: &str) -> String {
    format!("{title} · restored")
}

/// Apollo as audio.cpp's server carries it. Windows of 6 s, crossfaded over 1 s with 1 s of context on each side, keep
/// the memory a song of any length takes to that of six seconds.
pub fn entry(model: &Path) -> acpp_server::ModelEntry {
    let options: serde_json::Map<String, Value> =
        [("audio_chunk_duration_sec", 6), ("audio_chunk_overlap_sec", 1), ("edge_pad_duration_sec", 1)].into_iter().map(|(key, value)| (key.to_string(), json!(value))).collect();
    acpp_server::ModelEntry {
        id: ID.to_string(),
        family: "apollo".to_string(),
        path: model.to_path_buf(),
        task: "s2s".to_string(),
        mode: acpp_server::Mode::Offline,
        load_options: Default::default(),
        session_options: Default::default(),
        default_request_options: options,
        busy_timeout_ms: None,
        output_rate: None,
    }
}

/// Why a restoration cannot start yet; none when it can.
pub fn not_ready(model_installed: bool, server_installed: bool) -> Option<&'static str> {
    match (model_installed, server_installed) {
        (false, _) => Some("Apollo is not downloaded yet: Tools - Restore after compression, or Settings - Models"),
        (true, false) => Some("audio.cpp's server is not installed yet for Apollo: Settings - Models - Restore after compression"),
        (true, true) => None,
    }
}

/// The restored audio: interleaved samples at 44.1 kHz in `channels` channels.
#[derive(Debug)]
pub struct Restored {
    pub samples: Vec<f32>,
    pub channels: u16,
}

/// Restores the 44.1 kHz WAV `input` of `channels` channels through `run`, the server's task on Apollo, and holds what
/// comes back to that rate and those channels.
pub fn restore(input: &Path, channels: u16, run: impl FnOnce(Value) -> Result<Value>) -> Result<Restored> {
    let result = run(json!({ "audio": input }))?;
    let output = acpp_server::task_audio(&result)
        .map_err(|error| anyhow!("Apollo: {}", crate::acpp::describe(&error)))?
        .into_iter()
        .next()
        .context("Apollo returned no audio")?;
    if output.sample_rate != SAMPLE_RATE || output.channels != channels {
        bail!("Apollo returned {} Hz in {} channels, not {SAMPLE_RATE} Hz in {channels}", output.sample_rate, output.channels);
    }
    if output.samples.is_empty() {
        bail!("Apollo returned no audio");
    }
    Ok(Restored { samples: output.samples, channels })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_base64(samples: &[i16], rate: u32, channels: u16) -> String {
        use base64::Engine as _;
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let spec = hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
            let mut writer = hound::WavWriter::new(&mut bytes, spec).unwrap();
            for sample in samples {
                writer.write_sample(*sample).unwrap();
            }
            writer.finalize().unwrap();
        }
        base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
    }

    #[test]
    fn the_server_is_told_apollo_and_cuts_long_recordings_itself() {
        let entry = entry(Path::new("m/apollo/apollo-orig.gguf"));
        assert_eq!((entry.id.as_str(), entry.family.as_str(), entry.task.as_str(), entry.mode), (ID, "apollo", "s2s", acpp_server::Mode::Offline));
        assert_eq!(entry.default_request_options["audio_chunk_duration_sec"], 6);
        assert_eq!(entry.default_request_options["audio_chunk_overlap_sec"], 1);
        assert_eq!(entry.default_request_options["edge_pad_duration_sec"], 1);
    }

    #[test]
    fn the_weights_are_pinned_to_a_revision() {
        assert!(MODEL.url.contains("/resolve/73ce4162f6cce370dd533c6782b772135c1b2a8e/"), "{}", MODEL.url);
        assert!(!MODEL.url.contains("/resolve/main/"));
        assert!(MODEL.note.contains("CC BY-SA 4.0"));
    }

    #[test]
    fn what_is_missing_is_named_before_anything_runs() {
        assert!(not_ready(false, true).unwrap().contains("Apollo is not downloaded"));
        assert!(not_ready(false, false).unwrap().contains("Apollo is not downloaded"));
        assert!(not_ready(true, false).unwrap().contains("server is not installed"));
        assert_eq!(not_ready(true, true), None);
    }

    #[test]
    fn the_restored_audio_comes_back_at_its_rate_and_channels() {
        let mut asked = None;
        let restored = restore(Path::new("in.wav"), 2, |request| {
            asked = Some(request);
            Ok(json!({ "audio": wav_base64(&[100, -100, 200, -200], SAMPLE_RATE, 2) }))
        })
        .unwrap();
        assert_eq!(asked.unwrap()["audio"], "in.wav");
        assert_eq!((restored.channels, restored.samples.len()), (2, 4));
    }

    #[test]
    fn a_mono_recording_stays_mono() {
        let restored = restore(Path::new("in.wav"), 1, |_| Ok(json!({ "audio": wav_base64(&[1, 2, 3], SAMPLE_RATE, 1) }))).unwrap();
        assert_eq!(restored.channels, 1);
    }

    #[test]
    fn audio_of_another_rate_or_channels_is_refused() {
        let error = restore(Path::new("in.wav"), 2, |_| Ok(json!({ "audio": wav_base64(&[1, 2], 48_000, 2) }))).unwrap_err();
        assert!(error.to_string().contains("48000 Hz"), "{error}");
        let error = restore(Path::new("in.wav"), 2, |_| Ok(json!({ "audio": wav_base64(&[1, 2], SAMPLE_RATE, 1) }))).unwrap_err();
        assert!(error.to_string().contains("in 1 channels"), "{error}");
        let error = restore(Path::new("in.wav"), 2, |_| Ok(json!({ "text": "" }))).unwrap_err();
        assert!(error.to_string().contains("no audio"), "{error}");
    }

    #[test]
    fn a_stopped_run_says_stopped() {
        let error = restore(Path::new("in.wav"), 2, |_| bail!("stopped")).unwrap_err();
        assert_eq!(error.to_string(), "stopped");
    }
}
