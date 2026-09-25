//! Audio to MIDI: MuScriptor (Kyutai & Mirelo, arXiv:2607.08168) run by
//! HOT-Step's native GGML port, `ace-midi`, shipped here as `music-midi.exe`.
//!
//! Nothing of it comes with the studio. The transcriber is a release asset
//! built from the same HOT-Step commit as the trainer, and the weights come
//! from an open mirror of the gated official repositories (the same files,
//! byte for byte); both are fetched the first time a track is turned into
//! MIDI, or from the tools page before that. The code is MIT; the weights are
//! CC BY-NC 4.0, which the tools page says.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::downloads::{Asset, AssetKind, Downloader};

#[derive(Deserialize)]
struct Source {
    commit: String,
    shipped_as: String,
    release_tag: String,
    asset: String,
    bytes: u64,
}

fn source() -> &'static Source {
    static SOURCE: OnceLock<Source> = OnceLock::new();
    SOURCE.get_or_init(|| serde_json::from_str(include_str!("../../../engines/music-midi-source.json")).expect("engines/music-midi-source.json is valid"))
}

/// The folder the transcriber's archive unpacks into.
const TOOL_FOLDER: &str = "music-midi";

/// One size of the model.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Size {
    pub id: &'static str,
    pub params: &'static str,
    /// The mirror's revision the files were measured at.
    #[serde(skip)]
    revision: &'static str,
    #[serde(skip)]
    config_bytes: u64,
    pub bytes: u64,
}

pub const SIZES: [Size; 3] = [
    Size { id: "small", params: "103M", revision: "31a8f75d6a8b5383fd71ad1371dc1620389ab722", config_bytes: 124, bytes: 411_888_600 },
    Size { id: "medium", params: "307M", revision: "27246ba68bd4d8f98bdec10a6edf8d7cf42a8826", config_bytes: 126, bytes: 1_228_144_472 },
    Size { id: "large", params: "1.4B", revision: "87f4bf981f56f90fb5043153b3f54af3c3053da9", config_bytes: 125, bytes: 5_465_642_136 },
];

/// What a track is transcribed with when nobody chose: the middle size hears
/// most of what the large one does for a quarter of the download.
pub const DEFAULT_SIZE: &str = "medium";

pub fn size(id: &str) -> Option<&'static Size> {
    SIZES.iter().find(|size| size.id == id)
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// The transcriber's archive.
fn tool_asset() -> &'static Asset {
    static ASSET: OnceLock<Asset> = OnceLock::new();
    ASSET.get_or_init(|| {
        let source = source();
        Asset {
            id: "music-midi",
            label: leak(format!("Audio to MIDI (HOT-Step {})", &source.commit[..8])),
            kind: AssetKind::Runtime,
            url: leak(format!("https://github.com/timoncool/YuE2-Studio/releases/download/{}/{}", source.release_tag, source.asset)),
            relative_path: leak(source.asset.clone()),
            bytes: source.bytes,
            unzip_into: Some(TOOL_FOLDER),
            marker: leak(source.shipped_as.clone()),
            pick: &[],
            vram_gb: None,
            note: "",
        }
    })
}

/// A size's two files, from the open mirror at a fixed revision.
fn weight_assets(size: &'static Size) -> &'static [Asset] {
    static ASSETS: OnceLock<HashMap<&'static str, Vec<Asset>>> = OnceLock::new();
    ASSETS.get_or_init(|| {
        SIZES
            .iter()
            .map(|size| {
                let file = |name: &str, bytes: u64| Asset {
                    id: leak(format!("muscriptor-{}-{}", size.id, name.split('.').next().unwrap_or(name))),
                    label: leak(format!("MuScriptor {} ({})", size.id, size.params)),
                    kind: AssetKind::Model,
                    url: leak(format!("https://huggingface.co/cocktailpeanut/muscriptor-{}/resolve/{}/{name}", size.id, size.revision)),
                    relative_path: leak(format!("models/muscriptor-{}/{name}", size.id)),
                    bytes,
                    unzip_into: None,
                    marker: "",
                    pick: &[],
                    vram_gb: None,
                    note: "CC BY-NC 4.0: for non-commercial use.",
                };
                (size.id, vec![file("config.json", size.config_bytes), file("model.safetensors", size.bytes)])
            })
            .collect()
    })[size.id]
        .as_slice()
}

/// The transcriber and the model sizes on disk.
pub struct Transcriber {
    downloader: Downloader,
}

impl Transcriber {
    pub fn new(data_root: &Path) -> Self {
        Self { downloader: Downloader::new(data_root.join("midi")) }
    }

    pub fn downloader(&self) -> &Downloader {
        &self.downloader
    }

    /// `STUDIO_MIDI_BIN` in a developer build, else the one the archive unpacked.
    pub fn tool(&self) -> PathBuf {
        std::env::var_os("STUDIO_MIDI_BIN").map(PathBuf::from).unwrap_or_else(|| self.downloader.runtime_dir(TOOL_FOLDER).join(&source().shipped_as))
    }

    pub fn tool_installed(&self) -> bool {
        self.tool().is_file()
    }

    pub fn model_dir(&self, size: &Size) -> PathBuf {
        self.downloader.root().join("models").join(format!("muscriptor-{}", size.id))
    }

    pub fn model_installed(&self, size: &'static Size) -> bool {
        weight_assets(size).iter().all(|asset| self.downloader.is_installed(asset))
    }

    /// What is still missing before a size can transcribe.
    pub fn missing(&self, size: &'static Size) -> Vec<&'static Asset> {
        let mut assets = Vec::new();
        if !self.tool_installed() {
            assets.push(tool_asset());
        }
        assets.extend(weight_assets(size).iter().filter(|asset| !self.downloader.is_installed(asset)));
        assets
    }

    pub fn missing_bytes(&self, size: &'static Size) -> u64 {
        self.missing(size).iter().map(|asset| asset.bytes.saturating_sub(self.downloader.partial_bytes(asset))).sum()
    }

    /// Deletes a size's weights; the transcriber stays.
    pub fn remove(&self, size: &'static Size) -> anyhow::Result<u64> {
        let mut freed = 0;
        for asset in weight_assets(size) {
            freed += self.downloader.remove(asset)?;
        }
        Ok(freed)
    }

    /// Where a file handed in by path is written: the studio's own folder.
    pub fn loose_output(&self, source: &Path) -> PathBuf {
        let stem = source.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_else(|| "audio".into());
        self.downloader.root().join("files").join(format!("{stem}.mid"))
    }

    pub fn work_dir(&self) -> PathBuf {
        self.downloader.root().join("work")
    }
}

/// One note as the transcriber heard it, in seconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub pitch: u8,
    pub start: f64,
    pub end: f64,
    pub instrument: String,
}

/// A transcription at work, or the last one.
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    /// The library track, or none for a file named by its path.
    pub song_id: Option<String>,
    pub title: String,
    pub size: &'static str,
    /// preparing, downloading, reading, transcribing, done.
    pub stage: &'static str,
    pub chunks_done: u32,
    pub chunks_total: u32,
    pub notes: usize,
    pub done: bool,
    pub error: Option<String>,
    /// The MIDI file on this computer, once written.
    pub file: Option<String>,
}

impl Run {
    pub fn progress(&self) -> f64 {
        if self.done && self.error.is_none() {
            1.0
        } else if self.chunks_total == 0 {
            0.0
        } else {
            f64::from(self.chunks_done) / f64::from(self.chunks_total)
        }
    }
}

/// What a song's MIDI says about itself, kept beside the .mid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sidecar {
    pub size: String,
    pub made_at: String,
    pub instruments: Vec<String>,
    pub notes: Vec<Note>,
}

/// Collects the transcriber's JSON lines into notes and progress.
#[derive(Default)]
pub struct Events {
    pub notes: Vec<Note>,
    open: HashMap<i64, usize>,
    pub chunks_done: u32,
    pub chunks_total: u32,
    pub finished: bool,
}

impl Events {
    /// Takes one line of `--jsonl` output; anything that is not an event is ignored.
    pub fn take(&mut self, line: &str) {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line.trim()) else { return };
        match event["type"].as_str() {
            Some("progress") => {
                self.chunks_done = event["completed"].as_u64().unwrap_or(0) as u32;
                self.chunks_total = event["total"].as_u64().unwrap_or(0) as u32;
            }
            Some("note_start") => {
                let start = event["time"].as_f64().unwrap_or(0.0);
                self.notes.push(Note {
                    pitch: event["pitch"].as_u64().unwrap_or(0).min(127) as u8,
                    start,
                    end: start,
                    instrument: event["instrument"].as_str().unwrap_or("unknown").to_string(),
                });
                if let Some(index) = event["index"].as_i64() {
                    self.open.insert(index, self.notes.len() - 1);
                }
            }
            Some("note_end") => {
                if let (Some(index), Some(time)) = (event["index"].as_i64(), event["time"].as_f64()) {
                    if let Some(at) = self.open.remove(&index) {
                        self.notes[at].end = time.max(self.notes[at].start);
                    }
                }
            }
            Some("done") => self.finished = true,
            _ => {}
        }
    }

    pub fn instruments(&self) -> Vec<String> {
        let mut names: Vec<String> = self.notes.iter().map(|note| note.instrument.clone()).collect();
        names.sort();
        names.dedup();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transcribers_lines_become_notes_and_progress() {
        let mut events = Events::default();
        for line in [
            r#"{"type":"progress","completed":0,"total":4}"#,
            r#"{"type":"note_start","index":0,"pitch":60,"time":0.5,"instrument":"acoustic_piano"}"#,
            r#"{"type":"note_start","index":1,"pitch":36,"time":0.5,"instrument":"drums"}"#,
            r#"{"type":"note_end","index":1,"time":0.52}"#,
            "[ace-midi] a log line",
            r#"{"type":"note_end","index":0,"time":1.25}"#,
            r#"{"type":"progress","completed":1,"total":4}"#,
            r#"{"type":"done","notes":2,"midi_bytes":120}"#,
        ] {
            events.take(line);
        }
        assert_eq!(events.notes.len(), 2);
        assert_eq!((events.notes[0].pitch, events.notes[0].start, events.notes[0].end), (60, 0.5, 1.25));
        assert_eq!(events.notes[1].instrument, "drums");
        assert_eq!((events.chunks_done, events.chunks_total), (1, 4));
        assert!(events.finished);
        assert_eq!(events.instruments(), ["acoustic_piano", "drums"]);
    }

    #[test]
    fn every_size_has_its_two_files_on_the_mirror() {
        for size in &SIZES {
            let assets = weight_assets(size);
            assert_eq!(assets.len(), 2);
            assert!(assets.iter().all(|asset| asset.url.contains(size.revision) && asset.url.contains("cocktailpeanut/muscriptor-")));
        }
        assert!(size(DEFAULT_SIZE).is_some());
    }
}
