use std::fs;
use std::path::Path;

use super::Nvidia;
use crate::platform::render_nodes;

/// Where Vulkan loaders look for the manifests of the installed drivers (ICDs).
const ICD_DIRECTORIES: [&str; 3] = [
    "/usr/share/vulkan/icd.d",
    "/etc/vulkan/icd.d",
    "/usr/local/share/vulkan/icd.d",
];

/// Linux: what the NVIDIA kernel driver shows in `/proc/driver/nvidia`: its `version` once loaded, and a folder per
/// device in `gpus`; and what the kernel lists of other GPUs (render nodes, [`render_nodes`]) with the drivers that
/// are installed for Vulkan (the manifests in [`ICD_DIRECTORIES`]).
pub struct Os;

impl super::Accelerators for Os {
    const APPLE: bool = false;

    fn nvidia() -> Nvidia {
        let proc = Path::new("/proc/driver/nvidia");
        if !proc.join("version").exists() {
            return Nvidia::Absent;
        }
        // A device read properly: an unreadable folder or entry shows nothing.
        match fs::read_dir(proc.join("gpus")).map(|mut devices| devices.next()) {
            Ok(Some(Ok(_))) => Nvidia::Present,
            _ => Nvidia::NoAdapter,
        }
    }

    fn vulkan() -> bool {
        let drivers = ICD_DIRECTORIES
            .iter()
            .map(|directory| manifests_in(Path::new(directory)))
            .sum();
        vulkan_from(
            matches!(Self::nvidia(), Nvidia::Present),
            render_nodes::list().len(),
            drivers,
        )
    }
}

/// Whether a Vulkan GPU is there: an NVIDIA device (its proprietary driver brings its own Vulkan driver), or a render
/// node and at least one installed driver. A driver without a render node is a software rasterizer (Mesa's lavapipe),
/// which is not a GPU.
fn vulkan_from(nvidia: bool, render_nodes: usize, drivers: usize) -> bool {
    nvidia || (render_nodes > 0 && drivers > 0)
}

/// The driver manifests (`*.json`) in `directory`; none when it cannot be read.
fn manifests_in(directory: &Path) -> usize {
    fs::read_dir(directory).map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .count()
    })
}

#[cfg(test)]
mod tests;
