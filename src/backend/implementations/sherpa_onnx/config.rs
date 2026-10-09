//! A build's files into sherpa-onnx's config: each file's key in the catalogue is the path of the config field that
//! receives it, relative to the config root of its capability: `OfflineRecognizerConfig.model_config` for speech to
//! text (`whisper.encoder`, `tokens`, ...), `OfflineTtsConfig.model` for text to speech (`kokoro.model`, ...),
//! `VadModelConfig` for voice activity (`silero_vad.model`). One match per root names every path that takes a file
//! (`config/fields.rs`, generated from the pinned crate's source by `cargo xtask sherpa-libs --pin`), so a field
//! sherpa-onnx renames breaks the build, and a key it does not have fails the catalogue's tests (`tests.rs`), not a
//! load. Everything else is sherpa-onnx's default; the engine adds the provider and the threads, and, per call, what
//! a build's `call_params` maps its arguments to (`set_call`).

use std::collections::BTreeMap;

use sherpa_onnx::{OfflineRecognizerConfig, OfflineTtsConfig, VadModelConfig};

use super::{num_threads, text};
use crate::install::Installed;
use crate::{Error, Result};

mod fields;

pub(super) use fields::{stt_field, stt_option, tts_field, vad_field};

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

/// Sets every config path `call_params` maps `argument` to (a build's `call_params`: `whisper.language`, ...) to `value`,
/// or back to sherpa-onnx's default with `None` (for Whisper's language, detecting it). An argument the build does not
/// map sets nothing; `unsupported-model` for a path no field a call may set has.
pub(super) fn set_call(
    config: &mut OfflineRecognizerConfig,
    call_params: &BTreeMap<String, Vec<String>>,
    argument: &str,
    value: Option<&str>,
) -> Result<()> {
    for path in call_params.get(argument).into_iter().flatten() {
        let field =
            stt_option(&mut config.model_config, path).ok_or(Error::new("unsupported-model"))?;
        *field = value.map(text).transpose()?;
    }
    Ok(())
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

/// The voice activity detector's config: every file of `files` in the field its key names, on `provider`, on one
/// thread (a window is 32 ms of audio); `unsupported-model` for a key no field has. The rate, the window and the
/// options are the detector's (`detector.rs`).
pub(super) fn vad(files: &Installed, provider: &str) -> Result<VadModelConfig> {
    let mut config = VadModelConfig::default();
    for (key, path) in &files.files {
        let field = vad_field(&mut config, key).ok_or(Error::new("unsupported-model"))?;
        *field = Some(text(path)?);
    }
    config.provider = Some(provider.to_owned());
    config.num_threads = 1;
    Ok(config)
}
