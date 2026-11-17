//! The only place `xtask` asks which operating system it runs on: the build host, whose name the tasks print and
//! pick archives by. Only a value differs, so there is no trait.

/// The build host's operating system, as `std::env::consts::OS` names it ("linux", "macos", "windows").
pub(crate) const HOST_OS: &str = std::env::consts::OS;
