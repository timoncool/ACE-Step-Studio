//! Processing a finished track: noise reduction, the Spectral Lifter, vocal
//! naturalising, the user's own VST3 plugins and mastering to a reference, in
//! that order.
//!
//! A run never touches the track. It leaves a preview beside the library, to be
//! heard against the original and then kept as a version or thrown away; a kept
//! version plays in place of the original, which stays on disk and can be
//! chosen again at any time.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use audio_post::{denoise, lifter, mastering, naturalize, Stereo};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What to do to a track. A stage left out is skipped.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessRequest {
    #[serde(default)]
    pub denoise: Option<denoise::DenoiseSettings>,
    #[serde(default)]
    pub lifter: Option<lifter::LifterSettings>,
    #[serde(default)]
    pub naturalize: Option<naturalize::NaturalizeSettings>,
    /// VST3 plugins, run in order before mastering.
    #[serde(default)]
    pub vst: Option<Vec<crate::vst::VstSlot>>,
    #[serde(default)]
    pub master: Option<MasterSource>,
}

/// The reference a track is mastered to: another song of the library, or a
/// file uploaded for the purpose.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MasterSource {
    Song { song_id: String },
    Upload { upload_id: String },
}

impl ProcessRequest {
    pub fn stages(&self) -> Vec<&'static str> {
        let mut stages = Vec::new();
        if self.denoise.is_some() {
            stages.push("denoise");
        }
        if self.lifter.is_some() {
            stages.push("lifter");
        }
        if self.naturalize.is_some() {
            stages.push("naturalize");
        }
        if self.vst.as_ref().is_some_and(|chain| chain.iter().any(|slot| slot.enabled)) {
            stages.push("vst");
        }
        if self.master.is_some() {
            stages.push("master");
        }
        stages
    }
}

/// The run in progress or the last one, as the interface polls it.
#[derive(Debug, Clone, Serialize)]
pub struct ProcessRun {
    /// Tells a finishing worker whether its run is still the current one.
    pub id: String,
    pub song_id: String,
    pub stages: Vec<&'static str>,
    /// The stage working now, once started.
    pub stage: Option<&'static str>,
    pub done: bool,
    pub error: Option<String>,
    /// The preview's file name inside the processing folder, when ready.
    #[serde(skip)]
    pub preview: Option<String>,
    pub preview_ready: bool,
    pub request: ProcessRequest,
}

impl ProcessRun {
    /// The files only this run owns: its preview and an uploaded reference.
    pub fn leftovers(&self, media: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self.preview.iter().filter_map(|name| workspace_file(media, name)).collect();
        if let Some(MasterSource::Upload { upload_id }) = &self.request.master {
            files.extend(workspace_file(media, upload_id));
        }
        files
    }
}

/// Empties the workspace: previews and references live for one sitting.
pub fn clear_workspace(media: &Path) {
    let Ok(entries) = std::fs::read_dir(workspace(media)) else { return };
    for entry in entries.flatten() {
        if entry.path().is_file() {
            if let Err(error) = std::fs::remove_file(entry.path()) {
                eprintln!("[ERROR] processing: remove {}: {error}", entry.path().display());
            }
        }
    }
}

/// Where previews and uploaded references wait, inside the media folder.
pub fn workspace(media: &Path) -> PathBuf {
    media.join("processing")
}

/// A name that is a plain file of the workspace, never a path out of it.
pub fn workspace_file(media: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.starts_with('.') {
        return None;
    }
    let path = workspace(media).join(name);
    path.is_file().then_some(path)
}

/// Runs the stages on `source`, calling `on_stage` as each begins.
pub fn run(
    source: &Path,
    reference: Option<&Path>,
    request: &ProcessRequest,
    vst: Option<&crate::vst::VstHost>,
    on_stage: impl Fn(&'static str),
) -> Result<Stereo> {
    if request.stages().is_empty() {
        bail!("choose at least one kind of processing");
    }
    let mut audio = crate::audio_pcm::decode_stereo(source)?;
    if let Some(settings) = &request.denoise {
        on_stage("denoise");
        audio = denoise::denoise(&audio, settings);
    }
    if let Some(settings) = &request.lifter {
        on_stage("lifter");
        audio = lifter::lift(&audio, settings);
    }
    if let Some(settings) = &request.naturalize {
        on_stage("naturalize");
        audio = naturalize::naturalize(&audio, settings);
    }
    if let Some(chain) = request.vst.as_ref().filter(|chain| chain.iter().any(|slot| slot.enabled)) {
        on_stage("vst");
        let host = vst.context("the VST host is not installed")?;
        let work = source.parent().context("the track has no folder")?.join("processing");
        audio = host.process(&audio, chain, &work)?;
    }
    if request.master.is_some() {
        on_stage("master");
        let reference = reference.context("mastering needs a reference track")?;
        let reference = crate::audio_pcm::decode_stereo(reference)?;
        audio = mastering::master(&audio, &reference, &mastering::MasteringConfig::default())?;
    }
    Ok(audio)
}

/// The processing settings a kept version records, for showing and repeating.
pub fn settings_record(request: &ProcessRequest, reference_title: Option<&str>) -> Value {
    let mut value = serde_json::to_value(request).unwrap_or(Value::Null);
    if let (Some(title), Some(object)) = (reference_title, value.as_object_mut()) {
        object.insert("reference_title".into(), Value::String(title.to_string()));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_follow_the_request_in_processing_order() {
        let request = ProcessRequest {
            master: Some(MasterSource::Song { song_id: "x".into() }),
            denoise: Some(Default::default()),
            ..Default::default()
        };
        assert_eq!(request.stages(), vec!["denoise", "master"]);
        let parsed: ProcessRequest =
            serde_json::from_value(serde_json::json!({"lifter": {"shimmer_reduction_db": 3.0}, "master": {"type": "upload", "upload_id": "u"}})).unwrap();
        assert_eq!(parsed.stages(), vec!["lifter", "master"]);
        assert_eq!(parsed.lifter.unwrap().shimmer_reduction_db, 3.0);
    }

    #[test]
    fn a_vst_chain_runs_before_mastering_only_with_a_plugin_on() {
        let chain = |enabled| serde_json::json!([{ "path": "C:/x.vst3", "name": "X", "enabled": enabled }]);
        let on: ProcessRequest = serde_json::from_value(serde_json::json!({ "vst": chain(true), "master": { "type": "upload", "upload_id": "u" } })).unwrap();
        assert_eq!(on.stages(), vec!["vst", "master"]);
        let off: ProcessRequest = serde_json::from_value(serde_json::json!({ "vst": chain(false) })).unwrap();
        assert!(off.stages().is_empty());
    }

    #[test]
    fn workspace_names_cannot_leave_the_folder() {
        let media = std::env::temp_dir();
        assert!(workspace_file(&media, "../library.sqlite").is_none());
        assert!(workspace_file(&media, "a\\b.wav").is_none());
        assert!(workspace_file(&media, "").is_none());
    }
}
