use crate::host::Accelerator;
use crate::platform::render_nodes::{self, RenderNode, AMD, INTEL};

use super::Observer;

const GIB: u64 = 1024 * 1024 * 1024;

/// Linux: the machine's memory and its control group's limit, from `sysinfo` (`/proc/meminfo`, `/sys/fs/cgroup`). The
/// GPUs' memory is what the kernel shows of its render nodes ([`render_nodes`]).
pub struct Os;

impl Observer for Os {
    /// Vulkan, on the render node with the most memory ([`usable_bytes`]). An NVIDIA GPU on its proprietary driver
    /// shows no memory in sysfs, so it has none here: no model runs on it until a probe of the Vulkan device says
    /// (sidevoice-engine#20); the CPU runs it.
    fn accelerator_bytes() -> Vec<(Accelerator, u64)> {
        let system = Self::physical_bytes().unwrap_or(0);
        let best = render_nodes::list()
            .iter()
            .filter_map(|node| usable_bytes(node, system))
            .max();
        best.map(|bytes| (Accelerator::Vulkan, bytes))
            .into_iter()
            .collect()
    }
}

/// What models can use of a render node, `None` when nothing shows it. AMD: its video memory, which is all of a
/// discrete GPU's; an APU has a carve-out under 1 GiB and maps system memory (GTT), which is added. Intel's GPUs
/// show no memory in sysfs: an integrated one works in the system's memory, and half of it is counted here as an
/// estimate.
fn usable_bytes(node: &RenderNode, system: u64) -> Option<u64> {
    match node.vendor {
        Some(AMD) => {
            let vram = node.vram?;
            Some(if vram >= GIB {
                vram
            } else {
                vram + node.gtt.unwrap_or(0)
            })
        }
        Some(INTEL) => Some(system / 2).filter(|bytes| *bytes > 0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{usable_bytes, RenderNode, AMD, GIB, INTEL};

    fn node(vendor: u32, vram: Option<u64>, gtt: Option<u64>) -> RenderNode {
        RenderNode {
            vendor: Some(vendor),
            vram,
            gtt,
        }
    }

    #[test]
    fn a_discrete_amd_gpu_gives_its_video_memory() {
        let node = node(AMD, Some(16 * GIB), Some(8 * GIB));
        assert_eq!(usable_bytes(&node, 32 * GIB), Some(16 * GIB));
    }

    #[test]
    fn an_amd_apu_gives_its_carve_out_and_its_mappable_system_memory() {
        let node = node(AMD, Some(GIB / 2), Some(8 * GIB));
        assert_eq!(usable_bytes(&node, 16 * GIB), Some(GIB / 2 + 8 * GIB));
    }

    #[test]
    fn intel_gives_half_of_the_system_memory_and_an_unknown_gpu_nothing() {
        assert_eq!(
            usable_bytes(&node(INTEL, None, None), 16 * GIB),
            Some(8 * GIB)
        );
        assert_eq!(usable_bytes(&node(INTEL, None, None), 0), None);
        assert_eq!(usable_bytes(&node(0x10DE, None, None), 16 * GIB), None);
        assert_eq!(usable_bytes(&node(AMD, None, None), 16 * GIB), None);
    }
}
