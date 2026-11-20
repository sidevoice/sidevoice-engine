//! How DXGI's adapters become the NVIDIA state. Windows only, because DXGI is.

use super::{nvidia_among, Adapter, Nvidia, NVIDIA};

const INTEL: u32 = 0x8086;
const MICROSOFT: u32 = 0x1414;
const SOFTWARE: u32 = 2;

fn adapter(vendor: u32, flags: u32) -> Adapter {
    Adapter {
        vendor,
        flags,
        dedicated: 0,
        shared: 0,
    }
}

fn is_present(adapters: &[Adapter]) -> bool {
    matches!(nvidia_among(adapters), Nvidia::Present)
}

#[test]
fn an_nvidia_hardware_adapter_is_present() {
    assert!(is_present(&[adapter(INTEL, 0), adapter(NVIDIA, 0)]));
}

#[test]
fn no_adapter_or_another_vendors_is_no_adapter() {
    assert!(!is_present(&[]));
    assert!(!is_present(&[
        adapter(INTEL, 0),
        adapter(MICROSOFT, SOFTWARE)
    ]));
}

#[test]
fn a_software_adapter_is_not_a_gpu_whoever_made_it() {
    assert!(!is_present(&[adapter(NVIDIA, SOFTWARE)]));
    assert!(!adapter(MICROSOFT, SOFTWARE).is_hardware());
    assert!(adapter(INTEL, 0).is_hardware());
}
