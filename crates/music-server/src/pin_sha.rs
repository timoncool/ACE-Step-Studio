//! The SHA-256 of every pinned download, as its publisher states it: GitHub's release asset digest, the LFS object
//! id on Hugging Face (and, for a small file kept in git there, the hash of the file whose git object id is the
//! pinned revision's), NVIDIA's redistributable manifests, PyPI's file hashes. A downloaded file whose hash differs
//! is removed and the download fails; a neighbour's file is taken only with the pin's hash.

use std::path::Path;

use anyhow::{bail, Result};

const PINNED: &[(&str, &str)] = &[
    ("https://api.nuget.org/v3-flatcontainer/microsoft.ai.directml/1.15.4/microsoft.ai.directml.1.15.4.nupkg", "4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9"),
    ("https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.24.4/microsoft.ml.onnxruntime.directml.1.24.4.nupkg", "57e9f11b73437bef7a309496135d4c1f96b1a8e9ddba60013fa27bfc1d788681"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/cuda_cudart/linux-x86_64/cuda_cudart-linux-x86_64-12.9.79-archive.tar.xz", "1f6ad42d4f530b24bfa35894ccf6b7209d2354f59101fd62ec4a6192a184ce99"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/cuda_cudart/windows-x86_64/cuda_cudart-windows-x86_64-12.9.79-archive.zip", "179e9c43b0735ffe67207b3da556eb5a0c50f3047961882b7657d3b822d34ef8"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-12.9.1.4-archive.zip", "d534d98b0b453a98914dbf3adf47d7e84b55037abf02f87466439e1dcef581ed"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-13.5.1.27-archive.zip", "c946e1c825e05895747a95ed4fee18030b08052c09783b9b7b19818fd2e31f58"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcufft/linux-x86_64/libcufft-linux-x86_64-12.4.0.43-archive.tar.xz", "f7f8ac2cc67714989a3c8140cda39b1b7ed7d462a491e1ad3f53adb777f5070f"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-12.4.0.43-archive.zip", "69d0ad8dc3a1be66f01a748a8206d0ceaafa56939474663fbe790dc3e91d2009"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/LICENSE.md", "42f43bf4dc72ef422f91f6ca143f4514e21a68b353940d99091c1addcd0aee79"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/README.md", "ddc82a301a1b59035eefcc1a8b2ec90d56257104df7365b00493c58135a6b029"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/added_tokens.json", "228ddf33a2c2b94033dc0fad8e7847b554b1c9f7d2054f5455ffd562b6a30dc7"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/config.json", "9d823d0eacddfc5ee920fac73897abfd29fbecac7d9e49b5d8e7f0bd63a56144"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/generation_config.json", "e20fb70a44ce2b3cb3b2bbbea852e064638563b072719640a286d2a6865066e1"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/merges.txt", "2df2990a395e35e8dfbc7511e08c12d56018d8d04691e0133e5d63b21e154dc6"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/model.safetensors", "0121a185b87ada7497f22dfbcd6c86b5f599b69dfcc8676e37f36447caf0b630"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/normalizer.json", "bf1c507dc8724ca9cf9903640dacfb69dae2f00edee4f21ceba106a7392f26dd"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/preprocessor_config.json", "654cf18d3e163b948ceaf9766da56ce0b52de265d58673cf61c9376f126bd499"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/special_tokens_map.json", "baea4ea09372eb4fca86b4e4346139fd73cb807d5087e9de0948e971739c3e74"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/tokenizer.json", "00e6dce8d06acafefaea8d5d1e57e89c69b3237d08ecbe2bf6eb7124e248bcde"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/tokenizer_config.json", "3089ab61ce4745667a41e91620081cd3e7d61a9ae1f79fdf91b1bc8983916f4e"),
    ("https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/vocab.json", "6788c80b082e9b0d1393147d3a3e62ba19285ac0c82ace8e5ef00f37ead58971"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcufft/linux-x86_64/libcufft-linux-x86_64-11.4.1.4-archive.tar.xz", "b0e65af59b0c2f6c8ed9f5552a9b375890855b7926ae2c0404d15dcf2565bda4"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-11.4.1.4-archive.zip", "f26f80bb9abff3269c548e1559e8c2b4ba58ccb8acc6095bbc6404fc962d4b80"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcurand/linux-x86_64/libcurand-linux-x86_64-10.3.10.19-archive.tar.xz", "48281b4caadb1cf790d44ac76b23c77d06f474c0b1799814f314aafec9258ad6"),
    ("https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/linux-x86_64/cudnn-linux-x86_64-9.25.0.15_cuda12-archive.tar.xz", "0fe244321b401758cbb8430d1997f494add139405a4a4091338bcbb6f5024fa8"),
    ("https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/windows-x86_64/cudnn-windows-x86_64-9.25.0.15_cuda12-archive.zip", "06e94f70c52d7335b7ed8044eed28ce963b7fd59d8c2c446ffc60e695fccad91"),
    ("https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl", "623f43027d40d44ceadf0043f002bd25cf353e8f13ce90b9a87057019f560661"),
    ("https://files.pythonhosted.org/packages/cb/c0/0a517bfe63ccd3b92eb254d264e28fca3c7cab75d07daea315250fb1bf73/nvidia_cublas_cu12-12.9.2.10-py3-none-manylinux_2_27_x86_64.whl", "e4f53a8ca8c5d6e8c492d0d0a3d565ecb59a751b19cfdaa4f6da0ab2104c1702"),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-linux-cuda-13.1.0.tar.xz", "6d7e543f2985b785cfef2baf0b217fc2f891e6b5a03804ce2657b4eb70379126"),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-linux-x64-cpu.tar.xz", "bc0f20237b9ed263582ebd0844dfc7dbb61309a67c31f4a8d7ba156e21292c77"),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-macos-arm64.tar.xz", "d0fc45181d31dcceea99c3828378f4a7376e52e7fdbc43e29a07e806785b4c9a"),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-windows-cuda-13.1.0.zip", "a7c330774c0a40ec4de09ca0613af48fdc23c28bd0d90212425697daf7b1db74"),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-windows-x64-msvc.zip", "e002811d56605bce6a51c275cf8f9ba447a3707771289ea6fbcca7f4d3e9ba1f"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11236/cudart-llama-bin-win-cuda-12.4-x64.zip", "8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11236/cudart-llama-bin-win-cuda-13.4-x64.zip", "738f8c251ac22b70c3ae6f83a10cf222725df0395246a2cf58f32bdb85fbe668"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/cudart-llama-b11900-bin-ubuntu-cuda-12.8-x64.tar.gz", "0e700ddf2b15231e42a1956ab6957516a5c9bdc6e463cfc48d799f926e77224a"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/cudart-llama-b11900-bin-ubuntu-cuda-13.4-x64.tar.gz", "9080f072ee8b635921b646f08bcbca47f7847b9a256064dcd0b491bcba7ed4bb"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-macos-arm64.tar.gz", "6d3af8649e687b288395ee79f4423a21376892edb94520fa6320bfd3eb81700b"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-ubuntu-cuda-12.8-x64.tar.gz", "618c17e4e4ccc11953f21452ca34e4786a1554dc405e98a2c4d53d7c2ed5b415"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-ubuntu-cuda-13.4-x64.tar.gz", "925078bca4277d495e7fc07cfd20aa1a9ed57640b218bce3ad49de9bcc1c5799"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-ubuntu-vulkan-x64.tar.gz", "111ee8d3f61723859e57195bb9ba21241eb598163621840aaf3b5c066950c092"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-ubuntu-x64.tar.gz", "3effcc3009af64d6f30c217827d7982b3934577d282feb243b11fb6789ed124e"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-win-cpu-x64.zip", "d3bbbbb7d1b4979b72052fd73cdeeb5a4d5a4e6a3f52aeb40abd6f116558e323"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-win-cuda-12.4-x64.zip", "387ad76c926cdb301971851a11df018165a7b4df4b1b7c64cf4b09a367ec82bc"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-win-cuda-13.4-x64.zip", "0d75ad36367da00622c146a97e82fac3dcfcbe960afc2f0234eef026886b16f6"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11900/llama-b11900-bin-win-vulkan-x64.zip", "baf0183b1906d7cc0cfe9a0bd3fae34ceb4048acecc371ff3b7a91456783d051"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-x64-1.30.0.tgz", "a5ed5a3cac51fbb2e90da632ae43d19212faaa20e76484e62bcb7c23ddb3b3fd"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-x64-gpu_cuda12-1.30.0.tgz", "f9886932ee7bb0b4d3fcab736a392d4ff5efaa0672b47f19f0cec03437cf64f1"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-osx-arm64-1.30.0.tgz", "6ebb5062a934537c352937821f9fe9718e7de1a2db1122a93dd363ffd53a7012"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-win-x64-1.30.0.zip", "c6ba983baf5681af108599675d2a89c2d145512d02de28aed0bff177cd0ba949"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-win-x64-gpu_cuda12-1.30.0.zip", "d4667ea48eb0a10bc9b96b838f7b8975a6bf18de3bc5edd403a22e15c1458b23"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-ee7286d1/ace-caption-windows-x64.zip", "e3dec609c8e2715375da3425f262639490281760ffed420532544c17d7fbf868"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-ee7286d1/hotstep-tools-cuda-windows-x64.zip", "d4a9a08b796fd20e308448e1eb9faa77565ae59251b9885038785ef979eb36b3"),
    ("https://huggingface.co/StemSplitio/htdemucs-6s-onnx/resolve/49df9b6989cf2150840ea65b0bef77a2e471b678/htdemucs_6s_fp16weights.onnx", "7ce55792e2231c93fbf92de95f5fd5b3a5e6c89f7db690dfd693e8f1dce56869"),
    ("https://huggingface.co/chenmozhijin/BSRoformer-GGUF/resolve/df802a6773d25ba6ef785ff619daa3e510503168/GaboxR67/MelBandRoformers/melbandroformers/vocals/voc_fv6-Q8_0.gguf", "2cd84c9f24513749b0cb1a6ab3e3be5c5e2f7d0e8533e50512c1f394d2828a73"),
    ("https://huggingface.co/google/gemma-4-12B-it-qat-q4_0-gguf/resolve/29d097773436b69ff9feafd636ab4cf873786537/gemma-4-12b-it-qat-q4_0.gguf", "93567e57a8fe10b23569b9d9ec38cd005deedf71e29477c421a4b83f418a538b"),
    ("https://huggingface.co/google/gemma-4-E4B-it-qat-q4_0-gguf/resolve/4b4a2c1d584be7264f87aac328a1bc739ce81b6c/gemma-4-E4B_q4_0-it.gguf", "676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee"),
    ("https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/fc034cfff751157913579611efad8462ac1be606/gemma-4-12b-it-Q5_K_M.gguf", "32c554c7d1338a23837ae39b3db482213a4a642f96df3b2619f516a2375d16cd"),
    ("https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/fc034cfff751157913579611efad8462ac1be606/gemma-4-12b-it-Q6_K.gguf", "6f394336500bb5409bd68c31aa9d75097b5e6104f6cf30c82a09a7b64df41faf"),
    ("https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/fc034cfff751157913579611efad8462ac1be606/gemma-4-12b-it-Q8_0.gguf", "f20e7ff1be28c283eeeb18fc895733791c56a5851d5cd3fe9691b7f7d12afa72"),
];

/// The SHA-256 the pin at `url` names: this table, then the audio.cpp servers and their CUDA libraries of the shared
/// client, then the transcribe.cpp builds and models of the shared binding.
pub fn sha256(url: &str) -> Option<&'static str> {
    if let Some((_, sha256)) = PINNED.iter().find(|(pinned, _)| *pinned == url) {
        return Some(sha256);
    }
    use acpp_server::asset::{Build, CUDA12_RUNTIME};
    let servers = [Build::Cuda13, Build::Cuda12, Build::Vulkan, Build::Cpu, Build::Metal].into_iter().filter_map(Build::server);
    if let Some(asset) = servers.chain(CUDA12_RUNTIME.iter().copied()).find(|asset| asset.url == url) {
        return Some(asset.sha256);
    }
    if let Some(download) = dub_tcpp::Build::ALL.iter().map(|build| build.download()).find(|download| download.url == url) {
        return Some(download.sha256);
    }
    dub_tcpp::MODELS.iter().find(|model| model.download.url == url).map(|model| model.download.sha256)
}

/// The SHA-256 of the file downloaded from `url`, checked against the pin: a file that differs is removed and the
/// download fails with both hashes.
pub fn check(url: &str, file: &Path) -> Result<String> {
    let got = crate::install_record::sha256_of(file)?;
    if let Some(pinned) = sha256(url).filter(|pinned| *pinned != got) {
        let _ = std::fs::remove_file(file);
        bail!("{}: its SHA-256 {got} differs from the pinned {pinned}; the file is removed, download it again", file.display());
    }
    Ok(got)
}

/// Whether a neighbour's file staged at `staged` is the pin's: its hash when the pin names one.
pub fn accepts(url: &str, staged: &Path) -> bool {
    match sha256(url) {
        Some(pinned) => crate::install_record::sha256_of(staged).is_ok_and(|got| got == pinned),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_download_names_the_sha256_of_its_pin() {
        let mut urls: Vec<&str> = Vec::new();
        urls.extend(crate::lyrics_sync::ASSETS.iter().map(|asset| asset.url));
        urls.extend(crate::engine_runtime::ASSETS.iter().map(|asset| asset.url));
        urls.extend(crate::assistant_runtime::ASSETS.iter().map(|asset| asset.url));
        urls.extend([crate::separation::MODEL.url, crate::separation::ROFORMER_MODEL.url]);
        urls.extend([true, false].into_iter().filter_map(crate::separation::Separator::roformer_engine).map(|asset| asset.url));
        let missing: Vec<&str> = urls.into_iter().filter(|url| sha256(url).is_none()).collect();
        assert!(missing.is_empty(), "pins without a SHA-256: {missing:#?}");
        for (url, sha256) in PINNED {
            assert!(sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()), "{url}: {sha256}");
        }
    }

    #[test]
    fn a_file_that_is_not_the_pins_is_removed() {
        let dir = std::env::temp_dir().join(format!("pin-sha-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.json");
        std::fs::write(&file, b"not the pinned config").unwrap();
        let url = "https://huggingface.co/nyralabs/CrisperWhisper2.0_turbo/resolve/de0369c8a68025b7f6e86387b6eb5a3b369787c8/config.json";
        let error = check(url, &file).unwrap_err().to_string();
        assert!(error.contains("differs from the pinned 9d823d0e"), "{error}");
        assert!(!file.exists());
        assert!(!accepts(url, &file));

        std::fs::write(&file, b"anything").unwrap();
        assert!(check("https://example.invalid/unpinned.bin", &file).is_ok(), "a url without a pin is not refused here");
        std::fs::remove_dir_all(&dir).ok();
    }
}
