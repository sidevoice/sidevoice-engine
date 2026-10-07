//! sherpa-onnx's offline TTS, which every text-to-speech model here runs on: made once from a model's config, it
//! speaks a whole utterance at a time. Kokoro, VITS (Piper) and Supertonic differ in the config they make it from, and
//! in how a language reaches them.

use std::ffi::CString;
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use async_trait::async_trait;

use super::c_api::{GenerationConfig, OfflineTts, OfflineTtsConfig};
use super::library::Api;
use super::{c_string, model_metadata, num_threads};
use crate::backend::loaded_model::TtsModel;
use crate::backend::LoadedModel;
use crate::{Error, Result};

/// What a model is told of the language it speaks, from a BCP 47 tag: the `lang` of the generation's extra options,
/// or `None` to tell it nothing.
pub(super) type Language = fn(&str) -> Option<String>;

/// A TTS model in memory: the TTS object, its sample rate, its voices by speaker id, and how it is told a language.
pub(super) struct Synthesizer {
    api: Arc<Api>,
    tts: *const OfflineTts,
    sample_rate: u32,
    voices: Vec<String>,
    language: Language,
}

// SAFETY: the TTS object is a heap object of the library's, not tied to the thread that made it; it is only used
// through `&mut self` for speaking, so never from two threads at once.
unsafe impl Send for Synthesizer {}

impl Synthesizer {
    /// Creates it from `config`, after setting the threads and `provider` (which `config` must point to). Its voices
    /// are named by the `speaker_names` metadata of the ONNX model at `named_by`, if any, when it has as many as the
    /// model has speakers; otherwise by speaker id, `"0"`, `"1"`, .... Fails with `model-load-failed`.
    pub(super) fn new(
        api: Arc<Api>,
        config: &mut OfflineTtsConfig,
        named_by: Option<&Path>,
        language: Language,
    ) -> Result<Self> {
        config.model.num_threads = num_threads();
        // SAFETY: `config` has the header's layout (c_api.rs) and every pointer in it is to a live string; the
        // library copies what it needs.
        let tts = unsafe { (api.create_offline_tts)(config) };
        if tts.is_null() {
            return Err(Error::new("model-load-failed"));
        }
        // SAFETY: `tts` was just made, and is valid.
        let (rate, speakers) = unsafe {
            (
                (api.offline_tts_sample_rate)(tts),
                (api.offline_tts_num_speakers)(tts),
            )
        };
        // Built before the checks below, so that a failure destroys `tts`.
        let mut synthesizer = Self {
            api,
            tts,
            sample_rate: 0,
            voices: Vec::new(),
            language,
        };
        synthesizer.sample_rate =
            u32::try_from(rate).map_err(|_| Error::new("model-load-failed"))?;
        let speakers = usize::try_from(speakers).unwrap_or_default();
        let named = named_by
            .and_then(|model| model_metadata::read(model, "speaker_names"))
            .map(|names| names.split(',').map(str::to_owned).collect::<Vec<_>>())
            .filter(|names| names.len() == speakers);
        synthesizer.voices =
            named.unwrap_or_else(|| (0..speakers).map(|id| id.to_string()).collect());
        Ok(synthesizer)
    }
}

impl Drop for Synthesizer {
    fn drop(&mut self) {
        // SAFETY: made by `create_offline_tts` and destroyed only here.
        unsafe { (self.api.destroy_offline_tts)(self.tts) };
    }
}

impl LoadedModel for Synthesizer {
    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl TtsModel for Synthesizer {
    fn voices(&self) -> Vec<String> {
        self.voices.clone()
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Synthesises on the calling thread: the engine decides where to run it. `voice` is one of
    /// [`TtsModel::voices`], or fails with `unknown-voice`.
    async fn speak(
        &mut self,
        text: &str,
        voice: &str,
        language: Option<&str>,
        speed: f32,
    ) -> Result<Vec<f32>> {
        let failed = Error::new("speech-failed");
        let sid = self
            .voices
            .iter()
            .position(|name| name == voice)
            .ok_or(Error::new("unknown-voice"))?;
        let text: CString = c_string(text)?;
        let extra = language
            .and_then(self.language)
            .map(|lang| c_string(&serde_json::json!({ "lang": lang }).to_string()))
            .transpose()?;
        let config = GenerationConfig {
            sid: i32::try_from(sid).map_err(|_| failed)?,
            speed,
            extra: extra.as_ref().map_or(ptr::null(), |extra| extra.as_ptr()),
            ..GenerationConfig::default()
        };
        let api = &self.api;
        // SAFETY: `text`, `extra` and `config` outlive the call, which takes no callback; the audio is copied before
        // it is destroyed, once.
        unsafe {
            let audio = (api.offline_tts_generate_with_config)(
                self.tts,
                text.as_ptr(),
                &config,
                None,
                ptr::null_mut(),
            );
            if audio.is_null() {
                return Err(failed);
            }
            let samples = match usize::try_from((*audio).n) {
                Ok(0) => Ok(Vec::new()),
                Ok(n) if !(*audio).samples.is_null() => {
                    Ok(std::slice::from_raw_parts((*audio).samples, n).to_vec())
                }
                _ => Err(failed),
            };
            (api.destroy_offline_tts_generated_audio)(audio);
            samples
        }
    }
}
