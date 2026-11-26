//! The only place the native build touches the operating system. Everything else is host-agnostic.
//!
//! One module per concern: `links` (hard-link counts), `accelerators` (what accelerators the OS shows), `background`
//! (whether the GPU keeps serving an app in the background), `memory` (what it shows of memory, in MiB),
//! `local_engines` (which backends' libraries it can build), `narrow_paths` (whether libraries that read paths as ANSI
//! can open one), `vulkan` (the GPU whisper.cpp runs on through Vulkan, and how the binary reaches the loader) and `system` (the OS name the host reports, and the Apple silicon class). Each declares its trait and
//! re-exports the type of the OS being compiled, which lives in `<concern>/<os>.rs`. All are native but
//! `local_engines`, which the web build's backend list also names, and `system`, which the web build only tests.

#[cfg(native)]
pub mod accelerators;
#[cfg(native)]
pub mod background;
#[cfg(native)]
pub mod links;
pub mod local_engines;
#[cfg(native)]
pub mod memory;
#[cfg(native)]
pub mod narrow_paths;
#[cfg(any(native, test))]
pub mod system;
#[cfg(native)]
pub mod vulkan;
