//! VITS through sherpa-onnx's offline TTS: Piper's voices, each of one language, read with espeak-ng.

use sherpa_onnx::{OfflineTtsConfig, OfflineTtsModelConfig, OfflineTtsVitsModelConfig};

use super::path;
use super::synthesizer::Synthesizer;
use crate::install::Installed;
use crate::Result;

/// Creates the TTS from the `model`, `tokens` and `espeak-ng-data` (a directory) in `files`, on `provider`, with the
/// library's default noise and length scales. A voice speaks its one language: it is told none.
pub(super) fn load(files: &Installed, provider: &str) -> Result<Synthesizer> {
    let model = path(files, "model")?;
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            vits: OfflineTtsVitsModelConfig {
                model: Some(model.clone()),
                tokens: Some(path(files, "tokens")?),
                data_dir: Some(path(files, "espeak-ng-data")?),
                ..OfflineTtsVitsModelConfig::default()
            },
            provider: Some(provider.to_owned()),
            ..OfflineTtsModelConfig::default()
        },
        ..OfflineTtsConfig::default()
    };
    Synthesizer::new(config, Some(&model), |_| None)
}
