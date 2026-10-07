use super::Platform;
use crate::host::{Capabilities, Runs};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn a_platform_is_the_os_and_architecture_or_the_web() {
    let caps = |runs, os: &str, arch: &str| Capabilities {
        runs,
        os: os.to_owned(),
        arch: arch.to_owned(),
        accelerators: Vec::new(),
        memory_mb: None,
        cores: None,
    };
    let of = |runs, os, arch| Platform::of(&caps(runs, os, arch));
    assert_eq!(
        of(Runs::Native, "macos", "aarch64"),
        Some(Platform::MacosAarch64)
    );
    assert_eq!(
        of(Runs::Native, "windows", "x86_64"),
        Some(Platform::WindowsX86_64)
    );
    assert_eq!(of(Runs::Page, "linux", "x86_64"), Some(Platform::Web));
    assert_eq!(of(Runs::Native, "freebsd", "x86_64"), None);
}
