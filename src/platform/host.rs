//! What the host reports of the machine it runs on: the OS and architecture names as `std::env::consts` spell them
//! ("macos", "linux", "windows"; "aarch64", "x86_64"...), and whether that is Apple silicon. Nothing else asks the
//! compiler which platform it builds for, so production and tests read the same answer.
//!
//! Not behind `native`: the engine's tests on wasm32 read it too (to say what their fake host looks like).

/// The OS, as [`Capabilities::os`](crate::host::Capabilities) reports it on a native host.
pub const OS: &str = std::env::consts::OS;

/// The architecture, as [`Capabilities::arch`](crate::host::Capabilities) reports it on a native host.
pub const ARCH: &str = std::env::consts::ARCH;

/// Whether this is macOS on an ARM chip, the one machine where Metal is a GPU the engine's backends can use.
pub const APPLE_SILICON: bool = cfg!(apple_silicon);

#[cfg(test)]
mod tests;
