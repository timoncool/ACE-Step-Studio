//! audio.cpp's `audiocpp_server`: CrisperWhisper for karaoke, HT-Demucs stems and MuScriptor audio to MIDI, through
//! Dub Studio's crate `acpp-server` as Speech Studio and Dub Studio run it. One installation, in the karaoke folder
//! where the studio first put it; one process per build carries every model whose files are on disk, so a tool never
//! restarts the server another one started, and the server ends when its last run does.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use acpp_server::asset::Build;
use acpp_server::{AcppError, ModelEntry, Server, ServerConfig};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use crate::downloads::{Asset, AssetKind, Downloader};
use crate::lyrics_sync::OnnxFlavour;

/// How long a start may take before the server answers `/health` (it loads no model at start).
const READY_WITHIN: Duration = Duration::from_secs(60);

fn exe_name() -> &'static str {
    if cfg!(windows) { "audiocpp_server.exe" } else { "audiocpp_server" }
}

/// The server's builds and the CUDA libraries they do not carry, the shared client's pins (`acpp_server::asset`), one
/// folder a build under `runtime`, as Dub Studio and Speech Studio lay them out.
pub const ASSETS: &[Asset] = &[
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda13",
        label: "audio.cpp server (CUDA 13)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_WIN_CUDA13.url,
        relative_path: "runtime/audiocpp-cuda13.zip",
        bytes: acpp_server::asset::SERVER_WIN_CUDA13.size,
        unzip_into: Some(acpp_server::asset::Build::Cuda13.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on an NVIDIA card from the GTX 16 and RTX 20 series on.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda13-cufft",
        label: "NVIDIA cuFFT 12.4 (for the audio.cpp server)",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-12.4.0.43-archive.zip",
        relative_path: "runtime/audiocpp-cuda13-cufft.zip",
        bytes: 159_602_693,
        unzip_into: Some(acpp_server::asset::Build::Cuda13.folder()),
        marker: "cufft64_12.dll",
        pick: &["cufft64_12.dll"],
        vram_gb: None,
        note: "The Fourier transforms of the CUDA 13 server; its cuBLAS is the music engine's.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda12",
        label: "audio.cpp server (CUDA 12)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_WIN_CUDA12.url,
        relative_path: "runtime/audiocpp-cuda12.zip",
        bytes: acpp_server::asset::SERVER_WIN_CUDA12.size,
        unzip_into: Some(acpp_server::asset::Build::Cuda12.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on the NVIDIA cards and drivers CUDA 13 does not.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda12-cudart",
        label: "NVIDIA CUDA runtime 12.9 (for the audio.cpp server)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::CUDA12_RUNTIME[0].url,
        relative_path: "runtime/audiocpp-cuda12-cudart.whl",
        bytes: acpp_server::asset::CUDA12_RUNTIME[0].size,
        unzip_into: Some(acpp_server::asset::Build::Cuda12.folder()),
        marker: "cudart64_12.dll",
        pick: &["cudart64_12.dll"],
        vram_gb: None,
        note: "Part of the CUDA 12 server.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda12-cublas",
        label: "NVIDIA cuBLAS 12.9 (for the audio.cpp server)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::CUDA12_RUNTIME[1].url,
        relative_path: "runtime/audiocpp-cuda12-cublas.whl",
        bytes: acpp_server::asset::CUDA12_RUNTIME[1].size,
        unzip_into: Some(acpp_server::asset::Build::Cuda12.folder()),
        marker: "cublasLt64_12.dll",
        pick: &["cublas64_12.dll", "cublasLt64_12.dll"],
        vram_gb: None,
        note: "Part of the CUDA 12 server.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cuda12-cufft",
        label: "NVIDIA cuFFT 11.4 (for the audio.cpp server)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::CUDA12_RUNTIME[2].url,
        relative_path: "runtime/audiocpp-cuda12-cufft.whl",
        bytes: acpp_server::asset::CUDA12_RUNTIME[2].size,
        unzip_into: Some(acpp_server::asset::Build::Cuda12.folder()),
        marker: "cufft64_11.dll",
        pick: &["cufft64_11.dll"],
        vram_gb: None,
        note: "Part of the CUDA 12 server.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-vulkan",
        label: "audio.cpp server (Vulkan)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_WIN_VULKAN.url,
        relative_path: "runtime/audiocpp-vulkan.zip",
        bytes: acpp_server::asset::SERVER_WIN_VULKAN.size,
        unzip_into: Some(acpp_server::asset::Build::Vulkan.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on an AMD or Intel card.",
    },
    #[cfg(windows)]
    Asset {
        id: "audiocpp-cpu",
        label: "audio.cpp server (processor)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_WIN_CPU.url,
        relative_path: "runtime/audiocpp-cpu.zip",
        bytes: acpp_server::asset::SERVER_WIN_CPU.size,
        unzip_into: Some(acpp_server::asset::Build::Cpu.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on the processor: slower, the same words.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "audiocpp-cuda13",
        label: "audio.cpp server (CUDA 13)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_LINUX_CUDA13.url,
        relative_path: "runtime/audiocpp-cuda13.tar.gz",
        bytes: acpp_server::asset::SERVER_LINUX_CUDA13.size,
        unzip_into: Some(acpp_server::asset::Build::Cuda13.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on an NVIDIA card from the GTX 16 and RTX 20 series on.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "audiocpp-cuda13-cufft",
        label: "NVIDIA cuFFT 12.4 (for the audio.cpp server)",
        kind: AssetKind::Runtime,
        url: "https://developer.download.nvidia.com/compute/cuda/redist/libcufft/linux-x86_64/libcufft-linux-x86_64-12.4.0.43-archive.tar.xz",
        relative_path: "runtime/audiocpp-cuda13-cufft.tar.xz",
        bytes: 249_327_988,
        unzip_into: Some(acpp_server::asset::Build::Cuda13.folder()),
        marker: "libcufft.so.12",
        pick: &["libcufft.so.12"],
        vram_gb: None,
        note: "The Fourier transforms of the CUDA 13 server; its cuBLAS is the music engine's.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "audiocpp-vulkan",
        label: "audio.cpp server (Vulkan)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_LINUX_VULKAN.url,
        relative_path: "runtime/audiocpp-vulkan.tar.gz",
        bytes: acpp_server::asset::SERVER_LINUX_VULKAN.size,
        unzip_into: Some(acpp_server::asset::Build::Vulkan.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on an AMD or Intel card.",
    },
    #[cfg(target_os = "linux")]
    Asset {
        id: "audiocpp-cpu",
        label: "audio.cpp server (processor)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_LINUX_CPU.url,
        relative_path: "runtime/audiocpp-cpu.tar.gz",
        bytes: acpp_server::asset::SERVER_LINUX_CPU.size,
        unzip_into: Some(acpp_server::asset::Build::Cpu.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on the processor: slower, the same words.",
    },
    #[cfg(target_os = "macos")]
    Asset {
        id: "audiocpp-metal",
        label: "audio.cpp server (Metal)",
        kind: AssetKind::Runtime,
        url: acpp_server::asset::SERVER_MACOS.url,
        relative_path: "runtime/audiocpp-metal.tar.gz",
        bytes: acpp_server::asset::SERVER_MACOS.size,
        unzip_into: Some(acpp_server::asset::Build::Metal.folder()),
        marker: "audiocpp_server",
        pick: &[],
        vram_gb: None,
        note: "Runs CrisperWhisper on Apple silicon: Metal and the processor in one program.",
    },
];

pub fn asset(id: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|asset| asset.id == id)
}

/// The downloads of a build, the program first: the CUDA builds carry no cuFFT and, on CUDA 12, no runtime of their own.
pub fn server_ids(build: Build) -> &'static [&'static str] {
    match build {
        Build::Cuda13 => &["audiocpp-cuda13", "audiocpp-cuda13-cufft"],
        Build::Cuda12 => &["audiocpp-cuda12", "audiocpp-cuda12-cudart", "audiocpp-cuda12-cublas", "audiocpp-cuda12-cufft"],
        Build::Vulkan => &["audiocpp-vulkan"],
        Build::Cpu => &["audiocpp-cpu"],
        Build::Metal => &["audiocpp-metal"],
    }
}

/// The build of the server for `runtime`: the processor when it is chosen; else the card's CUDA build (on Linux CUDA 13
/// alone, as the engine ships it), Vulkan on a card CUDA does not run (AMD, Intel), the processor without a card; a Mac
/// its Metal program.
pub fn build_for(runtime: OnnxFlavour) -> Result<Build> {
    if let Some(mac) = Build::for_this_mac() {
        return Ok(mac);
    }
    if cfg!(target_os = "macos") {
        bail!("the audio.cpp server is published for Apple silicon only");
    }
    if matches!(runtime, OnnxFlavour::Cpu) {
        return Ok(Build::Cpu);
    }
    Ok(match crate::cuda_build::current() {
        Some(crate::cuda_build::CudaBuild::Cuda13) => Build::Cuda13,
        Some(crate::cuda_build::CudaBuild::Cuda12) if cfg!(windows) => Build::Cuda12,
        _ if crate::presets::display_card().is_some() => Build::Vulkan,
        _ => Build::Cpu,
    })
}

/// Whether a build computes on the graphics card.
pub fn on_card(build: Build) -> bool {
    !matches!(build, Build::Cpu)
}

/// Windows looks for a library's imports beside it: a CUDA 13 build of a program the studio runs gets links (copies
/// across drives) to the music engine's cuBLAS 13 in `source`, unless PATH already has them.
pub fn link_cuda13(source: Option<&Path>, engine_dir: &Path, who: &str) -> Result<()> {
    if !cfg!(windows) {
        return Ok(());
    }
    let on_path = |name: &str| std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()));
    for name in crate::engine_runtime::CUDA13_LIBRARIES {
        let target = engine_dir.join(name);
        if target.is_file() || on_path(name) {
            continue;
        }
        let Some(source) = source.map(|folder| folder.join(name)).filter(|source| source.is_file()) else {
            bail!("{who} on the card needs {name}, which comes with the music engine's CUDA 13 runtime: download it in Settings - Local engine, or choose the processor");
        };
        if std::fs::hard_link(&source, &target).is_err() {
            std::fs::copy(&source, &target).with_context(|| format!("copy {} beside {who}", source.display()))?;
        }
    }
    Ok(())
}

/// A model the server may carry, with the files it is complete with.
struct Known {
    entry: ModelEntry,
    files: Vec<PathBuf>,
}

/// The audio.cpp server of the studio: its downloads in the karaoke folder (the karaoke downloader's), its logs beside.
pub struct Acpp {
    downloader: Downloader,
    /// cuBLAS 13 of the music engine, which the CUDA 13 build imports.
    cuda13_libraries: OnceLock<PathBuf>,
    /// Every model the server may carry. One server per build serves them all: a config that names only the model of
    /// the run at hand would restart the server whenever another tool asks for its own.
    known: std::sync::Mutex<Vec<Known>>,
    /// Runs at work on the servers.
    active: std::sync::Mutex<usize>,
}

impl Acpp {
    pub fn new(downloader: Downloader) -> Self {
        Self { downloader, cuda13_libraries: OnceLock::new(), known: std::sync::Mutex::new(Vec::new()), active: std::sync::Mutex::new(0) }
    }

    /// The folder of the cuBLAS 13 the music engine runs on.
    pub fn set_cuda13_libraries(&self, folder: PathBuf) {
        self.cuda13_libraries.set(folder).ok();
    }

    pub fn cuda13_libraries(&self) -> Option<&Path> {
        self.cuda13_libraries.get().map(PathBuf::as_path)
    }

    /// A model the server may carry, complete once every one of `files` is on disk, in place of an earlier one of the
    /// same id.
    pub fn register(&self, entry: ModelEntry, files: Vec<PathBuf>) {
        let mut known = self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        known.retain(|model| model.entry.id != entry.id);
        known.push(Known { entry, files });
    }

    /// The registered models whose files are all on disk now: the server reads each configured model when it starts,
    /// and one missing file would stop all of them.
    fn models_on_disk(&self) -> Vec<ModelEntry> {
        let known = self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut models: Vec<ModelEntry> = known.iter().filter(|model| model.files.iter().all(|file| file.is_file())).map(|model| model.entry.clone()).collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        models
    }

    pub fn folder(&self, build: Build) -> PathBuf {
        self.downloader.runtime_dir(build.folder())
    }

    /// The program of a build, when it is downloaded.
    pub fn exe(&self, build: Build) -> Option<PathBuf> {
        let exe = self.folder(build).join(exe_name());
        exe.is_file().then_some(exe)
    }

    /// The downloads of a build not installed yet.
    pub fn missing(&self, build: Build) -> Vec<&'static Asset> {
        server_ids(build).iter().filter_map(|id| asset(id)).filter(|asset| !self.downloader.is_installed(asset)).collect()
    }

    pub fn installed(&self, build: Build) -> bool {
        self.missing(build).is_empty()
    }

    /// The running server of this build carrying `model` (registered with its files) and every other registered model
    /// on disk: the one there is, or a new one in its place when the models on disk changed or the process ended. An
    /// error, before the program starts, when the program, a CUDA library it imports or `model` is missing.
    fn server(&self, build: Build, model: ModelEntry, files: Vec<PathBuf>) -> Result<Arc<Server>> {
        let id = model.id.clone();
        self.register(model, files);
        let models = self.models_on_disk();
        if !models.iter().any(|entry| entry.id == id) {
            bail!("{id} is not completely downloaded; download it in Settings - Models");
        }
        let missing = self.missing(build);
        if !missing.is_empty() {
            bail!("the audio.cpp server ({}) is not downloaded: {} missing; download it in Settings - Models", build.folder(), missing.iter().map(|asset| asset.id).collect::<Vec<_>>().join(", "));
        }
        let folder = self.folder(build);
        let exe = self.exe(build).ok_or_else(|| anyhow!("the audio.cpp server ({}) has no {} in {}", build.folder(), exe_name(), folder.display()))?;
        if build == Build::Cuda13 {
            link_cuda13(self.cuda13_libraries(), &folder, "the audio.cpp server")?;
        }
        // Windows looks beside the program, then on PATH, where the engine's folder and its cuBLAS 13 are
        let path: Vec<PathBuf> = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect()).unwrap_or_default();
        let searched: Vec<&Path> = std::iter::once(folder.as_path()).chain(path.iter().map(PathBuf::as_path)).collect();
        let libraries = acpp_server::asset::missing_libraries(build, &searched);
        if !libraries.is_empty() {
            bail!("the audio.cpp server misses {} beside it in {}", libraries.join(", "), folder.display());
        }
        let config = ServerConfig {
            exe,
            backend: build.backend(),
            device: 0,
            threads: std::thread::available_parallelism().map(|threads| threads.get() as u32).unwrap_or(8).min(16),
            models,
            data_dir: self.downloader.root().join("logs").join(build.folder()),
            log: false,
        };
        // a Linux loader finds the engine's cuBLAS 13 for the CUDA build where the engine keeps it
        let libraries = [folder.clone(), crate::engine_bundle_root()];
        let prepare = move |command: &mut std::process::Command| {
            // the engine's folder lies inside the image too: it goes on after the image's folders are taken off
            dub_core::proc::outside_appimage(command);
            if cfg!(target_os = "linux") {
                let inherited = match command.get_envs().find(|(key, _)| *key == "LD_LIBRARY_PATH") {
                    Some((_, value)) => value.map(std::ffi::OsStr::to_os_string),
                    None => std::env::var_os("LD_LIBRARY_PATH"),
                };
                if let Ok(path) = std::env::join_paths(libraries.iter().cloned().chain(inherited.iter().flat_map(std::env::split_paths))) {
                    command.env("LD_LIBRARY_PATH", path);
                }
            }
        };
        let server = acpp_server::SERVERS.get(&config, READY_WITHIN, &prepare).map_err(|error| anyhow!("the audio.cpp server did not start: {}", describe(&error)))?;
        studio_log!("audiocpp_server {} ({}) carries {}", server.pid(), build.folder(), config.models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>().join(", "));
        Ok(server)
    }

    /// `work` on the server of `build` carrying `model`. When the last piece of work on the servers ends - done, failed
    /// or stopped - the server is ended: an idle server still holds a gigabyte of the card, which belongs to the music
    /// engine between runs. A piece of work counts from before its server is looked up, so one that ends meanwhile
    /// leaves the server to it.
    pub fn with_server<T>(&self, build: Build, model: ModelEntry, files: Vec<PathBuf>, work: impl FnOnce(&Server) -> Result<T>) -> Result<T> {
        *self.active.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
        let outcome = self.server(build, model, files).and_then(|server| {
            let done = work(&server);
            Ok((server, done))
        });
        let last = {
            let mut active = self.active.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            *active -= 1;
            *active == 0
        };
        match outcome {
            Ok((server, done)) => {
                if last {
                    server.kill();
                }
                done
            }
            Err(error) => Err(error),
        }
    }
}

/// An error of the server said the way the studio shows it.
pub fn describe(error: &AcppError) -> String {
    match error {
        AcppError::Cancelled => "stopped".into(),
        other => other.to_string(),
    }
}

/// Runs `request` on `model` in the server of `build` (`Acpp::with_server`). `stop` is asked every tenth of a second.
pub fn run(acpp: &Acpp, build: Build, model: ModelEntry, files: Vec<PathBuf>, request: Value, stop: &(dyn Fn() -> bool + Sync)) -> Result<Value> {
    let id = model.id.clone();
    acpp.with_server(build, model, files, |server| {
        let cancel = AtomicBool::new(false);
        let finished = AtomicBool::new(false);
        let answer = std::thread::scope(|scope| {
            scope.spawn(|| {
                while !finished.load(Ordering::SeqCst) {
                    if stop() {
                        cancel.store(true, Ordering::SeqCst);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
            let answer = server.run_task(&id, request, &cancel);
            finished.store(true, Ordering::SeqCst);
            answer
        });
        match answer {
            Ok(result) => Ok(result),
            Err(AcppError::Cancelled) => bail!("stopped"),
            Err(error) => Err(anyhow!("audio.cpp's {id}: {}", describe(&error))),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_build_this_platform_publishes_has_its_downloads() {
        for build in [Build::Cuda13, Build::Cuda12, Build::Vulkan, Build::Cpu, Build::Metal] {
            let Some(pinned) = build.server() else { continue };
            let ids = server_ids(build);
            let program = asset(ids[0]).expect("the program of a published build is a download");
            assert_eq!(program.url, pinned.url);
            assert_eq!(program.unzip_into, Some(build.folder()));
            #[cfg(windows)]
            for library in build.libraries() {
                let picked = ids.iter().filter_map(|id| asset(id)).any(|asset| asset.pick.contains(library));
                let from_engine = build == Build::Cuda13 && crate::engine_runtime::CUDA13_LIBRARIES.contains(library);
                assert!(picked || from_engine, "{build:?} imports {library}, which nothing downloads");
            }
        }
    }

    #[test]
    fn a_processor_choice_runs_the_processor_build() {
        if Build::for_this_mac().is_none() {
            assert_eq!(build_for(OnnxFlavour::Cpu).unwrap(), Build::Cpu);
            assert!(!on_card(Build::Cpu));
        }
    }

    #[test]
    fn a_model_joins_the_server_only_with_all_its_files() {
        let root = tempfile::tempdir().unwrap();
        let acpp = Acpp::new(Downloader::new(root.path().to_path_buf()));
        let folder = root.path().join("models/muscriptor-medium");
        std::fs::create_dir_all(&folder).unwrap();
        let files = vec![folder.join("config.json"), folder.join("model.safetensors")];
        acpp.register(ModelEntry::muscriptor("muscriptor-medium", &folder), files.clone());
        std::fs::write(&files[0], b"{}").unwrap();
        assert!(acpp.models_on_disk().is_empty(), "a folder with one of its files is not a model yet");
        std::fs::write(&files[1], b"x").unwrap();
        assert_eq!(acpp.models_on_disk().len(), 1);
    }
}
