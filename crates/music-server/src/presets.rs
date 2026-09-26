use std::{process::Command, sync::OnceLock};

use music_core::{Capability, ExecutionMode, StudioConfiguration};
use serde::Serialize;
use sysinfo::System;

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
        .map(|preset| PresetView {
            id: preset.id,
            title: preset.title,
            subtitle: preset.subtitle,
            min_vram_gb: preset.min_vram_gb,
            profile_id: preset.profile_id,
            provider_mode: preset.provider_mode,
        })
        .collect()
}

/// `nvidia-smi` costs tens of milliseconds and the setup screen polls status
/// once per second while a download runs. The machine's GPU does not change
/// inside one process lifetime, so probe it once.
fn probe() -> &'static Hardware {
    static HARDWARE: OnceLock<Hardware> = OnceLock::new();
    HARDWARE.get_or_init(|| {
        let mut system = System::new();
        system.refresh_memory();
        let total_ram_gb = system.total_memory() as f64 / 1_000_000_000.0;
        let (gpu_name, total_vram_gb) = nvidia_smi().unwrap_or_else(|| ("No NVIDIA GPU detected".into(), 0.0));
        let has_gpu = total_vram_gb > 0.0;
        let (recommended, reason) = recommend_for_hardware(&gpu_name, total_vram_gb);
        Hardware { gpu_name, total_vram_gb, total_ram_gb, has_gpu, recommended, reason }
    })
}

pub fn hardware() -> Hardware {
    probe().clone()
}

/// VRAM tiers follow the weights of each complete four-component set plus room
/// for activations: the thresholds are the presets' own `min_vram_gb`.
fn recommend_for_hardware(gpu_name: &str, total_vram_gb: f64) -> (&'static str, String) {
    // The smallest set runs on Vulkan or the processor, where the engine
    // goes on its own when CUDA cannot hold it; cloud music stays a choice.
    if total_vram_gb <= 0.0 {
        return (
            "native-minimal",
            "No NVIDIA VRAM was detected; the smallest set runs on Vulkan or the processor.".into(),
        );
    }
    let preset = PRESETS
        .iter()
        .filter(|preset| preset.profile_id.is_some() && !preset.provider_mode)
        .find(|preset| total_vram_gb >= preset.min_vram_gb);
    match preset {
        Some(preset) => (preset.id, format!("{gpu_name} with {total_vram_gb:.1} GB VRAM matches {}", preset.title)),
        None => (
            "native-minimal",
            format!("{gpu_name} with {total_vram_gb:.1} GB VRAM holds no set; the smallest runs on the processor instead"),
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
    match preset_id {
        "native-full" => "native",
        "native-quality" => "quality-q8",
        "native-balanced" => "balanced",
        "native-efficient" => "recommended-light",
        "native-minimal" => "minimal",
        // A machine without usable local VRAM still needs a named local target
        // for the Model Manager; the quality set is the smallest set that is
        // not advertised as a speed compromise.
        _ => "quality-q8",
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
    Ok(PresetApplication { profile_id: preset.profile_id.map(str::to_owned), selected_profile_changed: true })
}

fn nvidia_smi() -> Option<(String, f64)> {
    let output = Command::new("nvidia-smi").args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]).output().ok()?;
    if !output.status.success() { return None; }
    let line = String::from_utf8_lossy(&output.stdout).lines().next()?.trim().to_owned();
    let (name, memory) = line.rsplit_once(',')?;
    Some((name.trim().into(), memory.trim().parse::<f64>().ok()? / 1024.0))
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

    #[test]
    fn recommendation_matches_named_cards_and_vram_tiers() {
        assert_eq!(recommend_for_hardware("NVIDIA GeForce RTX 5090", 31.8).0, "native-full");
        assert_eq!(recommend_for_hardware("NVIDIA GeForce RTX 4090", 23.99).0, "native-full");
        assert_eq!(recommend_for_hardware("RTX 4080", 15.9).0, "native-quality");
        assert_eq!(recommend_for_hardware("RTX 4070", 11.9).0, "native-balanced");
        assert_eq!(recommend_for_hardware("RTX 4060", 8.0).0, "native-efficient");
        assert_eq!(recommend_for_hardware("GTX 1660", 6.0).0, "native-minimal");
        assert_eq!(recommend_for_hardware("GT 1030", 2.0).0, "native-minimal");
        assert_eq!(recommend_for_hardware("No NVIDIA GPU detected", 0.0).0, "native-minimal");
    }

    #[test]
    fn every_local_tier_installs_a_profile() {
        for vram in [5.0, 8.0, 10.0, 16.0, 24.0, 32.0] {
            assert!(crate::model_manager::profile_exists(profile_for_preset(recommend_for_hardware("NVIDIA test card", vram).0)));
        }
    }
}
