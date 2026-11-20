//! Linux: the GPUs the kernel lists under `/sys/class/drm`, one per render node (`renderD*`), as far as the kernel
//! says who makes them and how much memory they have. Read by `accelerators` (is there a GPU a Vulkan driver can use)
//! and by `memory` (how much it can give models), so that the sysfs layout has one reader.

use std::fs;
use std::path::Path;

#[cfg(test)]
mod tests;

/// PCI vendor ids, as sysfs shows them.
pub const AMD: u32 = 0x1002;
pub const INTEL: u32 = 0x8086;

/// One render node's GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderNode {
    /// The PCI vendor id, `None` when the kernel does not show it.
    pub vendor: Option<u32>,
    /// The dedicated video memory in bytes, where the driver says it (`amdgpu`); `None` otherwise.
    pub vram: Option<u64>,
    /// The system memory the GPU may map (`amdgpu`'s GTT), in bytes; `None` where the driver does not say.
    pub gtt: Option<u64>,
}

/// The render nodes this machine lists.
#[must_use]
pub fn list() -> Vec<RenderNode> {
    list_in(Path::new("/sys/class/drm"))
}

/// The render nodes listed in `drm`, a directory laid out as `/sys/class/drm`.
fn list_in(drm: &Path) -> Vec<RenderNode> {
    let Ok(entries) = fs::read_dir(drm) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("renderD"))
        .map(|entry| {
            let device = entry.path().join("device");
            RenderNode {
                vendor: read(&device.join("vendor")).and_then(|text| hex(&text)),
                vram: read(&device.join("mem_info_vram_total"))
                    .and_then(|text| text.trim().parse().ok()),
                gtt: read(&device.join("mem_info_gtt_total"))
                    .and_then(|text| text.trim().parse().ok()),
            }
        })
        .collect()
}

fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// A sysfs hexadecimal id (`0x1002`).
fn hex(text: &str) -> Option<u32> {
    u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
}
