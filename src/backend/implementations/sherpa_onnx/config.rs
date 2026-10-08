//! A build's files into sherpa-onnx's config: each file's key in the catalogue is the path of the config field that
//! receives it, relative to the config root of its capability: `OfflineRecognizerConfig.model_config` for speech to
//! text (`whisper.encoder`, `tokens`, ...), `OfflineTtsConfig.model` for text to speech (`kokoro.model`, ...). One
//! match per root names every path the backend takes, so a field sherpa-onnx renames breaks the build, and a key it
//! does not know fails the catalogue's tests (`tests.rs`), not a load. Everything else is sherpa-onnx's default; the
//! engine adds the provider and the threads.

use sherpa_onnx::{
    OfflineModelConfig, OfflineRecognizerConfig, OfflineTtsConfig, OfflineTtsModelConfig,
};

use super::{num_threads, text};
use crate::install::Installed;
use crate::{Error, Result};

/// The field of `OfflineRecognizerConfig.model_config` that `key` names, if the backend takes it.
pub(super) fn stt_field<'a>(
    config: &'a mut OfflineModelConfig,
    key: &str,
) -> Option<&'a mut Option<String>> {
    Some(match key {
        "tokens" => &mut config.tokens,
        "whisper.encoder" => &mut config.whisper.encoder,
        "whisper.decoder" => &mut config.whisper.decoder,
        "transducer.encoder" => &mut config.transducer.encoder,
        "transducer.decoder" => &mut config.transducer.decoder,
        "transducer.joiner" => &mut config.transducer.joiner,
        _ => return None,
    })
}

/// The field of `OfflineTtsConfig.model` that `key` names, if the backend takes it.
pub(super) fn tts_field<'a>(
    config: &'a mut OfflineTtsModelConfig,
    key: &str,
) -> Option<&'a mut Option<String>> {
    Some(match key {
        "kokoro.model" => &mut config.kokoro.model,
        "kokoro.voices" => &mut config.kokoro.voices,
        "kokoro.tokens" => &mut config.kokoro.tokens,
        "kokoro.data_dir" => &mut config.kokoro.data_dir,
        "kokoro.lexicon" => &mut config.kokoro.lexicon,
        "vits.model" => &mut config.vits.model,
        "vits.tokens" => &mut config.vits.tokens,
        "vits.data_dir" => &mut config.vits.data_dir,
        "supertonic.duration_predictor" => &mut config.supertonic.duration_predictor,
        "supertonic.text_encoder" => &mut config.supertonic.text_encoder,
        "supertonic.vector_estimator" => &mut config.supertonic.vector_estimator,
        "supertonic.vocoder" => &mut config.supertonic.vocoder,
        "supertonic.tts_json" => &mut config.supertonic.tts_json,
        "supertonic.unicode_indexer" => &mut config.supertonic.unicode_indexer,
        "supertonic.voice_style" => &mut config.supertonic.voice_style,
        _ => return None,
    })
}

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
