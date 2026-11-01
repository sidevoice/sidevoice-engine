//! The constants of `host` against the raw conditions and `std::env::consts` they stand for: what the engine's other
//! tests take as given, so this is where a wrong `apple_silicon` alias would show up.

use super::{APPLE_SILICON, ARCH, OS};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn it_names_the_host_as_the_compiler_does() {
    assert_eq!(OS, std::env::consts::OS);
    assert_eq!(ARCH, std::env::consts::ARCH);
    assert_eq!(
        APPLE_SILICON,
        cfg!(all(target_os = "macos", target_arch = "aarch64"))
    );
}
