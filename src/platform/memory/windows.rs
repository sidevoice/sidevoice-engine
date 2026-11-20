use crate::host::Accelerator;
use crate::platform::accelerators::windows::{adapters, Adapter};

/// Windows: the machine's memory from `sysinfo`; no control groups. The graphics adapters' memory is DXGI's.
pub struct Os;

const GIB: u64 = 1024 * 1024 * 1024;

impl super::Observer for Os {
    /// Vulkan, on the hardware adapter with the most memory ([`usable_bytes`]); DXGI says the same of every maker.
    fn accelerator_bytes() -> Vec<(Accelerator, u64)> {
        let best = adapters()
            .unwrap_or_default()
            .iter()
            .filter(|adapter| adapter.is_hardware())
            .map(usable_bytes)
            .max();
        best.map(|bytes| (Accelerator::Vulkan, bytes))
            .into_iter()
            .collect()
    }
}

/// What models can use of an adapter: its dedicated memory, which is all of a discrete GPU's. An integrated GPU
/// reports a small carve-out (under 1 GiB) and works in the system's memory, of which half is counted here as an
/// estimate (DXGI's `SharedSystemMemory` is the most it may map, not what is free).
fn usable_bytes(adapter: &Adapter) -> u64 {
    if adapter.dedicated >= GIB {
        adapter.dedicated
    } else {
        adapter.dedicated + adapter.shared / 2
    }
}

#[cfg(test)]
mod tests {
    use super::{usable_bytes, Adapter, GIB};

    fn adapter(dedicated: u64, shared: u64) -> Adapter {
        Adapter {
            vendor: 0,
            flags: 0,
            dedicated,
            shared,
        }
    }

    #[test]
    fn a_discrete_gpu_gives_its_dedicated_memory() {
        assert_eq!(usable_bytes(&adapter(8 * GIB, 16 * GIB)), 8 * GIB);
    }

    #[test]
    fn an_integrated_gpu_gives_its_carve_out_and_half_of_what_it_may_share() {
        assert_eq!(usable_bytes(&adapter(GIB / 4, 8 * GIB)), GIB / 4 + 4 * GIB);
    }
}
