//! Backends: what runs models (sherpa-onnx, whisper.cpp, MLX, transformers.js, ...). A backend describes itself as
//! data; the engine does the matching, ranking, selection and installing for every backend alike (`crate::resolver`,
//! `crate::install`). Which models a backend runs is the catalogue's to say, and which files it downloads is data too.
//!
//! This file is only the package's index:
//! - `contract`: the interface every backend implements ([`Backend`], [`BackendSpec`]);
//! - `requirement`: what the machine must meet ([`Requirement`]) and the common requirements;
//! - `registry`: how the backends of this build are found ([`built_in`]);
//! - `implementations`: one file per backend.

mod contract;
mod implementations;
mod registry;
mod requirement;

pub use contract::{Backend, BackendId, BackendSpec};
pub use registry::{built_in, BackendFactory};
pub use requirement::{MinCores, MinMemoryMb, Requirement};
