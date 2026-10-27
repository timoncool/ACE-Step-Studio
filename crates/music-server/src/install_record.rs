//! What a download put on disk, and which pin it came from.
//!
//! Beside every archive or file a download fetched lies `<that path>.json`, the
//! record Dub Studio keeps too: the archive's SHA-256, the pinned URL it came
//! from and every file it put down with its size. An asset counts as installed
//! when its record names the URL of today's pin and every recorded file is
//! there at its recorded size, so a runtime of an earlier pin under the same
//! file names is found out instead of being taken for the new one.
//!
//! What was downloaded before records existed has none. A model counts by its
//! presence, as before; a runtime is taken over only when the one file the pin
//! names (`KEY_FILES`) has exactly the pinned size, and is otherwise fetched
//! again.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallRecord {
    pub sha256: String,
    pub url: String,
    pub files: Vec<RecordedFile>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecordedFile {
    /// Relative to the root the record was written for, with `/`.
    pub path: String,
    pub size: u64,
}

/// For a runtime downloaded before records existed: the file of the pinned
/// archive that tells its version apart, relative to where the archive
/// unpacks or picks into, and that file's size in the archive of the pin.
const KEY_FILES: &[(&str, &str, u64)] = &[
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-win-cuda-13.4-x64.zip", "ggml-cuda.dll", 154_890_752),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-win-cuda-12.4-x64.zip", "ggml-cuda.dll", 593_773_568),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-win-vulkan-x64.zip", "ggml-vulkan.dll", 41_590_784),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-win-cpu-x64.zip", "ggml-cpu-haswell.dll", 1_426_944),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-win-x64-1.30.0.zip", "onnxruntime.dll", 16_462_648),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-win-x64-gpu_cuda12-1.30.0.zip", "onnxruntime_providers_cuda.dll", 352_453_432),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11236/cudart-llama-bin-win-cuda-13.4-x64.zip", "cudart64_13.dll", 551_024),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11236/cudart-llama-bin-win-cuda-12.4-x64.zip", "cudart64_12.dll", 553_984),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-13.5.1.27-archive.zip", "cublasLt64_13.dll", 460_301_424),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-12.9.1.4-archive.zip", "cublasLt64_12.dll", 668_669_952),
    ("https://github.com/Purfview/whisper-standalone-win/releases/download/faster-whisper/Whisper-Faster_r192.3_windows.zip", "whisper-faster.exe", 88_436_526),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-11.11.3.6-archive.zip", "cublas64_11.dll", 88_668_160),
    ("https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/windows-x86_64/cudnn-windows-x86_64-8.9.7.29_cuda11-archive.zip", "cudnn64_8.dll", 288_296),
    ("https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl", "cublasLt64_12.dll", 668_673_536),
    ("https://developer.download.nvidia.com/compute/cuda/redist/cuda_cudart/windows-x86_64/cuda_cudart-windows-x86_64-12.9.79-archive.zip", "cudart64_12.dll", 583_680),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-11.4.1.4-archive.zip", "cufft64_11.dll", 287_136_768),
    ("https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/windows-x86_64/cudnn-windows-x86_64-9.25.0.15_cuda12-archive.zip", "cudnn64_9.dll", 267_888),
    ("https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.24.4/microsoft.ml.onnxruntime.directml.1.24.4.nupkg", "onnxruntime.dll", 17_328_152),
    ("https://api.nuget.org/v3-flatcontainer/microsoft.ai.directml/1.15.4/microsoft.ai.directml.1.15.4.nupkg", "DirectML.dll", 18_527_776),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-windows-cuda-13.1.0.zip", "bs_roformer-cli.exe", 182_784),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-windows-x64-msvc.zip", "bs_roformer-cli.exe", 183_808),
    // Linux and macOS: the regular file a tar's links point to, under its own versioned name
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-x64-1.30.0.tgz", "libonnxruntime.so.1.30.0", 28_985_152),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-x64-gpu_cuda12-1.30.0.tgz", "libonnxruntime_providers_cuda.so", 597_264_112),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-osx-arm64-1.30.0.tgz", "libonnxruntime.1.30.0.dylib", 43_879_424),
    ("https://developer.download.nvidia.com/compute/cuda/redist/cuda_cudart/linux-x86_64/cuda_cudart-linux-x86_64-12.9.79-archive.tar.xz", "libcudart.so.12.9.79", 741_088),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcufft/linux-x86_64/libcufft-linux-x86_64-11.4.1.4-archive.tar.xz", "libcufft.so.11.4.1.4", 289_566_960),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcurand/linux-x86_64/libcurand-linux-x86_64-10.3.10.19-archive.tar.xz", "libcurand.so.10.3.10.19", 166_965_432),
    ("https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/linux-x86_64/cudnn-linux-x86_64-9.25.0.15_cuda12-archive.tar.xz", "libcudnn.so.9.25.0", 129_240),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-linux-cuda-13.1.0.tar.xz", "bs_roformer-cli", 304_904),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-linux-x64-cpu.tar.xz", "bs_roformer-cli", 295_400),
];

/// The record of the archive or file at `path`.
pub fn record_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".json");
    PathBuf::from(name)
}

pub fn read(record: &Path) -> Option<InstallRecord> {
    let text = fs::read_to_string(record).ok()?;
    match serde_json::from_str(&text) {
        Ok(parsed) => Some(parsed),
        Err(error) => {
            studio_log!("[ERROR] the install record {} is unreadable ({error}); its download counts as not installed", record.display());
            None
        }
    }
}

/// Whether the record at `record` says `url` was installed and every file it
/// put under `root` is there at its size. None when there is no record; false
/// while an install is under way or was cut off (`begin`), record or not.
pub fn matches(record: &Path, url: &str, root: &Path) -> Option<bool> {
    if installing_mark(record).exists() {
        return Some(false);
    }
    let parsed = read(record)?;
    Some(
        parsed.url == url
            && !parsed.files.is_empty()
            && parsed.files.iter().all(|file| fs::metadata(root.join(&file.path)).is_ok_and(|meta| meta.is_file() && meta.len() == file.size)),
    )
}

/// The mark beside a record that its download is being installed.
fn installing_mark(record: &Path) -> PathBuf {
    record.with_extension("json.installing")
}

/// Whether the download recorded at `record` is being installed, or its install was cut off.
pub fn under_way(record: &Path) -> bool {
    installing_mark(record).exists()
}

/// Marks the download recorded at `record` as being installed, before any of
/// its files is put down: until `write` replaces the mark with the record, it
/// counts as not installed, and a download from before records is not taken
/// over from the files unpacked so far - also after a crash in between.
pub fn begin(record: &Path) -> Result<()> {
    let mark = installing_mark(record);
    if let Some(directory) = mark.parent() {
        fs::create_dir_all(directory).with_context(|| format!("create {}", directory.display()))?;
    }
    fs::write(&mark, b"").with_context(|| format!("write {}", mark.display()))
}

/// The key file of a pinned runtime archive and its size, when the pin names one.
pub fn key_file(url: &str) -> Option<(&'static str, u64)> {
    KEY_FILES.iter().find(|(pinned, _, _)| *pinned == url).map(|(_, file, size)| (*file, *size))
}

/// Writes the record of `url` at `record`: the archive's hash and the files it
/// put down, relative to `root`. The record lands whole (a temporary file
/// renamed over it), and then the mark of the install under way goes.
pub fn write(record: &Path, url: &str, sha256: &str, files: &[PathBuf], root: &Path) -> Result<()> {
    let files = files
        .iter()
        .map(|path| {
            let size = fs::metadata(path).with_context(|| format!("measure {}", path.display()))?.len();
            let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
            Ok(RecordedFile { path: relative, size })
        })
        .collect::<Result<Vec<_>>>()?;
    let body = serde_json::to_vec_pretty(&InstallRecord { sha256: sha256.to_string(), url: url.to_string(), files })?;
    let partial = record.with_extension("json.tmp");
    fs::write(&partial, body).with_context(|| format!("write {}", partial.display()))?;
    fs::rename(&partial, record).with_context(|| format!("write {}", record.display()))?;
    remove_if_there(&installing_mark(record))
}

/// The mark in the name of a file moved aside by `set_aside`.
const ASIDE: &str = ".stale-";

/// Moves `path` aside under a name of its own beside it, for a file a running program holds, which Windows lets be
/// renamed but not removed or replaced; `sweep_aside` removes it at a later start.
pub fn set_aside(path: &Path) -> Result<PathBuf> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let aside = path.with_file_name(format!("{name}{ASIDE}{}", uuid::Uuid::now_v7().simple()));
    fs::rename(path, &aside).with_context(|| format!("move {} aside", path.display()))?;
    studio_log!("{} is held by a running program and was moved aside as {}; it is removed at the next start", path.display(), aside.display());
    Ok(aside)
}

/// Whether a file name is one `set_aside` gives: the name, `ASIDE`, then a UUID's 32 hex digits at the end.
fn set_aside_name(name: &str) -> bool {
    name.rsplit_once(ASIDE).is_some_and(|(before, id)| !before.is_empty() && id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// Removes what `set_aside` left under `root` in an earlier run, passing by the folders of the person's work.
pub fn sweep_aside(root: &Path) {
    const WORK: [&str; 11] = ["media", "workspace", "processing", "voices", "skins", "logs", "frontend", "fonts", "recordings", "projects", "reports"];
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() {
            if !WORK.contains(&name.as_str()) {
                sweep_aside(&path);
            }
        } else if kind.is_file() && set_aside_name(&name) {
            match fs::remove_file(&path) {
                Ok(()) => studio_log!("removed {}, moved aside by an earlier update", path.display()),
                Err(error) => studio_log!("[ERROR] {} was not removed: {error}", path.display()),
            }
        }
    }
}

fn remove_if_there(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

/// Deletes the files an earlier pin's record lists under `root` but `kept` (the file today's download replaced in
/// place), and that record; the mark of today's install stays. A file a running program holds is moved aside.
pub fn remove_earlier(record: &Path, root: &Path, kept: &Path) -> Result<()> {
    if let Some(parsed) = read(record) {
        for file in &parsed.files {
            let path = root.join(&file.path);
            if path != kept {
                if let Err(error) = remove_if_there(&path) {
                    set_aside(&path).with_context(|| format!("{error:#}"))?;
                }
            }
        }
    }
    remove_if_there(record)
}

/// The SHA-256 of a file, as lower-case hex.
pub fn sha256_of(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer).with_context(|| format!("read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("install-record-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_record_of_another_pin_or_a_changed_file_does_not_match() {
        let root = scratch("match");
        let library = root.join("runtime").join("lib.dll");
        fs::create_dir_all(library.parent().unwrap()).unwrap();
        fs::write(&library, b"twelve bytes").unwrap();
        let record = record_path(&root.join("runtime.zip"));
        assert_eq!(matches(&record, "https://x/v2.zip", &root), None);
        write(&record, "https://x/v2.zip", "ab", &[library.clone()], &root).unwrap();
        assert_eq!(matches(&record, "https://x/v2.zip", &root), Some(true));
        assert_eq!(matches(&record, "https://x/v3.zip", &root), Some(false));
        fs::write(&library, b"other").unwrap();
        assert_eq!(matches(&record, "https://x/v2.zip", &root), Some(false));
        remove_earlier(&record, &root, &root.join("today.dll")).unwrap();
        assert!(!library.exists() && !record.exists());
        let _ = fs::remove_dir_all(&root);
    }

    /// A matching record does not count while the mark of an install under way
    /// is beside it, and writing the record takes the mark away.
    #[test]
    fn an_install_under_way_does_not_match_until_its_record_is_written() {
        let root = scratch("under-way");
        let file = root.join("tool.dll");
        fs::write(&file, b"tool").unwrap();
        let record = record_path(&root.join("tool.zip"));
        write(&record, "https://example.invalid/tool.zip", "", &[file.clone()], &root).unwrap();
        assert_eq!(matches(&record, "https://example.invalid/tool.zip", &root), Some(true));
        begin(&record).unwrap();
        assert_eq!(matches(&record, "https://example.invalid/tool.zip", &root), Some(false));
        write(&record, "https://example.invalid/tool.zip", "abc", &[file], &root).unwrap();
        assert_eq!(matches(&record, "https://example.invalid/tool.zip", &root), Some(true));
        assert!(!installing_mark(&record).exists() && !record.with_extension("json.tmp").exists());
        // today's download replaced the file in place: the earlier record goes, the mark of today's install stays
        begin(&record).unwrap();
        remove_earlier(&record, &root, &root.join("tool.dll")).unwrap();
        assert!(installing_mark(&record).exists() && !record.exists() && root.join("tool.dll").exists());
        assert_eq!(matches(&record, "https://example.invalid/tool.zip", &root), Some(false));
        fs::remove_dir_all(&root).ok();
    }

    /// Every unpacked runtime the Windows studio pins names the file that tells
    /// its version apart: without it a runtime downloaded before records - every
    /// install of 3.7.0 - would be fetched again even at today's pin.
    #[cfg(windows)]
    #[test]
    fn every_pinned_windows_archive_names_its_key_file() {
        let archive = |url: &str| url.ends_with(".zip") || url.ends_with(".nupkg");
        let mut missing: Vec<&str> = Vec::new();
        let downloads = crate::lyrics_sync::ASSETS
            .iter()
            .chain(crate::engine_runtime::ASSETS)
            .chain([&crate::separation::ROFORMER_ENGINE, &crate::separation::ROFORMER_ENGINE_CPU]);
        // the audio.cpp server and its libraries came after records: no install of them predates one
        let since_records = |asset: &crate::downloads::Asset| asset.id.starts_with("audiocpp-");
        for asset in downloads.filter(|asset| asset.kind == crate::downloads::AssetKind::Runtime && archive(asset.url) && !since_records(asset)) {
            if key_file(asset.url).is_none() {
                missing.push(asset.url);
            }
        }
        for asset in crate::assistant_runtime::ASSETS.iter().filter(|asset| asset.kind == crate::assistant_runtime::AssetKind::Runtime && archive(asset.url)) {
            if key_file(asset.url).is_none() {
                missing.push(asset.url);
            }
        }
        assert!(missing.is_empty(), "pinned archives without a key file: {missing:#?}");
    }

    /// llama.cpp b11236 left on disk by 3.7.0 without a record is not taken for b11900; ONNX Runtime 1.30.0 is
    /// 3.7.0's pin and today's, so its files are taken as they are.
    #[test]
    fn a_3_7_0_runtime_without_a_record_is_not_taken_for_todays_pin() {
        let (_, llama) = key_file("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-win-cuda-13.4-x64.zip").unwrap();
        assert_ne!(llama, 8_917_504, "b11236's llama-server-impl.dll");
        let (_, onnx) = key_file("https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-win-x64-1.30.0.zip").unwrap();
        assert_eq!(onnx, 16_462_648, "1.30.0's onnxruntime.dll");
    }

    /// No two pinned archives share a key: a key file at its size names one build, so another build unpacked in its
    /// place is not taken for it.
    #[test]
    fn the_keys_of_different_archives_differ() {
        for (index, (url, file, size)) in KEY_FILES.iter().enumerate() {
            for (other, other_file, other_size) in &KEY_FILES[index + 1..] {
                assert!(url != other, "{url} is listed twice");
                assert!((file, size) != (other_file, other_size), "{url} and {other} share the key {file} of {size} bytes");
            }
        }
    }

    #[test]
    fn a_file_moved_aside_is_gone_at_the_next_start_and_the_work_is_left() {
        let root = scratch("aside");
        let runtime = root.join("karaoke").join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(root.join("media")).unwrap();
        let held = runtime.join("engine.dll");
        fs::write(&held, b"old").unwrap();
        let aside = set_aside(&held).unwrap();
        assert!(!held.exists() && aside.exists() && aside.parent() == held.parent());
        let song = root.join("media").join(format!("song.flac{ASIDE}x"));
        fs::write(&song, b"song").unwrap();
        let dataset = root.join("training").join("datasets").join("d1");
        fs::create_dir_all(&dataset).unwrap();
        let named_alike = dataset.join("0-My.stale-mix.wav");
        fs::write(&named_alike, b"song").unwrap();
        sweep_aside(&root);
        assert!(!aside.exists(), "removed at the next start");
        assert!(song.exists(), "the person's work is not swept");
        assert!(named_alike.exists(), "only the names set_aside gives are swept");
    }
}
