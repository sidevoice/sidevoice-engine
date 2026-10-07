//! Whisper through sherpa-onnx's offline recognizer: one whole turn at a time.

use std::ffi::{CStr, CString};
use std::sync::Arc;

use async_trait::async_trait;

use super::c_api::{OfflineRecognizer, OfflineRecognizerConfig};
use super::library::Api;
use super::{c_string, num_threads, path};
use crate::backend::loaded_model::SttModel;
use crate::backend::LoadedModel;
use crate::install::Installed;
use crate::{Error, Result};

/// The sample rate [`SttModel::transcribe`] takes, and Whisper's.
const SAMPLE_RATE: i32 = 16_000;

/// A Whisper model in memory: the recognizer, the config it was made from (to change its language), and the strings
/// that config points to.
pub(super) struct Whisper {
    api: Arc<Api>,
    recognizer: *const OfflineRecognizer,
    config: OfflineRecognizerConfig,
    /// The language `config` names now: `""` to detect it.
    language: CString,
    _strings: [CString; 5],
}

// SAFETY: the recognizer is a heap object of the library's, not tied to the thread that made it; it is only used
// through `&mut self`, so never from two threads at once.
unsafe impl Send for Whisper {}

impl Whisper {
    /// Creates the recognizer from the `encoder`, `decoder` and `tokens` in `files`, on `provider`.
    pub(super) fn load(api: Arc<Api>, files: &Installed, provider: &str) -> Result<Self> {
        let strings = [
            c_string(path(files, "encoder")?)?,
            c_string(path(files, "decoder")?)?,
            c_string(path(files, "tokens")?)?,
            c_string(provider)?,
            c_string("transcribe")?,
        ];
        let [encoder, decoder, tokens, provider, task] = &strings;
        let language = CString::default();
        let mut config = OfflineRecognizerConfig::default();
        config.feat_config.sample_rate = SAMPLE_RATE;
        config.feat_config.feature_dim = 80;
        config.model_config.whisper.encoder = encoder.as_ptr();
        config.model_config.whisper.decoder = decoder.as_ptr();
        config.model_config.whisper.language = language.as_ptr();
        config.model_config.whisper.task = task.as_ptr();
        config.model_config.tokens = tokens.as_ptr();
        config.model_config.provider = provider.as_ptr();
        config.model_config.num_threads = num_threads();
        config.decoding_method = c"greedy_search".as_ptr();
        // SAFETY: `config` has the header's layout (c_api.rs) and every pointer in it is to a live string.
        let recognizer = unsafe { (api.create_offline_recognizer)(&config) };
        if recognizer.is_null() {
            return Err(Error::new("model-load-failed"));
        }
        Ok(Self {
            api,
            recognizer,
            config,
            language,
            _strings: strings,
        })
    }

    /// Points the recognizer at `language` (a BCP 47 tag, of which Whisper reads the primary language), or at
    /// detecting it, unless it already is.
    fn set_language(&mut self, language: Option<&str>) -> Result<()> {
        let code = language
            .and_then(|tag| tag.split(['-', '_']).next())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if self.language.as_bytes() == code.as_bytes() {
            return Ok(());
        }
        self.language = c_string(&code)?;
        self.config.model_config.whisper.language = self.language.as_ptr();
        // SAFETY: as in `load`; the recognizer copies what it needs from `config`.
        unsafe { (self.api.offline_recognizer_set_config)(self.recognizer, &self.config) };
        Ok(())
    }
}

impl Drop for Whisper {
    fn drop(&mut self) {
        // SAFETY: made by `create_offline_recognizer` and destroyed only here.
        unsafe { (self.api.destroy_offline_recognizer)(self.recognizer) };
    }
}

impl LoadedModel for Whisper {
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl SttModel for Whisper {
    /// Decodes on the calling thread: the engine decides where to run it.
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String> {
        let failed = Error::new("transcription-failed");
        self.set_language(language)?;
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
