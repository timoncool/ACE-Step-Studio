//! The SHA-256 of every pinned download, as its publisher states it: GitHub's release asset digest, the LFS object
//! id on Hugging Face (and, for a small file kept in git there, the hash of the file whose git object id is the
//! pinned revision's), NVIDIA's redistributable manifests, PyPI's file hashes. A downloaded file whose hash differs
//! is removed and the download fails; a neighbour's file is taken only with the pin's hash.

use std::path::Path;

use anyhow::{bail, Result};

const PINNED: &[(&str, &str)] = &[
    ("https://github.com/mosynthkey/beat_this_cpp/raw/07ab790a9ec2eda8093d52d249e3ec4f0510ee72/onnx/beat_this.onnx", "c5c1466e08abdb03fdeb50668a06f244b787d564c212490482231a9cfbe9ccbd"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-12.9.1.4-archive.zip", "d534d98b0b453a98914dbf3adf47d7e84b55037abf02f87466439e1dcef581ed"),
    ("https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-13.5.1.27-archive.zip", "c946e1c825e05895747a95ed4fee18030b08052c09783b9b7b19818fd2e31f58"),
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
    ("https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl", "623f43027d40d44ceadf0043f002bd25cf353e8f13ce90b9a87057019f560661"),
    ("https://files.pythonhosted.org/packages/7a/38/bdd540bf511d2c9b6f9efc71a81c60cb88e295be0b9312b61d19bbed2212/nvidia_cublas-13.8.0.4-py3-none-manylinux_2_27_x86_64.whl", "9f17797dfcc048694461f4e47de17d2e3c25adf172ef723d2db0a07cd8744b89"),
    ("https://files.pythonhosted.org/packages/76/bf/3fea3d1c6262bded26ae00e3106432d63235954965d7785f5051ac146651/nvidia_cufft-12.4.0.43-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl", "0e8385013596b112d29c9ce8c63dc575b308d77636c7169104e18714f03961a8"),
    ("https://files.pythonhosted.org/packages/95/f4/61e6996dd20481ee834f57a8e9dca28b1869366a135e0d42e2aa8493bdd4/nvidia_cufft_cu12-11.4.1.4-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl", "c67884f2a7d276b4b80eb56a79322a95df592ae5e765cf1243693365ccab4e28"),
    ("https://files.pythonhosted.org/packages/98/8a/3431271f6344874b8f1ac03f16b3d679c91493f8da63f716160403e6d0a0/nvidia_cuda_runtime-13.4.92-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl", "9641f797da20ce1dd8e779b6e96d08cf9ba564cec8e8225458811ee26423f3a5"),
    ("https://files.pythonhosted.org/packages/bc/46/a92db19b8309581092a3add7e6fceb4c301a3fd233969856a8cbf042cd3c/nvidia_cuda_runtime_cu12-12.9.79-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl", "25bba2dfb01d48a9b59ca474a1ac43c6ebf7011f1b0b8cc44f54eb6ac48a96c3"),
    ("https://files.pythonhosted.org/packages/cb/c0/0a517bfe63ccd3b92eb254d264e28fca3c7cab75d07daea315250fb1bf73/nvidia_cublas_cu12-12.9.2.10-py3-none-manylinux_2_27_x86_64.whl", "e4f53a8ca8c5d6e8c492d0d0a3d565ecb59a751b19cfdaa4f6da0ab2104c1702"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-linux-cuda13-x64.tar.gz", "807f59113f17eb706bc7289155bdf507d05ae01650a5c2ccd9685f5574e9cd04"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-linux-cpu-x64.tar.gz", "65eb3cf26dfed0a23f01906a9f179ae7ab503f2bdac4f411e49ec902a5addafd"),
    ("https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-macos-arm64.tar.xz", "d0fc45181d31dcceea99c3828378f4a7376e52e7fdbc43e29a07e806785b4c9a"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-windows-cuda13-x64.zip", "c89128404d29c52ac0efed1a1072a76b277ccf13a33247611032746932e8d032"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-windows-cpu-x64.zip", "0b212654ea598503992119328daec051bbce5f14b73808c5cc588f278817f908"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-windows-cuda12-x64.zip", "72211af3451d9d4266406da8039a3ecdd9eabde1373cd235ff408711af8f7400"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-windows-vulkan-x64.zip", "fc82682b4071d065e2e3e84998817877a4d936da7fbb3e580799904c54a6a4ab"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-linux-cuda12-x64.tar.gz", "82540c02b5a9730137b0059ffda8c0c79b589a649af554dff1f88867a6c7444d"),
    ("https://github.com/timoncool/BSRoformer.cpp/releases/download/studio-0.1.1/BSRoformer-linux-vulkan-x64.tar.gz", "d0b6acc7b4b05c453bbfe99ea8154251fde4d96b2edd957fdab6e596f7a64927"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b11236/cudart-llama-bin-win-cuda-12.4-x64.zip", "8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/cudart-llama-bin-win-cuda-13.5-x64.zip", "c2bf4cc202bfbaaa7a0b75f5082457c3811326947bb216059b72839d844e9cba"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/cudart-llama-b12248-bin-ubuntu-cuda-12.8-x64.tar.gz", "ad244917cc8701d0fa517d6500dafd55bb74e29f26fb3a051ef31c8eed1cb90d"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/cudart-llama-b12248-bin-ubuntu-cuda-13.5-x64.tar.gz", "4b4dab68e9bf809b96fc77c900d22aea5639ade7eedf4b558d5382d2191bdcf8"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-macos-arm64.tar.gz", "7f643b4d35803ccc69270302f3368d9e8a40f56f74ef5a266e6f686d9dc7d3c6"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-ubuntu-cuda-12.8-x64.tar.gz", "508214544c8e36c2a2a614254231524a10a0857a8a025f7e4be1f437defae0ef"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-ubuntu-cuda-13.5-x64.tar.gz", "ac18e01410e0114e1d5f2f0f3bdcd9a98d83000b5179f543342c8c51c33143b7"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-ubuntu-vulkan-x64.tar.gz", "092d7097cf90c1d1a22d0bba5147500f2420bdb9630db8734230f35bcb664bed"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-ubuntu-x64.tar.gz", "6b3a530b44f6b0a4ae4872d3b3c8d1edc2ac6bba7d8bd211329a4ba644c0dc65"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-win-cpu-x64.zip", "bb306bfd390f8782c0acaba490505d12f07392791b48f1dd039387a5bacf1181"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-win-cuda-12.4-x64.zip", "3f8a8f15b50f2f1e34ad2975d8679a009ec83a83e6f440322cb22f28d8068d60"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-win-cuda-13.5-x64.zip", "ae0a1e064a94db82f3352c843ffa96eb190254fefafc29ce0213798c549a2799"),
    ("https://github.com/ggml-org/llama.cpp/releases/download/b12248/llama-b12248-bin-win-vulkan-x64.zip", "3745e446c88e18cf20fd52f9df62867beb60d12fd0208c130fb4663117425826"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-linux-x64-1.31.1.tgz", "c849e36a1ef3e22d4ff217b9d1ce4a3cf5b9467b975610913ae959ad5b16bdc9"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-osx-arm64-1.31.1.tgz", "2e1344a1926ee50eb25b38153cb6992c3567515aef0e677a1a520a1798896d6f"),
    ("https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-1.31.1.zip", "5869e26bb25314375f2d5ef1877e3ea06b1543d6c710e6e4a1ec2f7177ab4b2c"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-2ef93a1c/ace-caption-windows-x64.zip", "a2d7495bee6f01c3b6c55c79aba23d91169d9a3ce70a4d0ed9a24bf23c1865b4"),
    ("https://github.com/timoncool/YuE2-Studio/releases/download/hotstep-tools-2ef93a1c/hotstep-tools-cuda-windows-x64.zip", "07e157864bec7bba62f491a24c54881810d6943cca4af986f14cc41a8e972390"),
    ("https://huggingface.co/aaatmy/skey-onnx/resolve/c04b6a2bc1d82e50b46d8669560d056f3a2344ab/skey.onnx", "50bac66d5b02868afad07a0b159f08495cc3f5d8492654587e828ed4686412b1"),
    ("https://huggingface.co/audio-cpp/audio.cpp-gguf/resolve/462b4cec041e06ffb3d8fcf0d8c655051b1cd9f5/HTDemucs-6stems-GGUF/htdemucs-6stems-f16.gguf", "1db9bd58af6995d8caab209c99f5a284212e5bd0654fbfd7e50ce1d5ac140ba9"),
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

/// The SHA-256 of the files a pick takes out of a pinned package, by the package's URL and the file's name, as the
/// package's own RECORD lists them: a file already on disk from another address is known by its bytes.
const PICKED: &[(&str, &str, &str)] = &[
    ("https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl", "cublas64_12.dll", "52ce5ba0ec5327d39be6021f0f9362d2ed4d64d7206f9b5afc06398d827b8e82"),
    ("https://files.pythonhosted.org/packages/20/e2/fc9a0e985249d873150276d5afb02e39a66817fedbf1a385724393e505ed/nvidia_cublas_cu12-12.9.2.10-py3-none-win_amd64.whl", "cublasLt64_12.dll", "71705a7cf0923f4ab034d0d2620bbfc989ff669ac3d79f868b7a7c597ca559c5"),
    ("https://files.pythonhosted.org/packages/cb/c0/0a517bfe63ccd3b92eb254d264e28fca3c7cab75d07daea315250fb1bf73/nvidia_cublas_cu12-12.9.2.10-py3-none-manylinux_2_27_x86_64.whl", "libcublas.so.12", "5757ab5839fb4f203ca47ecb336110d10f4a5606b1e097f195fbca89774569e2"),
    ("https://files.pythonhosted.org/packages/cb/c0/0a517bfe63ccd3b92eb254d264e28fca3c7cab75d07daea315250fb1bf73/nvidia_cublas_cu12-12.9.2.10-py3-none-manylinux_2_27_x86_64.whl", "libcublasLt.so.12", "2c9006a75c74b3bea2dc7ae2ec38ab038b0e45ea02cb4b717a915e8a5796acb1"),
];

/// The SHA-256 of the file `name` a pick takes out of the package at `url`, when the package lists it.
pub fn picked(url: &str, name: &str) -> Option<&'static str> {
    PICKED.iter().find(|(package, file, _)| *package == url && *file == name).map(|(_, _, sha256)| *sha256)
}

/// Whether `path` holds the bytes of `sha256`. A file is hashed once for its size and modification time, since the
/// models page asks every second.
pub fn holds(path: &Path, sha256: &str) -> bool {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static HASHED: OnceLock<Mutex<HashMap<std::path::PathBuf, (u64, std::time::SystemTime, String)>>> = OnceLock::new();
    let Ok(meta) = std::fs::metadata(path) else { return false };
    let Ok(modified) = meta.modified() else { return false };
    let hashed = HASHED.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((len, at, got)) = hashed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(path) {
        if *len == meta.len() && *at == modified {
            return got == sha256;
        }
    }
    let got = match crate::install_record::sha256_of(path) {
        Ok(got) => got,
        Err(error) => {
            studio_log!("[ERROR] {} could not be hashed: {error:#}", path.display());
            return false;
        }
    };
    let same = got == sha256;
    hashed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(path.to_path_buf(), (meta.len(), modified, got));
    same
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_download_names_the_sha256_of_its_pin() {
        let mut urls: Vec<&str> = Vec::new();
        urls.extend(crate::lyrics_sync::assets().iter().map(|asset| asset.url));
        urls.extend(crate::engine_runtime::ASSETS.iter().map(|asset| asset.url));
        urls.extend(crate::acpp::ASSETS.iter().map(|asset| asset.url));
        urls.extend(crate::assistant_runtime::ASSETS.iter().map(|asset| asset.url));
        urls.extend([crate::separation::MODEL.url, crate::separation::ROFORMER_MODEL.url]);
        urls.extend(crate::separation::RoformerBuild::ALL.into_iter().filter_map(crate::separation::roformer_asset).map(|asset| asset.url));
        let missing: Vec<&str> = urls.into_iter().filter(|url| sha256(url).is_none()).collect();
        assert!(missing.is_empty(), "pins without a SHA-256: {missing:#?}");
        for (url, sha256) in PINNED {
            assert!(sha256.len() == 64 && sha256.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()), "{url}: {sha256}");
        }
    }


    #[test]
    fn no_download_follows_a_branch() {
        let mut urls: Vec<&str> = Vec::new();
        urls.extend(crate::lyrics_sync::assets().iter().map(|asset| asset.url));
        urls.extend(crate::acpp::ASSETS.iter().map(|asset| asset.url));
        urls.extend(crate::engine_runtime::ASSETS.iter().map(|asset| asset.url));
        urls.extend(crate::assistant_runtime::ASSETS.iter().map(|asset| asset.url));
        urls.extend([crate::separation::MODEL.url, crate::separation::ROFORMER_MODEL.url]);
        urls.extend(crate::training::pack().iter().map(|asset| asset.url));
        urls.extend(crate::training::listen_pack().iter().map(|asset| asset.url));
        urls.extend(crate::midi::SIZES.iter().flat_map(crate::midi::weight_assets).map(|asset| asset.url));
        let branches: Vec<&str> = urls.into_iter().filter(|url| url.contains("/resolve/main/") || url.contains("/raw/main/")).collect();
        assert!(branches.is_empty(), "downloads that follow a branch instead of a pinned revision: {branches:#?}");
        let skey = crate::training::listen_pack().iter().find(|asset| asset.id == "s-key").expect("the listening set downloads S-KEY");
        assert!(sha256(skey.url).is_some(), "S-KEY names the SHA-256 of its pin");
        let beat = crate::training::listen_pack().iter().find(|asset| asset.id == "beat-this").expect("the listening set downloads Beat This!");
        assert!(sha256(beat.url).is_some(), "Beat This! names the SHA-256 of its pin");
    }

    #[test]
    fn every_picked_hash_is_of_a_file_a_download_picks() {
        let downloads: Vec<&crate::downloads::Asset> =
            crate::lyrics_sync::assets().iter().chain(crate::engine_runtime::ASSETS).chain(crate::acpp::ASSETS).collect();
        for (url, name, sha256) in PICKED {
            assert_eq!(sha256.len(), 64, "{name}");
            if let Some(asset) = downloads.iter().find(|asset| asset.url == *url) {
                assert!(asset.pick.iter().any(|entry| crate::downloads::picked_file_name(entry) == *name), "{} does not pick {name}", asset.id);
            }
        }
        for asset in downloads.iter().filter(|asset| PICKED.iter().any(|(url, _, _)| *url == asset.url)) {
            for entry in asset.pick {
                assert!(picked(asset.url, crate::downloads::picked_file_name(entry)).is_some(), "{}: {entry} has no hash", asset.id);
            }
        }
    }

    #[test]
    fn a_file_is_known_by_its_bytes_and_hashed_again_once_it_changes() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("cublas64_12.dll");
        std::fs::write(&file, b"today").unwrap();
        let today = crate::install_record::sha256_of(&file).unwrap();
        assert!(holds(&file, &today));
        assert!(holds(&file, &today), "the second look is the first one's hash");
        std::fs::write(&file, b"an earlier build").unwrap();
        assert!(!holds(&file, &today), "another size is hashed again");
        assert!(!holds(&root.path().join("missing.dll"), &today));
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
