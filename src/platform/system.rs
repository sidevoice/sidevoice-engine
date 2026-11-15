//! What the engine knows of the system it runs on, as a value: the OS name the host reports in its
//! [`Capabilities`](crate::host::Capabilities). Each system states it itself, in `system/<os>.rs`, so that a new system
//! is a new file and nothing that asks is changed. The page is not an operating system: its file says what the web
//! build reports.

#[cfg(web)]
mod web;
#[cfg(web)]
pub(crate) use web::OS;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub(crate) use android::OS;

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
pub(crate) use ios::OS;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::OS;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::OS;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::OS;

#[cfg(test)]
mod tests;
