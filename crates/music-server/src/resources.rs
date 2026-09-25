//! Live machine resources for the studio's resource monitor.
//!
//! ACE Studio had a resource readout and it was worth keeping: local music
//! generation is the most VRAM-hungry thing on the machine, and a user who
//! cannot see VRAM pressure has no way to tell "still loading the DiT" from
//! "about to run out of memory". Every number here is measured — nothing is
//! estimated, and a metric that cannot be read is reported as absent rather
//! than as zero.

use std::{process::Command, sync::Mutex, time::Duration};

use serde::Serialize;
use sysinfo::{ProcessesToUpdate, System};

#[derive(Debug, Clone, Serialize)]
pub struct GpuSnapshot {
    pub name: String,
    pub vram_used_mb: u64,
    pub vram_total_mb: u64,
    /// Percent of the sampling period during which the GPU was busy.
    pub utilization_percent: Option<u8>,
    pub temperature_c: Option<u8>,
    pub power_draw_w: Option<f32>,
    pub power_limit_w: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessSnapshot {
    pub name: String,
    pub memory_mb: u64,
    pub cpu_percent: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceSnapshot {
    pub cpu_percent: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub gpus: Vec<GpuSnapshot>,
    /// The native engine process, when it is running on this machine.
    pub engine_process: Option<ProcessSnapshot>,
    /// Resident memory of the studio server itself.
    pub studio_process_mb: u64,
}

/// `sysinfo` reports CPU load as the delta between two refreshes, so the
/// sampler has to be kept alive between requests. A first call therefore
/// reports 0% CPU; every later call reports the load since the previous one.
static SAMPLER: Mutex<Option<System>> = Mutex::new(None);

pub fn snapshot() -> ResourceSnapshot {
    let mut guard = SAMPLER.lock().unwrap_or_else(|error| error.into_inner());
    if guard.is_none() {
        // The very first CPU reading has no previous sample to diff against and
        // would report a meaningless 100%. Prime it before the caller sees it.
        let mut primed = System::new();
        primed.refresh_cpu_usage();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        *guard = Some(primed);
    }
    let system = guard.as_mut().expect("sampler was primed above");
    system.refresh_memory();
    system.refresh_cpu_usage();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let engine_process = system
        .processes()
        .values()
        .find(|process| {
            let name = process.name().to_string_lossy().to_ascii_lowercase();
            name == "mm-server" || name == "mm-server.exe"
        })
        .map(|process| ProcessSnapshot {
            name: process.name().to_string_lossy().into_owned(),
            memory_mb: process.memory() / 1_048_576,
            cpu_percent: process.cpu_usage(),
        });

    let studio_process_mb = sysinfo::get_current_pid()
        .ok()
        .and_then(|pid| system.process(pid))
        .map(|process| process.memory() / 1_048_576)
        .unwrap_or_default();

    ResourceSnapshot {
        cpu_percent: system.global_cpu_usage(),
        ram_used_mb: system.used_memory() / 1_048_576,
        ram_total_mb: system.total_memory() / 1_048_576,
        gpus: nvidia_gpus().unwrap_or_default(),
        engine_process,
        studio_process_mb,
    }
}

fn nvidia_gpus() -> Option<Vec<GpuSnapshot>> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.used,memory.total,utilization.gpu,temperature.gpu,power.draw,power.limit",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(parse_gpu_line)
            .collect(),
    )
}

fn parse_gpu_line(line: &str) -> Option<GpuSnapshot> {
    let fields: Vec<&str> = line.split(',').map(str::trim).collect();
    if fields.len() < 3 || fields[0].is_empty() {
        return None;
    }
    Some(GpuSnapshot {
        name: fields[0].to_owned(),
        vram_used_mb: fields[1].parse().ok()?,
        vram_total_mb: fields[2].parse().ok()?,
        // `[N/A]` appears on cards that do not report these counters.
        utilization_percent: fields.get(3).and_then(|value| value.parse().ok()),
        temperature_c: fields.get(4).and_then(|value| value.parse().ok()),
        power_draw_w: fields.get(5).and_then(|value| value.parse().ok()),
        power_limit_w: fields.get(6).and_then(|value| value.parse().ok()),
    })
}

/// Sampling interval the UI should poll at. One second matches the resolution
/// `nvidia-smi` itself reports and keeps the process list cheap.
pub const SUGGESTED_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_nvidia_smi_row() {
        let gpu = parse_gpu_line("NVIDIA GeForce RTX 4090, 8244, 24564, 37, 51, 142.35, 450.00").unwrap();
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 4090");
        assert_eq!(gpu.vram_used_mb, 8244);
        assert_eq!(gpu.vram_total_mb, 24564);
        assert_eq!(gpu.utilization_percent, Some(37));
        assert_eq!(gpu.temperature_c, Some(51));
        assert_eq!(gpu.power_draw_w, Some(142.35));
        assert_eq!(gpu.power_limit_w, Some(450.0));
    }

    /// Unsupported counters must be absent, never silently reported as zero.
    #[test]
    fn missing_counters_are_absent_rather_than_zero() {
        let gpu = parse_gpu_line("NVIDIA T400, 100, 2048, [N/A], [N/A], [N/A], [N/A]").unwrap();
        assert_eq!(gpu.vram_total_mb, 2048);
        assert_eq!(gpu.utilization_percent, None);
        assert_eq!(gpu.temperature_c, None);
        assert_eq!(gpu.power_draw_w, None);
    }

    #[test]
    fn rejects_truncated_rows() {
        assert!(parse_gpu_line("").is_none());
        assert!(parse_gpu_line("NVIDIA, 100").is_none());
    }

    #[test]
    fn a_snapshot_reports_real_totals() {
        let snapshot = snapshot();
        assert!(snapshot.ram_total_mb > 0);
        assert!(snapshot.ram_used_mb <= snapshot.ram_total_mb);
    }
}
