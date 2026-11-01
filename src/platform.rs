//! The only place the native build touches the operating system. Everything else is host-agnostic.
//!
//! One module per concern: `links` (hard-link counts), `accelerators` (what accelerators the OS shows) and
//! `narrow_paths` (whether libraries that read paths as ANSI can open one). Each declares its trait and re-exports the
//! type of the OS being compiled, which lives in `<concern>/<os>.rs`. And `host`, the names of the OS and architecture
//! and whether it is Apple silicon: plain constants, the same on every OS, that the tests of the wasm build read too.

#[cfg(native)]
pub mod accelerators;
pub mod host;
#[cfg(native)]
pub mod links;
#[cfg(native)]
pub mod narrow_paths;
