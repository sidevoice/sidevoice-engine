//! sherpa-onnx's offline recognizer, which every speech-to-text model here runs on: made once from a model's config,
//! it decodes one whole turn at a time. Whisper and the transducers differ only in the config they make it from.

use std::ffi::CStr;
use std::sync::Arc;

use async_trait::async_trait;

use super::c_api::{OfflineRecognizer, OfflineRecognizerConfig};
use super::library::Api;
use super::num_threads;
use crate::backend::loaded_model::SttModel;
use crate::backend::LoadedModel;
use crate::{Error, Result};

/// The sample rate [`SttModel::transcribe`] takes, and what every recognizer here is configured for.
pub(super) const SAMPLE_RATE: i32 = 16_000;

/// A recognizer in memory.
pub(super) struct Recognizer {
    api: Arc<Api>,
    recognizer: *const OfflineRecognizer,
}

// SAFETY: the recognizer is a heap object of the library's, not tied to the thread that made it; it is only used
// through `&mut self` to decode, so never from two threads at once.
unsafe impl Send for Recognizer {}

impl Recognizer {
    /// Creates it from `config`, after setting what every model here shares: 16 kHz input, the threads, `provider`
    /// (which `config` must point to) and greedy search. Fails with `model-load-failed`.
    pub(super) fn new(api: Arc<Api>, config: &mut OfflineRecognizerConfig) -> Result<Self> {
        config.feat_config.sample_rate = SAMPLE_RATE;
        config.feat_config.feature_dim = 80;
        config.model_config.num_threads = num_threads();
        config.decoding_method = c"greedy_search".as_ptr();
        // SAFETY: `config` has the header's layout (c_api.rs) and every pointer in it is to a live string; the
        // library copies what it needs.
        let recognizer = unsafe { (api.create_offline_recognizer)(config) };
        if recognizer.is_null() {
            return Err(Error::new("model-load-failed"));
        }
        Ok(Self { api, recognizer })
    }

    /// Makes it decode with `config` from now on, which it copies.
    pub(super) fn set_config(&mut self, config: &OfflineRecognizerConfig) {
        // SAFETY: as in `new`; the recognizer is live until dropped.
        unsafe { (self.api.offline_recognizer_set_config)(self.recognizer, config) };
    }

    /// The text spoken in `pcm` (mono, at [`SAMPLE_RATE`]), on the calling thread: the engine decides where to run
    /// it. Fails with `transcription-failed`.
    pub(super) fn decode(&mut self, pcm: &[f32]) -> Result<String> {
        let failed = Error::new("transcription-failed");
        let samples = i32::try_from(pcm.len()).map_err(|_| failed)?;
        let api = &self.api;
        // SAFETY: the stream and result are made and destroyed here, each once, and `pcm` outlives the calls that
        // read it; the result's text is copied before the result is destroyed.
        unsafe {
            let stream = (api.create_offline_stream)(self.recognizer);
            if stream.is_null() {
                return Err(failed);
            }
            (api.accept_waveform_offline)(stream, SAMPLE_RATE, pcm.as_ptr(), samples);
            (api.decode_offline_stream)(self.recognizer, stream);
            let result = (api.get_offline_stream_result)(stream);
            let text = if result.is_null() || (*result).text.is_null() {
                None
            } else {
                Some(
                    CStr::from_ptr((*result).text)
                        .to_string_lossy()
                        .trim()
                        .to_owned(),
                )
            };
            if !result.is_null() {
                (api.destroy_offline_recognizer_result)(result);
            }
            (api.destroy_offline_stream)(stream);
            text.ok_or(failed)
        }
    }
}

impl Drop for Recognizer {
    fn drop(&mut self) {
        // SAFETY: made by `create_offline_recognizer` and destroyed only here.
        unsafe { (self.api.destroy_offline_recognizer)(self.recognizer) };
    }
}

/// A recognizer whose model takes no language: it hears whichever of its languages is spoken (the transducers).
impl LoadedModel for Recognizer {
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
    /// `language` is not asked of the model: it is not one that takes it.
    async fn transcribe(&mut self, pcm: &[f32], _language: Option<&str>) -> Result<String> {
        self.decode(pcm)
    }
}
