//! The GPU whisper.cpp runs on through Vulkan, and the way the binary reaches the Vulkan loader.
//!
//! Only Windows and Linux on x86_64 build ggml's Vulkan backend (Cargo.toml: `whisper-rs`'s `vulkan` feature; the
//! condition is spelled out side by side here and in `local_engines::WHISPER_CPP_VULKAN`: change them together). Every
//! other build says there is no Vulkan GPU.
//!
//! # The loader is looked up when it is needed, never linked
//!
//! ggml's Vulkan backend calls one function of the loader by name, `vkGetInstanceProcAddr`, and gets every other
//! through it. whisper-rs-sys links the loader (`-lvulkan`, `vulkan-1.lib`) for that one symbol, so a binary that links
//! it would not start on a machine without the loader (`libvulkan.so.1`, `vulkan-1.dll`). This module defines that
//! symbol itself, so the linker never needs the loader's: the first call opens the loader (on Linux through the
//! system's library search, on Windows only from System32, never from the application's folder or `PATH`) and forwards
//! to it. Without a loader the answer for the four commands that need no instance (`vkEnumerateInstanceVersion`,
//! `vkEnumerateInstanceExtensionProperties`, `vkEnumerateInstanceLayerProperties`, `vkCreateInstance`) is a function
//! that fails with `VK_ERROR_INITIALIZATION_FAILED`: ggml registers its backends the first time any device is listed
//! (any whisper.cpp load, on the CPU too), turns that failure into an exception it catches itself and registers no
//! Vulkan device, so whisper.cpp runs on the CPU. ggml 1.8.3 has no switch to leave a built-in backend out, and a
//! loader that cannot be found would otherwise end the process (Windows' delayed-load failure is a structured
//! exception C++'s `catch (...)` does not see).
//!
//! # The GPU
//!
//! [`gpu`] is the first device of ggml's own list that is a GPU: the one whisper.cpp takes (`gpu_device` 0). Asking
//! ggml, not the OS, is what makes it the device a model really gets: a driver the loader cannot use, a software
//! rasterizer (ggml keeps only discrete and integrated GPUs) or a GPU ggml rejects is not on the list.

#![cfg_attr(
    not(any(
        all(windows, target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64")
    )),
    allow(
        dead_code,
        reason = "the builds without ggml's Vulkan backend list no GPU, and only the Windows and Linux hosts ask"
    )
)]

/// A GPU, as ggml's Vulkan backend lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gpu {
    /// The driver's name for the device.
    pub name: String,
    /// Whether it shares the system's memory (an integrated GPU) rather than having its own.
    pub integrated: bool,
    /// The memory of its heaps, in bytes: its own memory when discrete; the system's heaps when integrated.
    pub total_bytes: u64,
}

impl Gpu {
    /// The memory a model can count on: a discrete GPU's own. An integrated GPU's figure is the system's memory,
    /// which the CPU needs too, so it counts for none: models stay on the CPU there.
    #[must_use]
    pub fn model_bytes(&self) -> Option<u64> {
        (!self.integrated && self.total_bytes > 0).then_some(self.total_bytes)
    }
}

/// Whether ggml's Vulkan backend lists a GPU whisper.cpp can run on.
#[must_use]
pub fn present() -> bool {
    gpu().is_some()
}

/// The memory models can use on the GPU whisper.cpp runs on ([`Gpu::model_bytes`]); `None` without a GPU.
#[must_use]
pub fn model_bytes() -> Option<u64> {
    gpu().and_then(Gpu::model_bytes)
}

#[cfg(any(
    all(windows, target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64")
))]
mod loader;
#[cfg(any(
    all(windows, target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64")
))]
pub use loader::gpu;

/// No Vulkan backend is built here.
#[cfg(not(any(
    all(windows, target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64")
)))]
#[must_use]
pub fn gpu() -> Option<&'static Gpu> {
    None
}

#[cfg(all(
    test,
    any(
        all(windows, target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64")
    )
))]
mod tests;
