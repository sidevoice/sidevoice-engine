//! The only place the native build touches the operating system. Everything else is host-agnostic.
//!
//! One module per concern: `links` (hard-link counts), `accelerators` (what accelerators the OS shows), `local_engines`
//! (which backends' libraries it can build) and `narrow_paths` (whether libraries that read paths as ANSI can open
//! one). Each declares its trait and re-exports the type of the OS being compiled, which lives in `<concern>/<os>.rs`.
//! All are native but `local_engines`, which the web build's backend list also names.

#[cfg(native)]
pub mod accelerators;
#[cfg(native)]
pub mod links;
pub mod local_engines;
#[cfg(native)]
pub mod narrow_paths;
