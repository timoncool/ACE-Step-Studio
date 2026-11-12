//! The CUDA libraries the music engine is linked against.
//!
//! The engine loads one of two CUDA backends at run time, `cuda13\ggml-cuda.dll`
//! or `cuda12\ggml-cuda.dll`, whichever the card and its driver run (see
//! `cuda_build`). Each imports its own cuBLAS, `cublas64_13.dll` or
//! `cublas64_12.dll`, which imports `cublasLt64_1x.dll`. The loader resolves
//! them from the executable's folder, so they go beside `ace-server.exe`, and
//! the two majors never clash by name. A missing cuBLAS fails the backend
//! load, and the engine stops on it.
//!
//! The two libraries are 512 MB unpacked, which is four times the rest of the
//! installer, so they are not shipped - they are fetched from NVIDIA's own
//! redistributable archive the first time the engine is asked to start, the
//! same way the models are. The build machine has them on PATH through the CUDA
//! Toolkit, which is exactly why this was invisible until someone without the
//! Toolkit ran the release.
//!
//! On Linux `cuda13/libggml-cuda.so` and `cuda12/libggml-cuda.so` need the CUDA
//! runtime as well as cuBLAS; both come from NVIDIA's wheels into
//! `<data>/engine-runtime/cuda`, which the studio puts on LD_LIBRARY_PATH.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::cuda_build::CudaBuild;
use crate::downloads::{Asset, AssetKind, Downloader};

/// The libraries of each build, by file name, exactly as its ggml-cuda.dll
/// imports them. The CUDA major is part of the name, so a rebuild on another
/// major is a change here, and the dependency check below catches it.
pub const CUDA13_LIBRARIES: [&str; 2] = ["cublas64_13.dll", "cublasLt64_13.dll"];
pub const CUDA12_LIBRARIES: [&str; 2] = ["cublas64_12.dll", "cublasLt64_12.dll"];

/// The Visual C++ runtime the engine, ggml and the ONNX Runtime the studio
/// loads are compiled against, by the names in their import tables:
/// onnxruntime.dll imports msvcp140_1.dll.
pub const VC_RUNTIME_LIBRARIES: [&str; 5] =
    ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll", "msvcp140_1.dll", "vcomp140.dll"];


#[cfg(not(target_os = "linux"))]
pub const ASSETS: &[Asset] = &[CUBLAS13, CUBLAS12];
#[cfg(target_os = "linux")]
pub const ASSETS: &[Asset] = &[LINUX_CUDA13[0], LINUX_CUDA13[1], LINUX_CUDA12[0], LINUX_CUDA12[1]];

const CUBLAS13: Asset = Asset {
    id: "engine-cuda-cublas",
    label: "NVIDIA cuBLAS 13.5",
    kind: AssetKind::Runtime,
    // NVIDIA's redistributable archive, the same one every CUDA application
    // ships from. The size and the digest are published in
    // redist/redistrib_13.3.0.json; the size below was confirmed against a
    // live HEAD request, which also confirmed range support.
    url: "https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-13.5.1.27-archive.zip",
    relative_path: "cuda-cublas.zip",
    bytes: 391_055_517,
    // No sub-directory: the libraries go straight in beside ace-server.exe,
    // which is where the Windows loader looks first and what NVIDIA's own
    // deployment guide recommends.
    unzip_into: None,
    marker: "cublasLt64_13.dll",
    pick: &CUDA13_LIBRARIES,
    vram_gb: None,
    note: "The linear algebra the engine's CUDA backend is linked against. Without it the engine cannot start at all.",
};

const CUBLAS12: Asset = Asset {
    id: "engine-cuda12-cublas",
    label: "NVIDIA cuBLAS 12.9",
    kind: AssetKind::Runtime,
    // The cuBLAS of the CUDA 12.9 toolkit the CUDA 12 backend is built with.
    // Size from redist/redistrib_12.9.1.json, confirmed by a HEAD request.
    url: "https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-12.9.1.4-archive.zip",
    relative_path: "cuda12-cublas.zip",
    bytes: 549_755_186,
    unzip_into: None,
    marker: "cublasLt64_12.dll",
    pick: &CUDA12_LIBRARIES,
    vram_gb: None,
    note: "The linear algebra of the engine's CUDA 12 backend, for cards CUDA 13 dropped and older drivers.",
};

/// The cuBLAS a CUDA build loads.
pub fn cublas_asset(build: CudaBuild) -> &'static Asset {
    match build {
        CudaBuild::Cuda13 => &CUBLAS13,
        CudaBuild::Cuda12 => &CUBLAS12,
    }
}


/// Linux: the CUDA runtime and cuBLAS of the engine's CUDA 13 backend, from NVIDIA's manylinux wheels, which the
/// audio.cpp server's CUDA 13 build loads too: one copy in `<data>/engine-runtime/cuda`, named in LD_LIBRARY_PATH.
pub const LINUX_CUDA13: [Asset; 2] = [
    Asset {
        id: "engine-cuda-cudart-linux",
        label: "NVIDIA CUDA runtime 13.4",
        kind: AssetKind::Runtime,
        url: "https://files.pythonhosted.org/packages/98/8a/3431271f6344874b8f1ac03f16b3d679c91493f8da63f716160403e6d0a0/nvidia_cuda_runtime-13.4.92-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl",
        relative_path: "cuda13-cudart.whl",
        bytes: 2_494_438,
        unzip_into: None,
        marker: "libcudart.so.13",
        pick: &["libcudart.so.13"],
        vram_gb: None,
        note: "The CUDA runtime of the engine's CUDA 13 backend and of the audio.cpp server.",
    },
    Asset {
        id: "engine-cuda-cublas-linux",
        label: "NVIDIA cuBLAS 13.8",
        kind: AssetKind::Runtime,
        url: "https://files.pythonhosted.org/packages/7a/38/bdd540bf511d2c9b6f9efc71a81c60cb88e295be0b9312b61d19bbed2212/nvidia_cublas-13.8.0.4-py3-none-manylinux_2_27_x86_64.whl",
        relative_path: "cuda13-cublas.whl",
        bytes: 439_317_144,
        unzip_into: None,
        marker: "libcublasLt.so.13",
        pick: &["libcublas.so.13", "libcublasLt.so.13"],
        vram_gb: None,
        note: "The linear algebra of the engine's CUDA 13 backend and of the audio.cpp server.",
    },
];

/// Linux: the CUDA runtime and cuBLAS of the engine's CUDA 12 backend, the CUDA 12.9 wheels the audio.cpp server's
/// CUDA 12 build loads too; their names carry the major, so they share the folder with the CUDA 13 ones.
pub const LINUX_CUDA12: [Asset; 2] = [
    Asset {
        id: "engine-cuda12-cudart-linux",
        label: "NVIDIA CUDA runtime 12.9",
        kind: AssetKind::Runtime,
        url: "https://files.pythonhosted.org/packages/bc/46/a92db19b8309581092a3add7e6fceb4c301a3fd233969856a8cbf042cd3c/nvidia_cuda_runtime_cu12-12.9.79-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl",
        relative_path: "cuda12-cudart.whl",
        bytes: 3_493_179,
        unzip_into: None,
        marker: "libcudart.so.12",
        pick: &["libcudart.so.12"],
        vram_gb: None,
        note: "The CUDA runtime of the engine's CUDA 12 backend and of the audio.cpp server.",
    },
    Asset {
        id: "engine-cuda12-cublas-linux",
        label: "NVIDIA cuBLAS 12.9",
        kind: AssetKind::Runtime,
        url: "https://files.pythonhosted.org/packages/cb/c0/0a517bfe63ccd3b92eb254d264e28fca3c7cab75d07daea315250fb1bf73/nvidia_cublas_cu12-12.9.2.10-py3-none-manylinux_2_27_x86_64.whl",
        relative_path: "cuda12-cublas.whl",
        bytes: 581_240_110,
        unzip_into: None,
        marker: "libcublasLt.so.12",
        pick: &["libcublas.so.12", "libcublasLt.so.12"],
        vram_gb: None,
        note: "The linear algebra of the engine's CUDA 12 backend and of the audio.cpp server.",
    },
];

/// The downloads a CUDA build of the engine needs on this platform: cuBLAS beside the engine on Windows; on Linux the
/// CUDA runtime and cuBLAS of either build, in one folder the audio.cpp server shares.
pub fn cuda_assets(build: CudaBuild) -> Vec<&'static Asset> {
    if cfg!(target_os = "linux") {
        return match build {
            CudaBuild::Cuda13 => LINUX_CUDA13.iter().collect(),
            CudaBuild::Cuda12 => LINUX_CUDA12.iter().collect(),
        };
    }
    vec![cublas_asset(build)]
}

/// The engine's downloadable runtime.
pub struct EngineRuntime {
    downloader: Downloader,
    /// Where the engine and the Visual C++ runtime shipped with it live.
    bundle_root: PathBuf,
}

impl EngineRuntime {
    /// Takes the directory `ace-server.exe` lives in: the libraries belong
    /// beside the binary that imports them, not in a folder of their own.
    /// Where that directory cannot be written - an installation under Program
    /// Files - they go to the studio's data folder instead, which joins PATH:
    /// the engine inherits it, and Windows looks there for what its backend imports.
    /// On Linux they always go to `<data>/engine-runtime/cuda`, which joins LD_LIBRARY_PATH for the engine and the
    /// audio.cpp server alike: the package's own folder is read-only there.
    pub fn new(bundle_root: &Path, data_root: &Path) -> Self {
        if cfg!(target_os = "linux") {
            let folder = data_root.join("engine-runtime").join("cuda");
            let existing = std::env::var_os("LD_LIBRARY_PATH").unwrap_or_default();
            if let Ok(joined) = std::env::join_paths(std::iter::once(folder.clone()).chain(std::env::split_paths(&existing))) {
                unsafe { std::env::set_var("LD_LIBRARY_PATH", joined) };
            }
            return Self { downloader: Downloader::new(folder), bundle_root: bundle_root.to_path_buf() };
        }
        let root = if writable(bundle_root) {
            bundle_root.to_path_buf()
        } else {
            let elsewhere = data_root.join("engine-runtime");
            let existing = std::env::var_os("PATH").unwrap_or_default();
            if let Ok(joined) = std::env::join_paths(std::iter::once(elsewhere.clone()).chain(std::env::split_paths(&existing))) {
                unsafe { std::env::set_var("PATH", joined) };
            }
            elsewhere
        };
        Self { downloader: Downloader::new(root), bundle_root: bundle_root.to_path_buf() }
    }

    pub fn downloader(&self) -> &Downloader {
        &self.downloader
    }

    /// Where the libraries live once installed - beside the engine.
    pub fn library_dir(&self) -> PathBuf {
        self.downloader.root().to_path_buf()
    }

    /// Whether the engine will find every library it imports.
    ///
    /// The Visual C++ runtime counts too: a machine that already has cuBLAS
    /// but no redistributable has nothing to download and still cannot start
    /// the engine, and checking only the downloads would have skipped the
    /// install entirely.
    pub fn is_ready(&self, build: CudaBuild) -> bool {
        self.missing(build).is_empty() && self.vc_runtime_missing().is_empty()
    }

    /// The Visual C++ runtime ships inside the engine bundle, app-local as
    /// Microsoft permits, so the studio never installs anything into the
    /// system. A machine that has it on its search path is fine either way.
    #[cfg(windows)]
    pub fn vc_runtime_missing(&self) -> Vec<&'static str> {
        VC_RUNTIME_LIBRARIES
            .iter()
            .copied()
            .filter(|library| !self.bundle_root.join(library).is_file() && !is_on_the_search_path(library))
            .collect()
    }

    /// Off Windows the engine is built against the platform's own runtime
    /// (libc++ and the system libraries), which is always present: nothing to
    /// check, nothing to fetch.
    #[cfg(not(windows))]
    pub fn vc_runtime_missing(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// What is still missing, so a caller can report the size before starting.
    ///
    /// A machine that already has the libraries on its search path - a CUDA
    /// Toolkit installation - downloads nothing: the engine inherits that path
    /// and finds them there.
    pub fn missing(&self, build: CudaBuild) -> Vec<&'static Asset> {
        cuda_assets(build)
            .into_iter()
            .filter(|asset| !self.downloader.is_installed(asset) && !asset.pick.iter().all(|library| is_on_the_search_path(library)))
            .collect()
    }

    /// Fetches whatever is missing and waits for it.
    pub async fn install_missing(&self, build: CudaBuild) -> Result<()> {
        let absent = self.vc_runtime_missing();
        if !absent.is_empty() {
            bail!("the engine bundle is incomplete: {} missing beside the engine; reinstall the studio", absent.join(", "));
        }
        self.downloader.install_all("engine", &self.missing(build)).await
    }
}

/// Whether files can be written into `directory`, found by writing one.
fn writable(directory: &Path) -> bool {
    if std::fs::create_dir_all(directory).is_err() {
        return false;
    }
    let probe = directory.join(format!(".write-probe-{}", std::process::id()));
    let written = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    written
}

/// Whether the loader would already find this library without help: the engine inherits this process's PATH (on
/// Linux its LD_LIBRARY_PATH), so a machine with the CUDA Toolkit there downloads nothing.
fn is_on_the_search_path(library: &str) -> bool {
    let variable = if cfg!(windows) { "PATH" } else { "LD_LIBRARY_PATH" };
    let Some(path) = std::env::var_os(variable) else { return false };
    std::env::split_paths(&path).any(|directory| directory.join(library).is_file())
}

/// What a binary needs that is neither beside it nor supplied by Windows.
///
/// Follows the chain: `ace-server.exe` imports `ggml.dll`, which imports
/// `ggml-cuda.dll`, which is where cuBLAS actually comes in. Checking only the
/// executable's own imports would have found nothing wrong with the release
/// that could not start.
pub fn unresolved_dependencies(directory: &Path, entry_point: &str) -> Result<Vec<String>> {
    unresolved_in(&[directory.to_path_buf()], &directory.join(entry_point))
}

/// What `entry_point` needs that none of `directories` holds and Windows does
/// not supply: a downloaded tool resolves beside itself first, then in the
/// engine's folder, which the studio puts on its PATH.
pub fn unresolved_in(directories: &[PathBuf], entry_point: &Path) -> Result<Vec<String>> {
    dub_core::compat::unresolved_in(directories, &[], entry_point).map_err(anyhow::Error::msg)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libraries_go_to_the_data_folder_when_the_engine_folder_cannot_be_written() {
        let data = std::env::temp_dir().join(format!("engine-runtime-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("a-file"), b"").unwrap();
        // a folder inside a file can be neither made nor written, as Program Files cannot without elevation
        let runtime = EngineRuntime::new(&data.join("a-file").join("engine"), &data);
        assert_eq!(runtime.library_dir(), data.join("engine-runtime"));
        let writable = data.join("install");
        std::fs::create_dir_all(&writable).unwrap();
        assert_eq!(EngineRuntime::new(&writable, &data).library_dir(), writable);
        std::fs::remove_dir_all(&data).ok();
    }

    /// The file names are the contract with the engine binary: they are what
    /// the import table asks the loader for, and what `pick` takes out of the
    /// archive. A typo here is a download that finishes and changes nothing.
    #[test]
    fn every_imported_library_is_picked_out_of_the_archive() {
        for (build, libraries, major) in
            [(CudaBuild::Cuda13, CUDA13_LIBRARIES, "13"), (CudaBuild::Cuda12, CUDA12_LIBRARIES, "12")]
        {
            let asset = cublas_asset(build);
            for library in libraries {
                assert!(library.contains(major), "{library} does not name CUDA {major}");
                assert!(asset.pick.contains(&library), "{library} is imported by {build:?} but never taken out of its archive");
            }
            assert!(asset.url.contains(&format!("-{major}.")), "{} is not a CUDA {major} cuBLAS", asset.url);
        }
    }

    /// A runtime asset that unpacks nowhere would download and vanish.
    #[test]
    fn the_libraries_land_in_one_directory_the_engine_can_be_pointed_at() {
        for asset in ASSETS {
            // Straight into the bundle: a sub-directory would put them
            // somewhere the loader does not look.
            assert_eq!(asset.unzip_into, None);
            assert_eq!(asset.kind, AssetKind::Runtime);
            assert!(asset.bytes > 0, "{} has no size, so nothing can report progress", asset.id);
        }
    }


    /// On Linux each CUDA build of the engine gets the CUDA runtime and cuBLAS of its own major from NVIDIA's wheels;
    /// on Windows its cuBLAS archive.
    #[test]
    fn each_cuda_build_downloads_the_libraries_of_its_own_major() {
        for (build, linux, major) in [(CudaBuild::Cuda13, &LINUX_CUDA13, "13"), (CudaBuild::Cuda12, &LINUX_CUDA12, "12")] {
            for asset in linux {
                assert!(asset.url.ends_with(".whl"), "{} is not a wheel", asset.id);
                assert!(asset.pick.iter().all(|library| library.ends_with(&format!(".so.{major}"))), "{} picks another major", asset.id);
            }
            let picked: Vec<&str> = linux.iter().flat_map(|asset| asset.pick.iter().copied()).collect();
            for library in ["libcudart.so", "libcublas.so", "libcublasLt.so"] {
                assert!(picked.contains(&format!("{library}.{major}").as_str()), "the CUDA {major} build gets no {library}");
            }
            let ids: Vec<&str> = cuda_assets(build).iter().map(|asset| asset.id).collect();
            if cfg!(target_os = "linux") {
                assert_eq!(ids, linux.iter().map(|asset| asset.id).collect::<Vec<_>>());
            } else {
                assert_eq!(ids, vec![cublas_asset(build).id]);
            }
        }
    }

    /// The release that shipped without cuBLAS passed every test there was,
    /// because no test ever looked at what the engine binary asks the loader
    /// for. This one does: whatever the staged bundle imports is either beside
    /// it, supplied by Windows, or downloaded on first start - and nothing
    /// else is allowed.
    #[test]
    #[cfg(windows)]
    fn the_staged_engine_bundle_can_actually_be_loaded() {
        let bundle = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../desktop/src-tauri/resources/engine")
            .canonicalize();
        let Ok(bundle) = bundle else {
            // No engine has been built into the bundle on this machine, which
            // is the normal state of a checkout that only touches the service.
            return;
        };
        let executable = music_engine::server::executable_name();
        if !bundle.join(&executable).is_file() {
            return;
        }
        // The backends load at run time - the CUDA ones from their folders, Vulkan and the processor generations
        // beside the executable - and resolve their imports from the executable's folder, so each is checked
        // from there, not only what the executable imports.
        let mut entry_points = vec![executable.clone()];
        for build in [CudaBuild::Cuda13, CudaBuild::Cuda12] {
            let backend = format!("{}/ggml-cuda.dll", build.folder());
            if bundle.join(&backend).is_file() {
                entry_points.push(backend);
            }
        }
        let handled: Vec<&str> =
            CUDA13_LIBRARIES.iter().chain(CUDA12_LIBRARIES.iter()).chain(VC_RUNTIME_LIBRARIES.iter()).copied().collect();
        for entry in std::fs::read_dir(&bundle).expect("read the bundle").flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase().starts_with("ggml-") && name.to_ascii_lowercase().ends_with(".dll") {
                entry_points.push(name);
            }
        }
        for entry_point in entry_points {
            let missing = unresolved_dependencies(&bundle, &entry_point).expect("read the bundle's import tables");
            let unexpected: Vec<&String> = missing
                .iter()
                .filter(|name| !handled.iter().any(|library| library.eq_ignore_ascii_case(name)))
                .collect();
            assert!(
                unexpected.is_empty(),
                "{entry_point} imports libraries that are neither shipped nor installed on first start: {unexpected:?}. \
                 Ship them next to {executable}, or add them to this module so the studio fetches them."
            );
        }
    }

    /// Every downloaded tool finds what it imports beside itself, in the engine's
    /// folder or in Windows.
    #[test]
    #[cfg(windows)]
    #[ignore = "reads runtimes downloaded into a data folder: scripts/check-dll-imports.ps1"]
    fn downloaded_runtimes_can_be_loaded() {
        let data = PathBuf::from(std::env::var("ACE_DLL_DATA").expect("ACE_DLL_DATA names a studio data folder"));
        let engine = PathBuf::from(std::env::var("ACE_DLL_ENGINE").expect("ACE_DLL_ENGINE names the engine folder"));
        let mut checked = 0;
        let mut problems = Vec::new();
        for area in ["hotstep", "midi/runtime", "training/runtime", "separation/runtime", "karaoke/runtime", "karaoke/models", "assistant/runtime"] {
            let Ok(folders) = std::fs::read_dir(data.join(area)) else { continue };
            for folder in folders.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()) {
                for binary in std::fs::read_dir(&folder).into_iter().flatten().flatten().map(|entry| entry.path()) {
                    let extension = binary.extension().map(|extension| extension.to_string_lossy().to_ascii_lowercase());
                    if !matches!(extension.as_deref(), Some("exe" | "dll")) {
                        continue;
                    }
                    // ONNX Runtime's archive carries its TensorRT provider, which the studio never registers
                    if binary.file_name().is_some_and(|name| name.eq_ignore_ascii_case("onnxruntime_providers_tensorrt.dll")) {
                        continue;
                    }
                    // a tool resolves beside itself, then in the engine's folder on the studio's PATH
                    // the engine's cuBLAS 13 arrives with its first CUDA start; a tool on the card finds it on PATH or gets a link beside it
                    let missing: Vec<String> = unresolved_in(&[folder.clone(), engine.clone()], &binary)
                        .expect("read the import tables")
                        .into_iter()
                        .filter(|name| !CUDA13_LIBRARIES.iter().any(|library| library.eq_ignore_ascii_case(name)))
                        .collect();
                    checked += 1;
                    if !missing.is_empty() {
                        problems.push(format!("{}: {missing:?}", binary.display()));
                    }
                }
            }
        }
        assert!(checked > 0, "no downloaded runtime under {}", data.display());
        assert!(problems.is_empty(), "downloaded runtimes import libraries nothing supplies:\n{}", problems.join("\n"));
    }

    /// The libraries count as installed only once they are beside the engine.
    ///
    /// This asserts on the downloader rather than on `is_ready`, because a
    /// machine with the CUDA Toolkit is ready without downloading anything -
    /// which is the point of the PATH check, and would make the test lie about
    /// what it proved.
    #[test]
    fn the_libraries_count_as_installed_only_beside_the_engine() {
        let root = std::env::temp_dir().join(format!("engine-runtime-{}", uuid::Uuid::now_v7()));
        let runtime = EngineRuntime::new(&root, &root);
        let asset = cublas_asset(CudaBuild::Cuda13);
        assert!(!runtime.downloader().is_installed(asset));
        assert_eq!(runtime.library_dir(), root);

        std::fs::create_dir_all(runtime.library_dir()).unwrap();
        for library in CUDA13_LIBRARIES {
            std::fs::write(runtime.library_dir().join(library), b"x").unwrap();
        }
        assert!(!runtime.downloader().is_installed(asset), "libraries without the record of their pin are not taken for it");
        let unpacked: Vec<_> = CUDA13_LIBRARIES.iter().map(|library| runtime.library_dir().join(library)).collect();
        crate::install_record::write(&runtime.downloader().record_of(asset), asset.url, "", &unpacked, runtime.downloader().root()).unwrap();
        assert!(runtime.downloader().is_installed(asset));
        assert!(runtime.missing(CudaBuild::Cuda13).is_empty());
        // The other build asks for its own cuBLAS, unless PATH has it.
        if !CUDA12_LIBRARIES.iter().all(|library| is_on_the_search_path(library)) {
            assert_eq!(runtime.missing(CudaBuild::Cuda12).len(), 1);
        }
        std::fs::remove_dir_all(&root).ok();
    }
}
