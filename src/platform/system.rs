//! What the engine knows of the system it runs on, as values: the OS name the host reports in its
//! [`Capabilities`](crate::host::Capabilities), and, for the tests, the accelerator class of Apple silicon. Only values differ per
//! system, but each system states them itself, in `system/<os>.rs`, so that a new system is a new file and nothing
//! that asks is changed. The page is not an operating system: its file says what the web build reports.

/// The values a system has.
pub trait System {
    /// The name the host reports as `Capabilities::os`: the values of `std::env::consts::OS` ("macos", "linux",
    /// "windows", "android", "ios"), and "web" in a page.
    const OS: &'static str;

    /// Whether this is a Mac on Apple silicon, which has the MLX backend and runs whisper.cpp on Metal, a GPU, before
    /// sherpa-onnx on the CPU. A Mac on Intel and every other system say `false`.
    #[cfg(test)]
    const APPLE_SILICON: bool;
}

#[cfg(web)]
mod web;
#[cfg(web)]
pub use web::Os as Current;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::Os as Current;

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
pub use ios::Os as Current;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Os as Current;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Os as Current;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Os as Current;

#[cfg(test)]
mod tests;
