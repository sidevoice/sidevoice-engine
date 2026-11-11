//! Whether the GPU keeps serving an app once it is in the background: not on iOS, where a Metal command buffer an app
//! commits while it is not in the foreground fails, and whisper.cpp then aborts the process (during a call on the
//! Dynamic Island, with the phone locked; sidevoice-engine#153, #154). Each OS says it in `background/<os>.rs`.

/// What an OS does with the GPU of an app in the background.
pub trait Background {
    /// Whether it stops serving the GPU to an app in the background, so that a model on the GPU stops working there.
    const GPU_STOPS_IN_BACKGROUND: bool;
}

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::Os as Machine;

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
pub use ios::Os as Machine;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Os as Machine;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Os as Machine;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Os as Machine;
