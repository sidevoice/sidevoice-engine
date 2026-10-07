//! VITS through sherpa-onnx's offline TTS: Piper's voices, each of one language, read with espeak-ng.

use std::path::Path;
use std::sync::Arc;

use super::c_api::OfflineTtsConfig;
use super::library::Api;
use super::synthesizer::Synthesizer;
use super::{c_string, path};
use crate::install::Installed;
use crate::Result;

/// Creates the TTS from the `model`, `tokens` and `espeak-ng-data` (a directory) in `files`, on `provider`, with the
/// library's default noise and length scales. A voice speaks its one language: it is told none.
pub(super) fn load(api: Arc<Api>, files: &Installed, provider: &str) -> Result<Synthesizer> {
    let model_path = path(files, "model")?;
    let model = c_string(model_path)?;
    let tokens = c_string(path(files, "tokens")?)?;
    let data_dir = c_string(path(files, "espeak-ng-data")?)?;
    let provider = c_string(provider)?;
    let mut config = OfflineTtsConfig::default();
    config.model.vits.model = model.as_ptr();
    config.model.vits.tokens = tokens.as_ptr();
    config.model.vits.data_dir = data_dir.as_ptr();
    config.model.provider = provider.as_ptr();
    Synthesizer::new(api, &mut config, Some(Path::new(model_path)), |_| None)
}
