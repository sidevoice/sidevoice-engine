//! Whether the GPU keeps serving an app once it is in the background: not on iOS, where a Metal command buffer an app
//! commits while it is not in the foreground fails, and whisper.cpp then aborts the process (during a call on the
//! Dynamic Island, with the phone locked; sidevoice-engine#153, #154). Only a value differs, so there is no trait.
//!
//! Android has not been seen to fail (sidevoice-web#572 asks to check it); it says `false` until it is. Desktops keep
//! the GPU for a window that is hidden or minimised.

/// iOS stops serving the GPU to an app in the background.
#[cfg(target_os = "ios")]
pub const GPU_STOPS_IN_BACKGROUND: bool = true;

/// Every other system keeps serving it.
#[cfg(not(target_os = "ios"))]
pub const GPU_STOPS_IN_BACKGROUND: bool = false;
