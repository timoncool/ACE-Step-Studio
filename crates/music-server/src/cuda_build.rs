//! Which CUDA build of the engine this machine runs.
//!
//! The engine ships one CUDA backend per toolkit. CUDA 13 targets Turing and
//! newer and needs a driver from its own release on; CUDA 12 carries the
//! Maxwell, Pascal and Volta cards CUDA 13 dropped, and every newer card whose
//! driver predates CUDA 13. Both hold device code for each architecture, so no
//! driver ever compiles PTX.

use std::{process::Command, sync::OnceLock};

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CudaBuild {
    Cuda12,
    Cuda13,
}

impl CudaBuild {
    /// The folder beside mm-server.exe that holds this build's ggml-cuda.dll.
    pub fn folder(self) -> &'static str {
        match self {
            CudaBuild::Cuda12 => "cuda12",
            CudaBuild::Cuda13 => "cuda13",
        }
    }
}

/// The first driver of each CUDA major line, from NVIDIA's CUDA compatibility
/// tables: minor version compatibility runs a whole major line on it, and
/// device code needs nothing newer.
const CUDA13_DRIVER: u32 = 580;
const CUDA12_DRIVER: u32 = 525;

/// The oldest architecture the CUDA 12 build has device code for: 5.2, the
/// Maxwell of the GTX 900 series and the Tesla M40.
const CUDA12_OLDEST: (u32, u32) = (5, 2);
/// Turing, the oldest architecture CUDA 13 still targets.
const CUDA13_OLDEST: (u32, u32) = (7, 5);

/// Picks the build from the card's compute capability and the driver's major
/// version, the way Ollama chooses between its CUDA runners.
pub fn cuda_build(compute: (u32, u32), driver_major: u32) -> Option<CudaBuild> {
    if compute >= CUDA13_OLDEST && driver_major >= CUDA13_DRIVER {
        Some(CudaBuild::Cuda13)
    } else if compute >= CUDA12_OLDEST && driver_major >= CUDA12_DRIVER {
        Some(CudaBuild::Cuda12)
    } else {
        None
    }
}

/// The first NVIDIA card's compute capability and driver major version,
/// probed once: the machine's card does not change inside one process.
fn device() -> Option<((u32, u32), u32)> {
    static DEVICE: OnceLock<Option<((u32, u32), u32)>> = OnceLock::new();
    *DEVICE.get_or_init(|| {
        let mut command = Command::new("nvidia-smi");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // A GUI process spawning a console tool flashes a window without this.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        // A driver too old to know compute_cap fails the query, and such a
        // driver runs neither build.
        let output = command.args(["--query-gpu=compute_cap,driver_version", "--format=csv,noheader"]).output().ok()?;
        if !output.status.success() {
            return None;
        }
        parse_query(String::from_utf8_lossy(&output.stdout).lines().next()?.trim())
    })
}

/// `7.5, 581.29` into the compute capability and the driver's major version.
fn parse_query(line: &str) -> Option<((u32, u32), u32)> {
    let (compute, driver) = line.split_once(',')?;
    let (major, minor) = compute.trim().split_once('.')?;
    let driver_major = driver.trim().split('.').next()?.parse().ok()?;
    Some(((major.parse().ok()?, minor.parse().ok()?), driver_major))
}

/// Whether the machine has an NVIDIA card with a working driver, whether or
/// not a CUDA build runs it.
pub fn nvidia_card() -> bool {
    static PRESENT: OnceLock<bool> = OnceLock::new();
    *PRESENT.get_or_init(|| {
        let mut command = Command::new("nvidia-smi");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        command.arg("-L").output().is_ok_and(|output| output.status.success())
    })
}

/// The build this machine runs, none without an NVIDIA card one of the builds
/// supports.
pub fn current() -> Option<CudaBuild> {
    device().and_then(|(compute, driver)| cuda_build(compute, driver))
}

/// Tensor cores before Ampere accumulate in FP16, where the V projection can
/// overflow to infinity and poison every later attention; the engine's clamp
/// keeps it in range and changes nothing on a card that never overflows.
pub fn accumulates_in_fp16() -> bool {
    device().is_some_and(|(compute, _)| compute < (8, 0))
}

/// Why the engine cannot start on CUDA here, in words for the person at the
/// machine.
pub const UNSUPPORTED: &str = "CUDA was chosen, but this card or its driver runs neither CUDA build of the engine: it needs an NVIDIA card from the GTX 900 series on and driver 525 or newer. Update the NVIDIA driver, or choose Vulkan in Settings.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_follows_the_architecture_and_the_driver() {
        assert_eq!(cuda_build((7, 5), 581), Some(CudaBuild::Cuda13));
        assert_eq!(cuda_build((12, 0), 590), Some(CudaBuild::Cuda13));
        assert_eq!(cuda_build((6, 1), 581), Some(CudaBuild::Cuda12));
        assert_eq!(cuda_build((5, 2), 560), Some(CudaBuild::Cuda12));
        assert_eq!(cuda_build((7, 0), 552), Some(CudaBuild::Cuda12));
        assert_eq!(cuda_build((8, 9), 566), Some(CudaBuild::Cuda12));
        assert_eq!(cuda_build((3, 5), 581), None);
        assert_eq!(cuda_build((5, 0), 581), None);
        assert_eq!(cuda_build((8, 6), 511), None);
    }

    #[test]
    fn the_query_reads_the_capability_and_the_driver_major() {
        assert_eq!(parse_query("7.5, 581.29"), Some(((7, 5), 581)));
        assert_eq!(parse_query("12.0, 591.44"), Some(((12, 0), 591)));
        assert_eq!(parse_query("[N/A], 581.29"), None);
    }
}
