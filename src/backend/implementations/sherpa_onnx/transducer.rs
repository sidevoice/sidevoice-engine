//! Transducers through sherpa-onnx's offline recognizer: the FastConformer and Parakeet models NeMo exports (sherpa-onnx
//! reads which from the model's metadata). They take no language: a multilingual one hears whichever is spoken.

use std::sync::Arc;

use super::c_api::OfflineRecognizerConfig;
use super::library::Api;
use super::recognizer::Recognizer;
use super::{c_string, path};
use crate::install::Installed;
use crate::Result;

/// Creates the recognizer from the `encoder`, `decoder`, `joiner` and `tokens` in `files`, on `provider`.
pub(super) fn load(api: Arc<Api>, files: &Installed, provider: &str) -> Result<Recognizer> {
    let encoder = c_string(path(files, "encoder")?)?;
    let decoder = c_string(path(files, "decoder")?)?;
    let joiner = c_string(path(files, "joiner")?)?;
    let tokens = c_string(path(files, "tokens")?)?;
    let provider = c_string(provider)?;
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer.encoder = encoder.as_ptr();
    config.model_config.transducer.decoder = decoder.as_ptr();
    config.model_config.transducer.joiner = joiner.as_ptr();
    config.model_config.tokens = tokens.as_ptr();
    config.model_config.provider = provider.as_ptr();
    // The library copies the strings while creating it: they need not outlive this call.
    Recognizer::new(api, &mut config)
}
