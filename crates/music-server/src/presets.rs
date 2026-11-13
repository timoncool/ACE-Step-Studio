use std::{process::Command, sync::OnceLock};

use music_core::{Capability, ExecutionMode, StudioConfiguration};
use serde::Serialize;
use sysinfo::System;

/// Bytes in the gigabyte memory is counted in: a "32 GB" computer has 32 × 2³⁰ bytes, not 32 × 10⁹.
const GIB: f64 = (1u64 << 30) as f64;

#[derive(Clone, Copy)]
pub struct Preset {
    pub id: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub min_vram_gb: f64,
    pub profile_id: Option<&'static str>,
    pub provider_mode: bool,
    /// `custom` is deliberately a no-op: it must never overwrite manual choices.
    pub preserves_configuration: bool,
}

pub const PRESETS: &[Preset] = &[
    Preset { id: "native-full", title: "Full native", subtitle: "XL turbo BF16, LM 4B BF16; original weights, 19.9 GB", min_vram_gb: 23.5, profile_id: Some("native"), provider_mode: false, preserves_configuration: false },
    Preset { id: "native-quality", title: "Recommended", subtitle: "XL turbo Q8_0, LM 4B Q8_0, 10.9 GB", min_vram_gb: 13.0, profile_id: Some("quality-q8"), provider_mode: false, preserves_configuration: false },
    Preset { id: "native-balanced", title: "Balanced", subtitle: "XL turbo Q6_K, LM 1.7B Q8_0, 7.2 GB", min_vram_gb: 9.5, profile_id: Some("balanced"), provider_mode: false, preserves_configuration: false },
    Preset { id: "native-efficient", title: "Light", subtitle: "XL turbo Q4_K_M, LM 1.7B Q8_0, 6.1 GB", min_vram_gb: 7.5, profile_id: Some("recommended-light"), provider_mode: false, preserves_configuration: false },
    Preset { id: "native-minimal", title: "Minimal", subtitle: "2B turbo Q4_K_M, LM 0.6B Q8_0, 3.3 GB", min_vram_gb: 4.0, profile_id: Some("minimal"), provider_mode: false, preserves_configuration: false },
    Preset { id: "full-openrouter", title: "Full OpenRouter", subtitle: "Cloud for every catalog-verified capability, including music", min_vram_gb: 0.0, profile_id: None, provider_mode: true, preserves_configuration: false },
    Preset { id: "custom", title: "Custom", subtitle: "Keep every current provider and model choice unchanged", min_vram_gb: -1.0, profile_id: None, provider_mode: false, preserves_configuration: true },
];

pub struct PresetApplication {
    pub profile_id: Option<String>,
    pub selected_profile_changed: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub gpu_name: String,
    pub total_vram_gb: f64,
    pub total_ram_gb: f64,
    pub has_gpu: bool,
    /// The card is an AMD or Intel one the engine reaches through Vulkan, not NVIDIA's CUDA or Apple's Metal.
    pub vulkan: bool,
    /// No card the songs are computed on: none at all, or one with less memory than the smallest local set needs.
    pub on_processor: bool,
    pub recommended: &'static str,
    pub reason: String,
}

#[derive(Serialize)]
pub struct PresetView {
    pub id: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub min_vram_gb: f64,
    pub profile_id: Option<&'static str>,
    pub provider_mode: bool,
}

pub fn list() -> Vec<PresetView> {
    PRESETS
        .iter()
        .map(|preset| {
            let (profile_id, subtitle) = match preset.profile_id.map(on_this_card) {
                Some((profile, Some(files))) => (Some(profile), files),
                Some((profile, None)) => (Some(profile), preset.subtitle),
                None => (None, preset.subtitle),
            };
            PresetView { id: preset.id, title: preset.title, subtitle, min_vram_gb: preset.min_vram_gb, profile_id, provider_mode: preset.provider_mode }
        })
        .collect()
}

/// `nvidia-smi` costs tens of milliseconds and the setup screen polls status
/// once per second while a download runs. The machine's GPU does not change
/// inside one process lifetime, so probe it once.
fn probe() -> &'static Hardware {
    static HARDWARE: OnceLock<Hardware> = OnceLock::new();
    HARDWARE.get_or_init(|| {
        let mut system = System::new().expect("sysinfo has Windows, Linux and macOS");
        system.refresh_memory();
        let total_ram_gb = system.total_memory() as f64 / GIB;
        let (card, vulkan, nvidia) = match nvidia_smi() {
            Some(card) => (Some(card), false, true),
            None => {
                let other = other_card();
                let vulkan = other.is_some() && !cfg!(target_os = "macos");
                (other, vulkan, false)
            }
        };
        let (gpu_name, total_vram_gb) = card.unwrap_or_else(|| ("No NVIDIA GPU detected".into(), 0.0));
        let has_gpu = total_vram_gb > 0.0;
        let (recommended, mut reason) = recommend_for_hardware(&gpu_name, total_vram_gb, total_ram_gb, crate::model_manager::profile_weights_bytes);
        if vulkan {
            reason.push_str("; the engine computes on it through Vulkan");
        }
        let on_processor = computes_on_processor(nvidia, total_vram_gb);
        Hardware { gpu_name, total_vram_gb, total_ram_gb, has_gpu, vulkan, on_processor, recommended, reason }
    })
}

/// Whether songs are computed on the processor: no card, or built-in graphics below the smallest local set. An
/// NVIDIA card nvidia-smi lists is where the engine computes on CUDA at any size; a Mac computes on Metal.
fn computes_on_processor(nvidia: bool, total_vram_gb: f64) -> bool {
    let smallest = PRESETS
        .iter()
        .filter(|preset| !preset.provider_mode && preset.profile_id.is_some())
        .map(|preset| preset.min_vram_gb)
        .fold(f64::INFINITY, f64::min);
    !cfg!(target_os = "macos") && !nvidia && total_vram_gb < smallest
}

pub fn hardware() -> Hardware {
    probe().clone()
}

/// The unquantised set stays in the list for whoever picks it; the studio never recommends full weights,
/// Q8_0 is near lossless at half the memory.
const FULL_WEIGHTS_PRESET: &str = "native-full";

/// What the system, the window and the service hold beside the models.
const SYSTEM_RAM_GB: f64 = 4.0;
/// A set read from disk passes through memory even when its weights end on the card.
const LOADING_RAM_GB: f64 = 2.0;
/// The compute buffers of the longest songs, when they live in RAM.
const WORK_RAM_GB: f64 = 2.0;

/// Memory a set needs on this machine. On a card its preset fits, the weights
/// live in video memory; on Vulkan over shared memory or on the processor
/// they live in RAM with the buffers.
pub fn ram_needed_gb(profile: &str, weights_bytes: u64, total_vram_gb: f64) -> f64 {
    // Apple Silicon's video memory is the system's own, so weights "on the card" still take the RAM
    let on_card = !cfg!(target_os = "macos") && PRESETS.iter().any(|preset| preset.profile_id == Some(profile) && total_vram_gb >= preset.min_vram_gb);
    if on_card {
        SYSTEM_RAM_GB + LOADING_RAM_GB
    } else {
        weights_bytes as f64 / GIB + WORK_RAM_GB + SYSTEM_RAM_GB
    }
}

/// The largest set the card and the memory both hold; full weights are never
/// the answer. Without a card for any set, the smallest runs on Vulkan or the
/// processor, where the engine goes on its own when CUDA cannot hold it; cloud
/// music stays a choice.
fn recommend_for_hardware(gpu_name: &str, total_vram_gb: f64, total_ram_gb: f64, weights_bytes: impl Fn(&str) -> u64) -> (&'static str, String) {
    let fits = |id: &str| total_ram_gb >= ram_needed_gb(id, weights_bytes(id), total_vram_gb);
    let preset = PRESETS
        .iter()
        .filter(|preset| !preset.provider_mode && preset.id != FULL_WEIGHTS_PRESET)
        .filter_map(|preset| preset.profile_id.map(|profile| (preset, profile)))
        .find(|(preset, profile)| total_vram_gb > 0.0 && total_vram_gb >= preset.min_vram_gb && fits(profile));
    match preset {
        Some((preset, _)) => (preset.id, format!("{gpu_name} with {total_vram_gb:.1} GB VRAM and {total_ram_gb:.0} GB of memory matches {}", preset.title)),
        None if total_vram_gb <= 0.0 => (
            "native-minimal",
            "No NVIDIA VRAM was detected; the smallest set runs on Vulkan or the processor.".into(),
        ),
        None => (
            "native-minimal",
            format!("{gpu_name} with {total_vram_gb:.1} GB VRAM and {total_ram_gb:.0} GB of memory holds no set on the card; the smallest runs on the processor instead"),
        ),
    }
}

/// Chooses the complete local set on a clean install. This only records a
/// selection; downloading any component remains a separate user action.
pub fn recommended_local_profile() -> &'static str {
    profile_for_preset(probe().recommended)
}

/// Maps a hardware preset onto the complete four-component profile it installs.
pub fn profile_for_preset(preset_id: &str) -> &'static str {
    let profile = match preset_id {
        "native-full" => "native",
        "native-quality" => "quality-q8",
        "native-balanced" => "balanced",
        "native-efficient" => "recommended-light",
        "native-minimal" => "minimal",
        // A machine without usable local VRAM still needs a named local target
        // for the Model Manager; the quality set is the smallest set that is
        // not advertised as a speed compromise.
        _ => "quality-q8",
    };
    on_this_card(profile).0
}

/// The set a preset installs on this card, with the files it names: on an RTX 50 with the CUDA 13 build the XL sets
/// take their DiT in MXFP4, which its tensor cores compute directly, at half the DiT memory of Q8_0.
fn on_this_card(profile: &'static str) -> (&'static str, Option<&'static str>) {
    fp4_sibling(profile, crate::cuda_build::computes_fp4_natively())
}

fn fp4_sibling(profile: &'static str, fp4_native: bool) -> (&'static str, Option<&'static str>) {
    match profile {
        "quality-q8" if fp4_native => ("quality-mxfp4", Some("XL turbo MXFP4, LM 4B Q8_0, 8.2 GB; MXFP4: half the DiT memory of Q8_0, computed on the RTX 50's tensor cores directly")),
        "balanced" | "recommended-light" if fp4_native => ("balanced-mxfp4", Some("XL turbo MXFP4, LM 1.7B Q8_0, 5.8 GB; MXFP4: half the DiT memory of Q8_0, computed on the RTX 50's tensor cores directly")),
        _ => (profile, None),
    }
}

pub fn apply(id: &str, configuration: &mut StudioConfiguration, openrouter_music_available: bool) -> Result<PresetApplication, String> {
    let preset = PRESETS.iter().find(|preset| preset.id == id).ok_or_else(|| format!("unknown preset '{id}'"))?;
    if preset.preserves_configuration {
        return Ok(PresetApplication { profile_id: None, selected_profile_changed: false });
    }
    if preset.provider_mode {
        if !openrouter_music_available {
            return Err("Full OpenRouter requires a refreshed catalog with an eligible music-generation model; current provider choices were left unchanged".into());
        }
        for selection in &mut configuration.selections {
            selection.mode = ExecutionMode::OpenRouter;
            selection.local_engine = None;
            selection.cloud_model = None;
        }
        return Ok(PresetApplication { profile_id: None, selected_profile_changed: true });
    }
    let music = configuration.selections.iter_mut().find(|selection| selection.capability == Capability::MusicGeneration).ok_or("music capability is missing")?;
    music.mode = ExecutionMode::Local;
    music.local_engine = Some(crate::model_manager::ENGINE_ID.into());
    music.cloud_model = None;
    Ok(PresetApplication { profile_id: preset.profile_id.map(|profile| on_this_card(profile).0.to_owned()), selected_profile_changed: true })
}

fn nvidia_smi() -> Option<(String, f64)> {
    let output = dub_core::proc::host_command("nvidia-smi").args(crate::cuda_build::chosen_card()).args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]).output().ok()?;
    if !output.status.success() { return None; }
    let line = String::from_utf8_lossy(&output.stdout).lines().next()?.trim().to_owned();
    let (name, memory) = line.rsplit_once(',')?;
    Some((name.trim().into(), memory.trim().parse::<f64>().ok()? / 1024.0))
}

/// The card that sizes the recommendation when nvidia-smi lists none: on
/// Windows an AMD or Intel adapter, which the engine reaches through Vulkan.
/// An NVIDIA adapter nvidia-smi does not list is left out: CUDA does not run
/// there, and the engine on Vulkan is not for NVIDIA cards.
#[cfg(windows)]
fn other_card() -> Option<(String, f64)> {
    registry_adapter(|name| !is_nvidia(name))
}

#[cfg(not(windows))]
fn other_card() -> Option<(String, f64)> {
    display_adapter()
}


/// A graphics card of any make - the ONNX parts reach one through DirectML
/// where CUDA does not run - as the display adapter with the most dedicated
/// memory. Probed once: the machine's card does not change inside a process.
pub fn display_card() -> Option<&'static str> {
    static CARD: OnceLock<Option<String>> = OnceLock::new();
    CARD.get_or_init(|| display_adapter().map(|(name, _)| name)).as_deref()
}

/// The display adapter with the most dedicated memory, from the driver's own
/// registry entry: `HardwareInformation.qwMemorySize` is the 64-bit size the
/// driver reports, where WMI's `AdapterRAM` wraps at 4 GB.
#[cfg(windows)]
fn display_adapter() -> Option<(String, f64)> {
    registry_adapter(|_| true)
}


/// The adapter with the most memory among those `keep` takes, as the display adapters' class in the registry lists them.
#[cfg(windows)]
fn registry_adapter(keep: impl Fn(&str) -> bool) -> Option<(String, f64)> {
    use std::os::windows::process::CommandExt;
    const CLASS: &str = r"HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    // A GUI process spawning a console tool flashes a window without this.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let query = |value: &str| -> Option<String> {
        let output = Command::new("reg").args(["query", CLASS, "/s", "/v", value]).creation_flags(CREATE_NO_WINDOW).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    };
    best_adapter(&query("DriverDesc")?, &query("HardwareInformation.qwMemorySize")?, keep)
}

#[cfg(windows)]
/// Whether an adapter's name is an NVIDIA card's: nvidia-smi answers for those, and a Vulkan run of the engine is not
/// for them.
fn is_nvidia(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["nvidia", "geforce", "quadro", "rtx", "tesla"].iter().any(|mark| name.contains(mark))
}

#[cfg(target_os = "macos")]
fn display_adapter() -> Option<(String, f64)> {
    // Apple Silicon has one GPU per machine and no separate VRAM: its memory
    // is the system's (unified), so the whole RAM is what a model set has to
    // share with the rest of the machine. The chip name is the only identity
    // macOS reports for it.
    let output = Command::new("system_profiler").args(["-json", "SPDisplaysDataType"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let chip = apple_chip(&String::from_utf8_lossy(&output.stdout))?;
    let ram = Command::new("sysctl").args(["-n", "hw.memsize"]).output().ok().and_then(|output| {
        std::str::from_utf8(&output.stdout).ok().and_then(|value| value.trim().parse::<u64>().ok())
    })?;
    Some((chip, ram as f64 / GIB))
}

/// The chip name from `system_profiler -json SPDisplaysDataType`: the value of
/// `"sppci_model" : "Apple M2 Max"`.
#[cfg(any(target_os = "macos", test))]
fn apple_chip(profile: &str) -> Option<String> {
    let after_key = profile.split("\"sppci_model\"").nth(1)?;
    let name = after_key.split('"').nth(1)?;
    name.starts_with("Apple ").then(|| name.to_string())
}

/// Linux has no registry to read the adapter from, so the card is named from
/// its PCI identity instead. An unnamed card would leave the ONNX parts and
/// the writing assistant on the processor on a machine whose GPU works.
#[cfg(not(any(windows, target_os = "macos")))]
fn display_adapter() -> Option<(String, f64)> {
    let card = linux_card()?;
    let name = linux_card_name(&card.vendor_id, &card.device_id);
    Some((name, card.vram_gb))
}

#[cfg(not(any(windows, target_os = "macos")))]
struct LinuxCard {
    vendor_id: String,
    device_id: String,
    vram_gb: f64,
}

/// The first DRM card that is a PCI device, with the dedicated memory it
/// reports. Only the AMD driver publishes `mem_info_vram_total`; where it is
/// absent the size stays 0.0, which recommends no local set rather than
/// guessing at one that may not fit.
#[cfg(not(any(windows, target_os = "macos")))]
fn linux_card() -> Option<LinuxCard> {
    let mut cards: Vec<std::path::PathBuf> = std::fs::read_dir("/sys/class/drm")
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("card") && name[4..].chars().all(|c| c.is_ascii_digit()))
        })
        .collect();
    cards.sort();
    cards.into_iter().find_map(|path| {
        let device = path.join("device");
        let read = |name: &str| -> Option<String> {
            std::fs::read_to_string(device.join(name)).ok().map(|value| value.trim().to_owned())
        };
        let vendor_id = read("vendor")?;
        let device_id = read("device").unwrap_or_default();
        let vram_gb = read("mem_info_vram_total")
            .and_then(|bytes| bytes.parse::<f64>().ok())
            .map_or(0.0, |bytes| bytes / 1024.0 / 1024.0 / 1024.0);
        Some(LinuxCard { vendor_id, device_id, vram_gb })
    })
}

/// `lspci` names the card as a person would; without it the vendor still names
/// it well enough to reach the GPU. `8086` is Intel, `1002` and `1022` AMD.
#[cfg(not(any(windows, target_os = "macos")))]
fn linux_card_name(vendor_id: &str, device_id: &str) -> String {
    let vendor = match vendor_id.trim_start_matches("0x").to_ascii_lowercase().as_str() {
        "8086" => "Intel",
        "1002" | "1022" => "AMD",
        "10de" => "NVIDIA",
        _ => "PCI",
    };
    lspci_name(device_id).unwrap_or_else(|| format!("{vendor} display adapter ({vendor_id}:{device_id})"))
}

/// The `lspci` line for this PCI device, trimmed to the adapter's own name.
#[cfg(not(any(windows, target_os = "macos")))]
fn lspci_name(device_id: &str) -> Option<String> {
    let output = dub_core::proc::host_command("lspci").args(["-nn"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_lspci_name(&String::from_utf8_lossy(&output.stdout), device_id)
}

/// Pulls the adapter's own name out of an `lspci -nn` listing. The line is
/// found by the bracketed device id, which `lspci` writes as `[vendor:device]`
/// in lowercase, so the `:` keeps a class code like `[0300]` from matching.
/// The class prefix sits before the first `: ` and the ids after the first
/// ` [`, so both are dropped.
#[cfg(not(any(windows, target_os = "macos")))]
fn parse_lspci_name(listing: &str, device_id: &str) -> Option<String> {
    let id = device_id.trim_start_matches("0x").to_ascii_lowercase();
    if id.is_empty() {
        return None;
    }
    let needle = format!(":{id}]");
    let line = listing.lines().find(|line| line.to_ascii_lowercase().contains(&needle))?;
    let after_class = line.split_once(": ")?.1;
    let name = after_class.split_once(" [").map_or(after_class, |(name, _)| name).trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Joins `reg query /s` listings of the adapter names and memory sizes by
/// their subkey and keeps the adapter with the most memory among those `keep` takes.
#[cfg(windows)]
fn best_adapter(names: &str, sizes: &str, keep: impl Fn(&str) -> bool) -> Option<(String, f64)> {
    fn values(listing: &str) -> Vec<(String, String)> {
        let mut key = String::new();
        let mut out = Vec::new();
        for line in listing.lines() {
            if line.starts_with("HKEY_") {
                key = line.trim().to_owned();
            } else if let Some((_, value)) = line.trim().split_once("    REG_") {
                if let Some((_, data)) = value.split_once("    ") {
                    out.push((key.clone(), data.trim().to_owned()));
                }
            }
        }
        out
    }
    let names = values(names);
    values(sizes)
        .into_iter()
        .filter_map(|(key, size)| {
            let bytes = u64::from_str_radix(size.trim_start_matches("0x"), 16).ok()?;
            let name = names.iter().find(|(name_key, _)| *name_key == key)?.1.clone();
            (!name.starts_with("Microsoft") && keep(&name)).then_some((name, bytes as f64 / 1024.0 / 1024.0 / 1024.0))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_presets_have_complete_profile_ids() {
        for preset in PRESETS.iter().filter(|preset| !preset.provider_mode && !preset.preserves_configuration) {
            assert!(crate::model_manager::profile_exists(preset.profile_id.unwrap()));
        }
    }

    #[test]
    fn openrouter_preset_selects_every_capability_without_a_fake_model_id() {
        let mut configuration = StudioConfiguration::default();
        let result = apply("full-openrouter", &mut configuration, true).unwrap();
        assert!(result.selected_profile_changed);
        assert!(result.profile_id.is_none());
        assert!(configuration.selections.iter().all(|selection| selection.mode == ExecutionMode::OpenRouter && selection.cloud_model.is_none()));
    }

    #[test]
    fn full_openrouter_leaves_configuration_untouched_without_a_verified_music_model() {
        let mut configuration = StudioConfiguration::default();
        let before = serde_json::to_value(&configuration).unwrap();
        assert!(apply("full-openrouter", &mut configuration, false).is_err());
        assert_eq!(serde_json::to_value(&configuration).unwrap(), before);
    }

    #[test]
    fn custom_is_a_true_no_op() {
        let mut configuration = StudioConfiguration::default();
        let before = serde_json::to_value(&configuration).unwrap();
        let result = apply("custom", &mut configuration, false).unwrap();
        assert!(!result.selected_profile_changed);
        assert!(result.profile_id.is_none());
        assert_eq!(serde_json::to_value(&configuration).unwrap(), before);
    }

    fn recommend(gpu: &str, vram: f64) -> &'static str {
        recommend_for_hardware(gpu, vram, 64.0, crate::model_manager::profile_weights_bytes).0
    }

    #[test]
    fn recommendation_matches_named_cards_and_vram_tiers() {
        // full weights are never recommended, even on a card they fit
        assert_eq!(recommend("NVIDIA GeForce RTX 5090", 31.8), "native-quality");
        assert_eq!(recommend("NVIDIA GeForce RTX 4090", 23.99), "native-quality");
        assert_eq!(recommend("RTX 4080", 15.9), "native-quality");
        assert_eq!(recommend("RTX 4070", 11.9), "native-balanced");
        assert_eq!(recommend("RTX 4060", 8.0), "native-efficient");
        assert_eq!(recommend("GTX 1660", 6.0), "native-minimal");
        assert_eq!(recommend("GT 1030", 2.0), "native-minimal");
        assert_eq!(recommend("No NVIDIA GPU detected", 0.0), "native-minimal");
    }

    /// Songs are computed on the processor without a card and on built-in graphics below the smallest set; an NVIDIA
    /// card of any size computes on CUDA. The window says so then.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn songs_go_to_the_processor_without_a_card_or_on_built_in_graphics() {
        assert!(computes_on_processor(false, 0.0), "no card");
        assert!(computes_on_processor(false, 0.125), "Intel UHD with 128 MB");
        assert!(!computes_on_processor(true, 3.0), "an NVIDIA card of 3 GB computes on CUDA");
        assert!(!computes_on_processor(false, 16.0), "an AMD card of 16 GB");
    }

    #[test]
    fn every_local_tier_installs_a_profile() {
        for vram in [5.0, 8.0, 10.0, 16.0, 24.0, 32.0] {
            assert!(crate::model_manager::profile_exists(profile_for_preset(recommend("NVIDIA test card", vram))));
        }
    }


    /// An RTX 5090 on the CUDA 13 build is recommended the set with the DiT in MXFP4; an RTX 4090 keeps its set and
    /// sees MXFP4 as a choice; the CUDA 12 build, AMD, a Mac and no card see no MXFP4 at all.
    #[test]
    fn mxfp4_is_the_rtx_50_default_a_cuda13_choice_and_absent_elsewhere() {
        use crate::cuda_build::{mxfp4_offered, CudaBuild};
        let rtx5090 = fp4_sibling("quality-q8", true);
        assert_eq!(rtx5090.0, "quality-mxfp4");
        assert!(rtx5090.1.is_some_and(|files| files.contains("MXFP4")));
        assert_eq!(fp4_sibling("recommended-light", true).0, "balanced-mxfp4");
        assert_eq!(fp4_sibling("minimal", true), ("minimal", None), "no 2B set in MXFP4");
        assert_eq!(fp4_sibling("quality-q8", false), ("quality-q8", None), "RTX 4090: the recommendation stays");
        for (profile, _) in [rtx5090, fp4_sibling("balanced", true)] {
            assert!(crate::model_manager::profile_exists(profile), "{profile}");
        }
        assert!(mxfp4_offered(Some(CudaBuild::Cuda13)), "RTX 4090 on CUDA 13 sees MXFP4");
        for build in [Some(CudaBuild::Cuda12), None] {
            assert!(!mxfp4_offered(build), "{build:?}: CUDA 12, AMD, a Mac or no card");
        }
    }

    #[test]
    fn a_set_needs_the_memory_too() {
        let weights = crate::model_manager::profile_weights_bytes;
        assert_eq!(ram_needed_gb("quality-q8", weights("quality-q8"), 16.0), 6.0);
        assert!(ram_needed_gb("quality-q8", weights("quality-q8"), 8.0) > 16.0);
        // a 16 GB card in a machine with 5 GB of memory falls to the smallest set
        assert_eq!(recommend_for_hardware("RTX 4080", 16.0, 5.0, weights).0, "native-minimal");
        assert_eq!(recommend_for_hardware("RTX 4080", 16.0, 8.0, weights).0, "native-quality");
    }

    #[cfg(windows)]
    #[test]
    fn the_adapter_with_the_most_memory_wins_and_basic_display_never_does() {
        let names = "\r\nHKEY_LOCAL_MACHINE\\X\\0000\r\n    DriverDesc    REG_SZ    AMD Radeon RX 7800 XT\r\n\r\nHKEY_LOCAL_MACHINE\\X\\0001\r\n    DriverDesc    REG_SZ    Intel(R) UHD Graphics 770\r\n\r\nHKEY_LOCAL_MACHINE\\X\\0002\r\n    DriverDesc    REG_SZ    Microsoft Basic Display Adapter\r\n";
        let sizes = "\r\nHKEY_LOCAL_MACHINE\\X\\0000\r\n    HardwareInformation.qwMemorySize    REG_QWORD    0x400000000\r\n\r\nHKEY_LOCAL_MACHINE\\X\\0001\r\n    HardwareInformation.qwMemorySize    REG_QWORD    0x80000000\r\n\r\nHKEY_LOCAL_MACHINE\\X\\0002\r\n    HardwareInformation.qwMemorySize    REG_QWORD    0x800000000\r\n";
        let (name, vram) = best_adapter(names, sizes, |_| true).unwrap();
        assert_eq!(name, "AMD Radeon RX 7800 XT");
        assert_eq!(vram, 16.0);
    }

    /// A laptop with Intel graphics and an NVIDIA card nvidia-smi does not list: the NVIDIA one is not taken for a
    /// Vulkan run, and the Intel graphics' memory is below every set on the card.
    #[cfg(windows)]
    #[test]
    fn a_card_without_nvidia_smi_is_amd_or_intel_and_built_in_graphics_gets_no_set_on_the_card() {
        let names = "\r\nHKEY_LOCAL_MACHINE\\X\\0000\r\n    DriverDesc    REG_SZ    Intel(R) UHD Graphics\r\n\r\nHKEY_LOCAL_MACHINE\\X\\0001\r\n    DriverDesc    REG_SZ    NVIDIA GeForce RTX 3060 Laptop GPU\r\n";
        let sizes = "\r\nHKEY_LOCAL_MACHINE\\X\\0000\r\n    HardwareInformation.qwMemorySize    REG_QWORD    0x8000000\r\n\r\nHKEY_LOCAL_MACHINE\\X\\0001\r\n    HardwareInformation.qwMemorySize    REG_QWORD    0x180000000\r\n";
        let (name, vram) = best_adapter(names, sizes, |name| !is_nvidia(name)).unwrap();
        assert_eq!((name.as_str(), vram), ("Intel(R) UHD Graphics", 0.125));
        let (preset, reason) = recommend_for_hardware(&name, vram, 32.0, |_| 7_000_000_000);
        assert_eq!(preset, "native-minimal", "128 MB of built-in graphics holds no set: the smallest runs on the processor");
        assert!(reason.contains("Intel(R) UHD Graphics"), "{reason}");
        let radeon = "\r\nHKEY_LOCAL_MACHINE\\X\\0000\r\n    DriverDesc    REG_SZ    AMD Radeon RX 7800 XT\r\n";
        let sixteen = "\r\nHKEY_LOCAL_MACHINE\\X\\0000\r\n    HardwareInformation.qwMemorySize    REG_QWORD    0x400000000\r\n";
        let (name, vram) = best_adapter(radeon, sixteen, |name| !is_nvidia(name)).unwrap();
        assert_eq!(recommend_for_hardware(&name, vram, 64.0, |_| 13_000_000_000).0, "native-quality", "16 GB of an AMD card sizes the set as on Linux");
    }

    #[test]
    fn the_apple_chip_is_read_from_the_system_profiler_json() {
        let profile = r#"{ "SPDisplaysDataType" : [ { "_name" : "Apple M2 Max", "sppci_cores" : "30", "sppci_model" : "Apple M2 Max" } ] }"#;
        assert_eq!(apple_chip(profile).as_deref(), Some("Apple M2 Max"));
        assert_eq!(apple_chip(r#"{ "sppci_model" : "AMD Radeon Pro" }"#), None);
        assert_eq!(apple_chip("{}"), None);
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn a_linux_card_is_named_from_its_vendor_when_lspci_is_silent() {
        let name = linux_card_name("0x8086", "0xffff");
        assert!(name.starts_with("Intel"), "got {name}");
        assert!(name.contains("0x8086"), "the ids stay visible: {name}");
        assert_eq!(linux_card_name("0x1002", "0xffff").split(' ').next(), Some("AMD"));
        assert_eq!(linux_card_name("0x10de", "0xffff").split(' ').next(), Some("NVIDIA"));
        assert_eq!(linux_card_name("0x1234", "0xffff").split(' ').next(), Some("PCI"));
        assert_eq!(linux_card_name("8086", "ffff").split(' ').next(), Some("Intel"));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn an_lspci_line_keeps_the_adapter_name_and_drops_the_class_and_ids() {
        let listing = concat!(
            "00:1f.3 Audio device [0403]: Intel Corporation Device [8086:7f50]\n",
            "04:00.0 VGA compatible controller [0300]: Intel Corporation Battlemage G21 [Arc B580] [8086:e20b]\n",
        );
        assert_eq!(parse_lspci_name(listing, "0xe20b").as_deref(), Some("Intel Corporation Battlemage G21"));
        assert_eq!(parse_lspci_name(listing, "0x7f50").as_deref(), Some("Intel Corporation Device"));
        assert_eq!(parse_lspci_name(listing, "0x0300"), None);
        assert_eq!(parse_lspci_name(listing, "0x1234"), None);
        assert_eq!(parse_lspci_name(listing, "0x"), None);
    }
}
