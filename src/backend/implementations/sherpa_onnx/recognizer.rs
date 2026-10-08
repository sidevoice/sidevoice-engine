//! Every speech-to-text model through sherpa-onnx's offline recognizer, alike: one whole turn at a time, with the
//! recognizer its config makes. Nothing here knows one model from another.
//!
//! The language a call names is not passed on yet: sherpa-onnx reads Whisper's from the recognizer's config, which a
//! live recognizer cannot change, and how a call's arguments reach a model's config is to be data
//! (sidevoice-engine#46). Until then each model does what its config says, which for Whisper is to detect the
//! language.

use async_trait::async_trait;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig};

use crate::backend::loaded_model::SttModel;
use crate::backend::BackendModel;
use crate::{Error, Result};

/// The sample rate [`SttModel::transcribe`] takes (the engine resamples to it).
const SAMPLE_RATE: i32 = 16_000;

/// A speech-to-text model in memory.
pub(super) struct Recognizer(OfflineRecognizer);

impl Recognizer {
    /// Creates the recognizer from `config`, the build's files in it (`config.rs`). Fails with `model-load-failed`.
    pub(super) fn load(config: &OfflineRecognizerConfig) -> Result<Self> {
        OfflineRecognizer::create(config)
            .map(Self)
            .ok_or(Error::new("model-load-failed"))
    }
}

impl BackendModel for Recognizer {
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl SttModel for Recognizer {
    /// Decodes on the calling thread: the engine decides where to run it. `_language` is not passed on yet (see the
    /// module's docs).
    async fn transcribe(&mut self, pcm: &[f32], _language: Option<&str>) -> Result<String> {
        let stream = self.0.create_stream();
        stream.accept_waveform(SAMPLE_RATE, pcm);
        self.0.decode(&stream);
        let result = stream
            .get_result()
            .ok_or(Error::new("transcription-failed"))?;
        Ok(result.text.trim().to_owned())
    }
}
