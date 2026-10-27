//! The update screen (CONTRACT, «The update screen»): what this release changed in what the studio downloads. The
//! plan compares the disk with the release's pins and the leftovers of former releases: what is updated (old → new,
//! size), what is new, what is no longer needed and is removed (space freed), each with its reason from the release's
//! table of changes below. One button downloads it all and removes the leftovers only after the downloads that take
//! their place; the screen comes once per release whose plan is not empty, and later from the settings.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The release whose plan the window has shown, in the data folder.
pub const SEEN_FILE: &str = "update-seen.txt";

/// The part of the studio whose downloader puts a row down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Engine,
    Karaoke,
    Separation,
    Midi,
    Training,
    Listening,
    Assistant,
}

/// One change of this release: a download by its id (a trailing `*` takes every id it begins) or a leftover by the
/// name `Leftover::what` gives it, and its reason.
struct Change {
    what: &'static str,
    reason: &'static str,
}

/// The table of changes of this release against 3.7.0. A release that changes what the studio downloads writes its
/// own rows here; ONNX Runtime on the processor, HT-Demucs, MuScriptor and SheetSage2 on audio.cpp have their reasons
/// in `REASONS` already and add their rows when they come.
const CHANGES: &[Change] = &[
    Change { what: "crisperwhisper-turbo*", reason: "crisperwhisper" },
    Change { what: "audiocpp-*", reason: "crisperwhisper" },
    Change { what: "transcribe-engine*", reason: "transcribe-avx2" },
    Change { what: "llama-*", reason: "llama" },
    Change { what: "music-train", reason: "hotstep" },
    Change { what: "ace-caption", reason: "hotstep" },
    Change { what: "music-midi", reason: "hotstep" },
    Change { what: "engine-cuda12-cublas", reason: "cublas-wheel" },
    Change { what: "cuda-cublas", reason: "cublas-wheel" },
    Change { what: "faster-whisper", reason: "faster-whisper-gone" },
    Change { what: "parakeet-onnx", reason: "parakeet-onnx-gone" },
];

/// Every reason, by its code, in English; the window says it in its own language by the code.
pub const REASONS: &[(&str, &str)] = &[
    ("crisperwhisper", "Karaoke and the dataset lyrics are heard by CrisperWhisper 2.0 turbo in the audio.cpp server instead of Faster-Whisper, with a time for every word"),
    ("transcribe-avx2", "Built for every processor with AVX2: Parakeet no longer closes the studio on processors without AVX-512"),
    ("llama", "A newer llama.cpp for the assistant"),
    ("hotstep", "A newer build of HOT-Step's tools for training, captions and audio to MIDI"),
    ("cublas-wheel", "The same cuBLAS 12.9, now from NVIDIA's package with its checksum pinned"),
    ("faster-whisper-gone", "Faster-Whisper is replaced by CrisperWhisper"),
    ("parakeet-onnx-gone", "Parakeet runs on transcribe.cpp; these ONNX models are no longer read"),
    ("ort-cpu", "Tempo, key and the listening pack run on ONNX Runtime on the processor"),
    ("ort-gpu-gone", "ONNX Runtime no longer runs on the card; its CUDA, cuDNN and DirectML files are not read"),
    ("demucs-audiocpp", "Stems are split by HT-Demucs in the audio.cpp server"),
    ("midi-audiocpp", "Audio to MIDI runs MuScriptor and SheetSage2 in the audio.cpp server"),
    ("newer", "This release uses another build of it"),
    ("unused", "No longer used"),
];

fn reason_of(what: &str, default: &'static str) -> &'static str {
    CHANGES
        .iter()
        .find(|change| match change.what.strip_suffix('*') {
            Some(prefix) => what.starts_with(prefix),
            None => what == change.what,
        })
        .map_or(default, |change| change.reason)
}

fn english(code: &str) -> &'static str {
    REASONS.iter().find(|(known, _)| *known == code).map_or("", |(_, text)| text)
}

/// One download of a part of the studio, as the disk has it.
pub struct Candidate {
    pub part: Part,
    pub id: &'static str,
    pub label: &'static str,
    pub model: bool,
    pub relative_path: &'static str,
    pub url: &'static str,
    pub bytes: u64,
    pub downloaded: u64,
    /// What an earlier release left of it: the URL its record names, or nothing when its files lie there unrecorded.
    pub earlier: Option<String>,
}

/// A leftover of former releases, by the name the table of changes knows it, and its files and folders.
pub struct Leftover {
    pub what: &'static str,
    pub name: &'static str,
    pub paths: Vec<PathBuf>,
}

/// A component to download: updated (`from` names the release on disk when the disk tells it) or new. The files of
/// one model's folder are one row.
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub id: String,
    pub part: Part,
    pub ids: Vec<&'static str>,
    pub name: String,
    pub from: Option<String>,
    pub to: String,
    pub bytes: u64,
    /// Bytes still to download.
    pub download: u64,
    pub reason_code: &'static str,
    pub reason: &'static str,
}

/// A leftover of a former release that the button removes.
#[derive(Clone, Debug, Serialize)]
pub struct Gone {
    pub what: &'static str,
    pub name: &'static str,
    /// Where it lies, under the data folder.
    pub paths: Vec<String>,
    /// Bytes its removal frees (a linked-in folder frees none).
    pub size: u64,
    pub reason_code: &'static str,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    pub version: String,
    pub updated: Vec<Row>,
    pub added: Vec<Row>,
    pub retired: Vec<Gone>,
    pub download: u64,
    pub freed: u64,
    /// The window shows the screen at start: the plan is not empty and this release's plan has not been shown yet.
    pub show: bool,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.updated.is_empty() && self.added.is_empty() && self.retired.is_empty()
    }
}

/// The release a pinned URL names: the GitHub release tag, the Hugging Face revision, the version in a package's
/// name, or else the file name.
pub fn release_of(url: &str) -> String {
    let parts: Vec<&str> = url.split('/').collect();
    let after = |word: &str| parts.iter().position(|part| *part == word).and_then(|at| parts.get(at + 1)).copied();
    if let (Some(_), Some(tag)) = (parts.iter().position(|part| *part == "releases"), after("download")) {
        return tag.to_string();
    }
    if let Some(revision) = after("resolve") {
        return revision.chars().take(8).collect();
    }
    parts.last().copied().unwrap_or_default().to_string()
}

/// The rows of `candidates`, the files of one model's folder together.
fn rows(candidates: Vec<Candidate>, default_reason: &'static str) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for candidate in candidates {
        let key = match (candidate.model, Path::new(candidate.relative_path).parent()) {
            (true, Some(folder)) if !folder.as_os_str().is_empty() => folder.to_string_lossy().replace('\\', "/"),
            _ => candidate.id.to_string(),
        };
        let left = candidate.bytes.saturating_sub(candidate.downloaded);
        if let Some(row) = rows.iter_mut().find(|row| row.id == key && row.part == candidate.part) {
            row.ids.push(candidate.id);
            row.bytes += candidate.bytes;
            row.download += left;
            continue;
        }
        let code = reason_of(candidate.id, default_reason);
        rows.push(Row {
            id: key,
            part: candidate.part,
            ids: vec![candidate.id],
            name: candidate.label.split(": ").next().unwrap_or(candidate.label).to_string(),
            from: candidate.earlier.as_deref().map(release_of),
            to: release_of(candidate.url),
            bytes: candidate.bytes,
            download: left,
            reason_code: code,
            reason: english(code),
        });
    }
    rows
}

/// The bytes removing `path` frees: a link frees nothing, a folder what lies in it.
pub fn tree_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if !meta.is_dir() {
        return meta.len();
    }
    match std::fs::read_dir(path) {
        Ok(entries) => entries.flatten().map(|entry| tree_size(&entry.path())).sum(),
        Err(error) => {
            studio_log!("{} is not readable, its size is not counted: {error}", path.display());
            0
        }
    }
}

/// The plan of this release: `updated` are downloads of which an earlier release lies on disk, `added` the new ones
/// that take the place of what the person used, `leftovers` what former releases left.
pub fn plan(updated: Vec<Candidate>, added: Vec<Candidate>, leftovers: Vec<Leftover>, data_root: &Path, version: &str) -> Plan {
    let updated = rows(updated, "newer");
    let added = rows(added, "newer");
    let retired: Vec<Gone> = leftovers
        .into_iter()
        .filter(|leftover| !leftover.paths.is_empty())
        .map(|leftover| {
            let code = reason_of(leftover.what, "unused");
            Gone {
                what: leftover.what,
                name: leftover.name,
                paths: leftover
                    .paths
                    .iter()
                    .map(|path| path.strip_prefix(data_root).unwrap_or(path).to_string_lossy().replace('\\', "/"))
                    .collect(),
                size: leftover.paths.iter().map(|path| tree_size(path)).sum(),
                reason_code: code,
                reason: english(code),
            }
        })
        .collect();
    let download = updated.iter().chain(&added).map(|row| row.download).sum();
    let freed = retired.iter().map(|gone| gone.size).sum();
    let mut plan = Plan { version: version.to_string(), updated, added, retired, download, freed, show: false };
    plan.show = !plan.is_empty() && !seen(data_root, version);
    plan
}

/// Whether the window has shown this release's plan.
fn seen(data_root: &Path, version: &str) -> bool {
    match std::fs::read_to_string(data_root.join(SEEN_FILE)) {
        Ok(text) => text.lines().last().is_some_and(|line| line.trim() == version),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            studio_log!("[ERROR] {SEEN_FILE}: {error}");
            false
        }
    }
}

/// The window has shown this release's plan (the person pressed the button or «Later»).
pub fn mark_seen(data_root: &Path, version: &str) -> Result<(), String> {
    std::fs::write(data_root.join(SEEN_FILE), format!("{version}\n")).map_err(|error| format!("{SEEN_FILE}: {error}"))
}

/// The update screen's download, while it runs and after: what it fetches, and how it ended.
#[derive(Clone, Debug, Serialize)]
pub struct Job {
    pub status: JobStatus,
    pub error: Option<String>,
    /// The plan's rows as they were when the button was pressed, with what is still to download.
    pub updated: Vec<Row>,
    pub added: Vec<Row>,
}

impl Job {
    pub fn rows_mut(&mut self) -> impl Iterator<Item = &mut Row> {
        self.updated.iter_mut().chain(self.added.iter_mut())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Done,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &'static str, model: bool, relative_path: &'static str, earlier: Option<&str>) -> Candidate {
        Candidate {
            part: Part::Karaoke,
            id,
            label: "CrisperWhisper 2.0 turbo: config.json",
            model,
            relative_path,
            url: "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/config.json",
            bytes: 100,
            downloaded: 40,
            earlier: earlier.map(String::from),
        }
    }

    #[test]
    fn a_release_is_named_by_its_tag_or_revision() {
        assert_eq!(release_of("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-win-cuda-13.4-x64.zip"), "b11900");
        assert_eq!(release_of("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/model.safetensors"), "de0369c8");
        assert_eq!(release_of("https://example.invalid/e.dll"), "e.dll");
    }

    #[test]
    fn every_change_has_its_reason_in_every_language_of_the_window() {
        let windows = [include_str!("../../../app/i18n/en.ts"), include_str!("../../../app/i18n/ru.ts"), include_str!("../../../app/i18n/zh.ts"), include_str!("../../../app/i18n/ja.ts"), include_str!("../../../app/i18n/ko.ts")];
        let screen = include_str!("../../../app/components/UpdateScreen.tsx");
        for change in CHANGES {
            assert!(REASONS.iter().any(|(code, _)| *code == change.reason), "{}: {} has no text", change.what, change.reason);
        }
        for (code, text) in REASONS {
            assert!(!text.is_empty());
            let key = format!("updateWhy{}", code.split('-').map(|word| word[..1].to_uppercase() + &word[1..]).collect::<String>());
            assert!(screen.contains(&format!("'{code}': '{key}'")), "the window does not name {code} by {key}");
            for window in windows {
                assert!(window.contains(&format!("{key}:")), "{key} is missing from a language of the window");
            }
        }
    }

    #[test]
    fn a_models_files_are_one_row_and_a_runtime_its_own() {
        let p = plan(
            vec![candidate("transcribe-engine", false, "runtime/transcribe_engine.dll", Some("https://github.com/timoncool/transcribe.cpp/releases/download/engine-0.1.1/transcribe_engine.dll"))],
            vec![
                candidate("crisperwhisper-turbo", true, "models/crisperwhisper2-turbo/model.safetensors", None),
                candidate("crisperwhisper-turbo-config", true, "models/crisperwhisper2-turbo/config.json", None),
            ],
            Vec::new(),
            Path::new("/nowhere"),
            "9.9.9",
        );
        assert_eq!(p.updated.len(), 1);
        let engine = &p.updated[0];
        assert_eq!((engine.from.as_deref(), engine.reason_code), (Some("engine-0.1.1"), "transcribe-avx2"));
        assert_eq!(p.added.len(), 1, "a model's files are one row");
        let model = &p.added[0];
        assert_eq!((model.id.as_str(), model.ids.len(), model.bytes, model.download), ("models/crisperwhisper2-turbo", 2, 200, 120));
        assert_eq!((model.name.as_str(), model.reason_code), ("CrisperWhisper 2.0 turbo", "crisperwhisper"));
        assert_eq!(p.download, 60 + 120);
        assert!(p.show);
    }

    #[test]
    fn a_plan_shows_once_per_release_and_removes_nothing_itself() {
        let root = tempfile::tempdir().unwrap();
        let whisper = root.path().join("karaoke/models/whisper");
        std::fs::create_dir_all(&whisper).unwrap();
        std::fs::write(whisper.join("model.bin"), vec![0u8; 500]).unwrap();
        let leftovers = || vec![Leftover { what: "faster-whisper", name: "Faster-Whisper", paths: vec![whisper.clone()] }, Leftover { what: "parakeet-onnx", name: "Parakeet (ONNX)", paths: Vec::new() }];

        let p = plan(Vec::new(), Vec::new(), leftovers(), root.path(), "9.9.9");
        assert!(p.show);
        assert_eq!(p.retired.len(), 1, "a leftover not on disk is no row");
        assert_eq!((p.retired[0].paths.clone(), p.retired[0].size, p.retired[0].reason_code), (vec!["karaoke/models/whisper".to_string()], 500, "faster-whisper-gone"));
        assert_eq!(p.freed, 500);
        assert!(whisper.is_dir(), "the plan itself removes nothing");

        mark_seen(root.path(), "9.9.9").unwrap();
        assert!(!plan(Vec::new(), Vec::new(), leftovers(), root.path(), "9.9.9").show, "shown once per release");
        assert!(plan(Vec::new(), Vec::new(), leftovers(), root.path(), "10.0.0").show, "the next release shows its own");
        assert!(!plan(Vec::new(), Vec::new(), Vec::new(), root.path(), "10.0.0").show, "an empty plan shows nothing");
    }
}
