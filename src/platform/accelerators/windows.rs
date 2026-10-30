use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};

use super::Nvidia;

#[cfg(test)]
mod tests;

/// NVIDIA's PCI vendor id, as DXGI reports it.
const NVIDIA: u32 = 0x10DE;

/// Windows: the graphics adapters DXGI lists. The system itself says which adapters there are and who makes them; no
/// vendor library is loaded to find out (a library of NVIDIA's would be searched for, and its dependencies too, in places
/// an attacker may write to). An NVIDIA adapter means NVIDIA's display driver is installed and running on it.
pub struct Os;

impl super::Accelerators for Os {
    const APPLE: bool = false;

    fn nvidia() -> Nvidia {
        // SAFETY: `CreateDXGIFactory1` takes no pointer from us; the factory is released when dropped.
        let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
            return Nvidia::Absent;
        };
        // Enumeration ends with an error (`DXGI_ERROR_NOT_FOUND`) after the last adapter.
        let adapters = (0..).map_while(|index| {
            // SAFETY: `EnumAdapters1` and `GetDesc1` only read the adapter the factory owns.
            let adapter = unsafe { factory.EnumAdapters1(index) }.ok()?;
            let desc = unsafe { adapter.GetDesc1() }.ok()?;
            Some((desc.VendorId, desc.Flags))
        });
        nvidia_among(adapters)
    }
}

/// Whether one of `adapters` (vendor id and `DXGI_ADAPTER_FLAG`s) is NVIDIA's hardware: software adapters (WARP, which
/// carries Microsoft's id) are never counted.
fn nvidia_among(adapters: impl IntoIterator<Item = (u32, u32)>) -> Nvidia {
    let software = DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32;
    let found = adapters
        .into_iter()
        .any(|(vendor, flags)| vendor == NVIDIA && flags & software == 0);
    if found {
        Nvidia::Present
    } else {
        Nvidia::NoAdapter
    }
}
