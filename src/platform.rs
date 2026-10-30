//! The only place the native build touches the operating system's file API. Everything else is host-agnostic.
//!
//! One module per concern: `links` (hard-link counts), `narrow_paths` (paths for libraries that read them as ANSI). Each declares its trait and re-exports the type of the OS being
//! compiled, which lives in `<concern>/unix.rs` or `<concern>/windows.rs`.

pub mod links;
pub mod narrow_paths;
