//! The only place the native build touches the operating system. Everything else is host-agnostic.
//!
//! One module per concern: `links` (hard-link counts) and `accelerators` (what accelerators the OS shows). Each declares
//! its trait and re-exports the type of the OS being compiled, which lives in `<concern>/<os>.rs`.

pub mod accelerators;
pub mod links;
