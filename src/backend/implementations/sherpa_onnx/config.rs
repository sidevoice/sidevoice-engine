//! A build's files into sherpa-onnx's config: each file's key in the catalogue is the path of the config field that
//! receives it, relative to the config root of its capability: `OfflineRecognizerConfig.model_config` for speech to
//! text (`whisper.encoder`, `tokens`, ...), `OfflineTtsConfig.model` for text to speech (`kokoro.model`, ...). One
//! match per root names every path that takes a file (`config/fields.rs`, generated from the pinned crate's source by
//! `cargo xtask sherpa-libs --pin`), so a field sherpa-onnx renames breaks the build, and a key it does not have
//! fails the catalogue's tests (`tests.rs`), not a load. Everything else is sherpa-onnx's default; the engine adds
//! the provider and the threads.

use sherpa_onnx::{OfflineRecognizerConfig, OfflineTtsConfig};

use super::{num_threads, text};
use crate::install::Installed;
use crate::{Error, Result};

mod fields;

pub(super) use fields::{stt_field, tts_field};

/// The recognizer's config: every file of `files` in the field its key names, on `provider`; `unsupported-model` for a
/// key no field has.
pub(super) fn recognizer(files: &Installed, provider: &str) -> Result<OfflineRecognizerConfig> {
    let mut config = OfflineRecognizerConfig::default();
    for (key, path) in &files.files {
        let field =
            stt_field(&mut config.model_config, key).ok_or(Error::new("unsupported-model"))?;
        *field = Some(text(path)?);
    }
    config.model_config.provider = Some(provider.to_owned());
    config.model_config.num_threads = num_threads();
    Ok(config)
}

/// The TTS's config: every file of `files` in the field its key names, on `provider`; `unsupported-model` for a key no
/// field has.
pub(super) fn tts(files: &Installed, provider: &str) -> Result<OfflineTtsConfig> {
    let mut config = OfflineTtsConfig::default();
    for (key, path) in &files.files {
        let field = tts_field(&mut config.model, key).ok_or(Error::new("unsupported-model"))?;
        *field = Some(text(path)?);
    }
    config.model.provider = Some(provider.to_owned());
    config.model.num_threads = num_threads();
    Ok(config)
}
