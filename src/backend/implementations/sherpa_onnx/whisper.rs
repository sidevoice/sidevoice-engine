//! Whisper through sherpa-onnx's offline recognizer: one whole turn at a time, in the language asked for or the one it
//! detects.

use std::ffi::CString;
use std::sync::Arc;

use async_trait::async_trait;

use super::c_api::OfflineRecognizerConfig;
use super::library::Api;
use super::recognizer::Recognizer;
use super::{c_string, path, primary_language};
use crate::backend::loaded_model::SttModel;
use crate::backend::LoadedModel;
use crate::install::Installed;
use crate::Result;

/// A Whisper model in memory: the recognizer, the config it was made from (to change its language), and the strings
/// that config points to.
pub(super) struct Whisper {
    recognizer: Recognizer,
    config: OfflineRecognizerConfig,
    /// The language `config` names now: `""` to detect it.
    language: CString,
    _strings: [CString; 5],
}

// SAFETY: the config's pointers are to the strings this owns, which never move (a `CString` keeps its bytes on the
// heap), and it is only read through `&mut self`.
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
        config.model_config.whisper.encoder = encoder.as_ptr();
        config.model_config.whisper.decoder = decoder.as_ptr();
        config.model_config.whisper.language = language.as_ptr();
        config.model_config.whisper.task = task.as_ptr();
        config.model_config.tokens = tokens.as_ptr();
        config.model_config.provider = provider.as_ptr();
        let recognizer = Recognizer::new(api, &mut config)?;
        Ok(Self {
            recognizer,
            config,
            language,
            _strings: strings,
        })
    }

    /// Points the recognizer at `language` (a BCP 47 tag, of which Whisper reads the primary language), or at
    /// detecting it, unless it already is.
    fn set_language(&mut self, language: Option<&str>) -> Result<()> {
        let code = language.map(primary_language).unwrap_or_default();
        if self.language.as_bytes() == code.as_bytes() {
            return Ok(());
        }
        self.language = c_string(&code)?;
        self.config.model_config.whisper.language = self.language.as_ptr();
        self.recognizer.set_config(&self.config);
        Ok(())
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
        self.set_language(language)?;
        self.recognizer.decode(pcm)
    }
}
