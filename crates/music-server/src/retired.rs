//! What earlier versions of the studio downloaded into its data folder and no part of it reads any more (Speech
//! Studio's `RETIRED`): the update screen lists it with the space it frees and removes it once what takes its place
//! is downloaded, so it does not keep gigabytes that no row of the setup screen shows.

use std::fs;
use std::path::{Path, PathBuf};

use crate::update_screen::Leftover;

/// Each leftover by the name the update screen's table of changes knows it, and its paths under the studio's data
/// folder, files or folders.
const RETIRED: &[(&str, &str, &[&str])] = &[
    // the ONNX Parakeet of 3.6 and earlier; karaoke recognises on transcribe.cpp
    ("parakeet-onnx", "Parakeet (ONNX)", &["karaoke/models/parakeet", "karaoke/models/parakeet-fp32", "karaoke/models/parakeet-ultra"]),
    // Faster-Whisper (CTranslate2) of 3.7 and earlier with its CUDA 11 cuBLAS and cuDNN 8 and its models; karaoke
    // hears with CrisperWhisper on audio.cpp's server
    (
        "faster-whisper",
        "Faster-Whisper",
        &[
            "karaoke/runtime/whisper",
            "karaoke/models/whisper",
            "karaoke/runtime/whisper-faster.zip",
            "karaoke/runtime/whisper-faster.zip.json",
            "karaoke/runtime/whisper-faster.part",
            "karaoke/runtime/whisper-cublas.zip",
            "karaoke/runtime/whisper-cublas.zip.json",
            "karaoke/runtime/whisper-cublas.part",
            "karaoke/runtime/whisper-cublas.tar.xz",
            "karaoke/runtime/whisper-cublas.tar.xz.json",
            "karaoke/runtime/whisper-cublas.tar.part",
            "karaoke/runtime/whisper-cudnn.zip",
            "karaoke/runtime/whisper-cudnn.zip.json",
            "karaoke/runtime/whisper-cudnn.part",
            "karaoke/runtime/whisper-cudnn.tar.xz",
            "karaoke/runtime/whisper-cudnn.tar.xz.json",
            "karaoke/runtime/whisper-cudnn.tar.part",
        ],
    ),
    // the ONNX HT-Demucs of 3.8 and earlier; stems separate on audio.cpp's server
    ("htdemucs-onnx", "HT-Demucs (ONNX)", &["separation/models/htdemucs/htdemucs_6s_fp16.onnx"]),
    // HOT-Step's MIDI transcriber of its own folder, before the tools shared one in `hotstep`; MuScriptor runs on
    // audio.cpp's server
    ("midi-hotstep", "HOT-Step MIDI transcriber", &["midi/runtime"]),
    // ONNX Runtime on the card, CUDA and DirectML, with their CUDA and DirectML libraries, of 3.8 and earlier; tempo
    // and key run on the processor build
    (
        "ort-gpu",
        "ONNX Runtime for the graphics card (CUDA, cuDNN, DirectML)",
        &[
            "karaoke/runtime/onnx-cuda",
            "karaoke/runtime/onnx-dml",
            "karaoke/runtime/onnxruntime-cuda.zip",
            "karaoke/runtime/onnxruntime-cuda.zip.json",
            "karaoke/runtime/onnxruntime-cuda.tgz",
            "karaoke/runtime/onnxruntime-cuda.tgz.json",
            "karaoke/runtime/cuda-cublas.whl",
            "karaoke/runtime/cuda-cublas.whl.json",
            "karaoke/runtime/cuda-cublas.zip",
            "karaoke/runtime/cuda-cublas.zip.json",
            "karaoke/runtime/cuda-cudart.zip",
            "karaoke/runtime/cuda-cudart.zip.json",
            "karaoke/runtime/cuda-cudart.tar.xz",
            "karaoke/runtime/cuda-cudart.tar.xz.json",
            "karaoke/runtime/cuda-cufft.zip",
            "karaoke/runtime/cuda-cufft.zip.json",
            "karaoke/runtime/cuda-cufft.tar.xz",
            "karaoke/runtime/cuda-cufft.tar.xz.json",
            "karaoke/runtime/cuda-curand.tar.xz",
            "karaoke/runtime/cuda-curand.tar.xz.json",
            "karaoke/runtime/cuda-cudnn.zip",
            "karaoke/runtime/cuda-cudnn.zip.json",
            "karaoke/runtime/cuda-cudnn.tar.xz",
            "karaoke/runtime/cuda-cudnn.tar.xz.json",
            "karaoke/runtime/onnxruntime-directml.nupkg",
            "karaoke/runtime/onnxruntime-directml.nupkg.json",
            "karaoke/runtime/directml.nupkg",
            "karaoke/runtime/directml.nupkg.json",
        ],
    ),
];

/// The leftovers under `data_root`, each with the paths of it that are there.
pub fn leftovers(data_root: &Path) -> Vec<Leftover> {
    RETIRED
        .iter()
        .map(|(what, name, paths)| Leftover {
            what,
            name,
            paths: paths.iter().map(|relative| data_root.join(relative)).filter(|path| fs::symlink_metadata(path).is_ok()).collect::<Vec<PathBuf>>(),
        })
        .collect()
}

/// Removes the leftovers named in `what`: the update screen's last step, after the downloads that take their place.
/// A link (another program's folder linked in) loses only the link; a hard-linked file stays with the program that
/// shares it.
pub fn remove_leftovers(data_root: &Path, what: &[&str]) {
    for leftover in leftovers(data_root).into_iter().filter(|leftover| what.contains(&leftover.what)) {
        for path in leftover.paths {
            let Ok(meta) = fs::symlink_metadata(&path) else { continue };
            let removed = if meta.file_type().is_symlink() {
                fs::remove_dir(&path).or_else(|_| fs::remove_file(&path))
            } else if meta.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            match removed {
                Ok(()) => studio_log!("removed the retired {}", path.display()),
                Err(error) => studio_log!("[ERROR] could not remove the retired {}: {error}", path.display()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leftovers_wait_for_the_update_screen_and_go_by_name() {
        let root = tempfile::tempdir().unwrap();
        for folder in ["karaoke/models/parakeet", "karaoke/models/parakeet-fp32", "karaoke/models/parakeet-ultra"] {
            fs::create_dir_all(root.path().join(folder)).unwrap();
            fs::write(root.path().join(folder).join("model.bin"), b"x").unwrap();
        }
        let whisper = root.path().join("karaoke/models/whisper/faster-whisper-small/model.bin");
        let kept = [
            root.path().join("karaoke/models/asr-gguf-parakeet-v3/parakeet-tdt-0.6b-v3-Q8_0.gguf"),
            root.path().join("karaoke/models/crisperwhisper2-turbo/model.safetensors"),
            whisper.clone(),
            root.path().join("karaoke/runtime/onnx/onnxruntime.dll"),
            root.path().join("karaoke/runtime/onnxruntime.zip.json"),
        ];
        for file in &kept {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"x").unwrap();
        }
        let onnx = root.path().join("separation/models/htdemucs/htdemucs_6s_fp16.onnx");
        fs::create_dir_all(onnx.parent().unwrap()).unwrap();
        fs::write(&onnx, b"x").unwrap();
        let gguf = root.path().join("separation/models/htdemucs/htdemucs-6stems-f16.gguf");
        fs::write(&gguf, b"x").unwrap();
        let transcriber = root.path().join("midi/runtime/music-midi");
        fs::create_dir_all(&transcriber).unwrap();
        fs::write(transcriber.join("music-midi.exe"), b"x").unwrap();
        let weights = root.path().join("midi/models/muscriptor-medium/model.safetensors");
        fs::create_dir_all(weights.parent().unwrap()).unwrap();
        fs::write(&weights, b"x").unwrap();
        for path in RETIRED.iter().find(|(what, _, _)| *what == "ort-gpu").unwrap().2 {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"x").unwrap();
        }
        let found: Vec<(&str, usize)> = leftovers(root.path()).iter().map(|leftover| (leftover.what, leftover.paths.len())).collect();
        assert_eq!(found, vec![("parakeet-onnx", 3), ("faster-whisper", 1), ("htdemucs-onnx", 1), ("midi-hotstep", 1), ("ort-gpu", 28)]);
        remove_leftovers(root.path(), &["ort-gpu"]);
        assert!(!root.path().join("karaoke/runtime/onnx-cuda").exists(), "the card's ONNX Runtime goes");
        remove_leftovers(root.path(), &["midi-hotstep"]);
        assert!(!root.path().join("midi/runtime").exists() && weights.is_file(), "the old transcriber goes, MuScriptor's weights stay");
        remove_leftovers(root.path(), &["htdemucs-onnx"]);
        assert!(!onnx.exists() && gguf.is_file(), "the ONNX model goes, the GGUF that replaced it stays");
        remove_leftovers(root.path(), &["parakeet-onnx"]);
        assert!(!root.path().join("karaoke/models/parakeet").exists());
        assert!(kept.iter().all(|file| file.is_file()), "Faster-Whisper waits for its own turn");
        remove_leftovers(root.path(), &["parakeet-onnx", "faster-whisper"]);
        assert!(!root.path().join("karaoke/models/whisper").exists());
        assert!(kept[..2].iter().all(|file| file.is_file()));
        assert!(leftovers(root.path()).iter().all(|leftover| leftover.paths.is_empty()));
    }
}
