use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};

use super::Nvidia;

#[cfg(test)]
mod tests;

/// NVIDIA's PCI vendor id, as DXGI reports it.
const NVIDIA: u32 = 0x10DE;

/// One graphics adapter, as DXGI describes it: what the engine reads of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Adapter {
    /// The PCI vendor id.
    pub vendor: u32,
    /// The `DXGI_ADAPTER_FLAG`s.
    pub flags: u32,
    /// Dedicated video memory, in bytes.
    pub dedicated: u64,
    /// System memory the adapter may share, in bytes.
    pub shared: u64,
}

impl Adapter {
    /// Whether it is hardware: software adapters (WARP, which carries Microsoft's id) are never counted.
    pub fn is_hardware(&self) -> bool {
        self.flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0
    }
}

/// The adapters DXGI lists, none when the factory cannot be made.
pub fn adapters() -> Option<Vec<Adapter>> {
    // SAFETY: `CreateDXGIFactory1` takes no pointer from us; the factory is released when dropped.
    let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }.ok()?;
    // Enumeration ends with an error (`DXGI_ERROR_NOT_FOUND`) after the last adapter.
    Some(
        (0..)
            .map_while(|index| {
                // SAFETY: `EnumAdapters1` and `GetDesc1` only read the adapter the factory owns.
                let adapter = unsafe { factory.EnumAdapters1(index) }.ok()?;
                let desc = unsafe { adapter.GetDesc1() }.ok()?;
                Some(Adapter {
                    vendor: desc.VendorId,
                    flags: desc.Flags,
                    dedicated: desc.DedicatedVideoMemory as u64,
                    shared: desc.SharedSystemMemory as u64,
                })
            })
            .collect(),
    )
}

/// Windows: the graphics adapters DXGI lists. The system itself says which adapters there are and who makes them; no
/// vendor library is loaded to find out (a library of NVIDIA's would be searched for, and its dependencies too, in places
/// an attacker may write to). An NVIDIA adapter means NVIDIA's display driver is installed and running on it; any
/// hardware adapter has a Vulkan driver with its display driver (NVIDIA, AMD and Intel all ship one).
pub struct Os;

impl super::Accelerators for Os {
    const APPLE: bool = false;

    fn nvidia() -> Nvidia {
        adapters().map_or(Nvidia::Absent, |adapters| nvidia_among(&adapters))
    }

    fn vulkan() -> bool {
        adapters().is_some_and(|adapters| adapters.iter().any(Adapter::is_hardware))
    }
}

/// Whether one of `adapters` is NVIDIA's hardware.
fn nvidia_among(adapters: &[Adapter]) -> Nvidia {
    let found = adapters
        .iter()
        .any(|adapter| adapter.vendor == NVIDIA && adapter.is_hardware());
    if found {
        Nvidia::Present
    } else {
        Nvidia::NoAdapter
    }
}
