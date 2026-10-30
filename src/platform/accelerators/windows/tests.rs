//! How DXGI's adapters become the NVIDIA state. Windows only, because DXGI is.

use super::{nvidia_among, Nvidia, NVIDIA};

const INTEL: u32 = 0x8086;
const MICROSOFT: u32 = 0x1414;
const SOFTWARE: u32 = 2;

fn is_present(adapters: &[(u32, u32)]) -> bool {
    matches!(nvidia_among(adapters.iter().copied()), Nvidia::Present)
}

#[test]
fn an_nvidia_hardware_adapter_is_present() {
    assert!(is_present(&[(INTEL, 0), (NVIDIA, 0)]));
}

#[test]
fn no_adapter_or_another_vendors_is_no_adapter() {
    assert!(!is_present(&[]));
    assert!(!is_present(&[(INTEL, 0), (MICROSOFT, SOFTWARE)]));
}

#[test]
fn a_software_adapter_is_not_a_gpu_whoever_made_it() {
    assert!(!is_present(&[(NVIDIA, SOFTWARE)]));
}
