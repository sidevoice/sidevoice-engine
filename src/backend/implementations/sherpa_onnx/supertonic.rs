//! Supertonic through sherpa-onnx's offline TTS: multilingual without espeak-ng, told each utterance's language by its
//! primary subtag (`es`, `en`, ...; English when it is told none). Its voices are the styles in its voice file, by
//! speaker id.

use std::sync::Arc;

use super::c_api::OfflineTtsConfig;
use super::library::Api;
use super::synthesizer::Synthesizer;
use super::{c_string, path, primary_language};
use crate::install::Installed;
use crate::Result;

/// Creates the TTS from the `duration_predictor`, `text_encoder`, `vector_estimator`, `vocoder`, `tts_json`,
/// `unicode_indexer` and `voice_style` in `files`, on `provider`.
pub(super) fn load(api: Arc<Api>, files: &Installed, provider: &str) -> Result<Synthesizer> {
    let duration_predictor = c_string(path(files, "duration_predictor")?)?;
    let text_encoder = c_string(path(files, "text_encoder")?)?;
    let vector_estimator = c_string(path(files, "vector_estimator")?)?;
    let vocoder = c_string(path(files, "vocoder")?)?;
    let tts_json = c_string(path(files, "tts_json")?)?;
    let unicode_indexer = c_string(path(files, "unicode_indexer")?)?;
    let voice_style = c_string(path(files, "voice_style")?)?;
    let provider = c_string(provider)?;
    let mut config = OfflineTtsConfig::default();
    let supertonic = &mut config.model.supertonic;
    supertonic.duration_predictor = duration_predictor.as_ptr();
    supertonic.text_encoder = text_encoder.as_ptr();
    supertonic.vector_estimator = vector_estimator.as_ptr();
    supertonic.vocoder = vocoder.as_ptr();
    supertonic.tts_json = tts_json.as_ptr();
    supertonic.unicode_indexer = unicode_indexer.as_ptr();
    supertonic.voice_style = voice_style.as_ptr();
    config.model.provider = provider.as_ptr();
    Synthesizer::new(api, &mut config, None, |tag| Some(primary_language(tag)))
}
