//! Kokoro through sherpa-onnx's offline TTS: a whole utterance at a time.

use std::ffi::CString;
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use async_trait::async_trait;

use super::c_api::{GenerationConfig, OfflineTts, OfflineTtsConfig};
use super::library::Api;
use super::{c_string, model_metadata, num_threads, path};
use crate::backend::loaded_model::TtsModel;
use crate::backend::LoadedModel;
use crate::install::Installed;
use crate::{Error, Result};

/// A Kokoro model in memory: the TTS object, its sample rate and its voices, by speaker id.
pub(super) struct Kokoro {
    api: Arc<Api>,
    tts: *const OfflineTts,
    sample_rate: u32,
    voices: Vec<String>,
}

// SAFETY: the TTS object is a heap object of the library's, not tied to the thread that made it; it is only used
// through `&mut self` for speaking, so never from two threads at once.
unsafe impl Send for Kokoro {}

impl Kokoro {
    /// Creates the TTS from the `model`, `voices`, `tokens` and `espeak-ng-data` (a directory) in `files`, on
    /// `provider`. The voices are named by the model's `speaker_names` metadata; a model without it has its speaker
    /// ids, `"0"`, `"1"`, ..., as names.
    pub(super) fn load(api: Arc<Api>, files: &Installed, provider: &str) -> Result<Self> {
        let model_path = path(files, "model")?;
        let model = c_string(model_path)?;
        let voices = c_string(path(files, "voices")?)?;
        let tokens = c_string(path(files, "tokens")?)?;
        let data_dir = c_string(path(files, "espeak-ng-data")?)?;
        let provider = c_string(provider)?;
        let mut config = OfflineTtsConfig::default();
        config.model.kokoro.model = model.as_ptr();
        config.model.kokoro.voices = voices.as_ptr();
        config.model.kokoro.tokens = tokens.as_ptr();
        config.model.kokoro.data_dir = data_dir.as_ptr();
        config.model.kokoro.length_scale = 1.0;
        config.model.num_threads = num_threads();
        config.model.provider = provider.as_ptr();
        // SAFETY: `config` has the header's layout (c_api.rs) and every pointer in it is to a live string; the
        // library copies what it needs.
        let tts = unsafe { (api.create_offline_tts)(&config) };
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
        let mut kokoro = Self {
            api,
            tts,
            sample_rate: 0,
            voices: Vec::new(),
        };
        kokoro.sample_rate = u32::try_from(rate).map_err(|_| Error::new("model-load-failed"))?;
        let speakers = usize::try_from(speakers).unwrap_or_default();
        let named = model_metadata::read(Path::new(model_path), "speaker_names")
            .map(|names| names.split(',').map(str::to_owned).collect::<Vec<_>>())
            .filter(|names| names.len() == speakers);
        kokoro.voices = named.unwrap_or_else(|| (0..speakers).map(|id| id.to_string()).collect());
        Ok(kokoro)
    }
}

impl Drop for Kokoro {
    fn drop(&mut self) {
        // SAFETY: made by `create_offline_tts` and destroyed only here.
        unsafe { (self.api.destroy_offline_tts)(self.tts) };
    }
}

impl LoadedModel for Kokoro {
    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl TtsModel for Kokoro {
    fn voices(&self) -> Vec<String> {
        self.voices.clone()
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Synthesises on the calling thread: the engine decides where to run it. `voice` is one of
    /// [`TtsModel::voices`], or fails with `unknown-voice`.
    async fn speak(&mut self, text: &str, voice: &str, speed: f32) -> Result<Vec<f32>> {
        let failed = Error::new("speech-failed");
        let sid = self
            .voices
            .iter()
            .position(|name| name == voice)
            .ok_or(Error::new("unknown-voice"))?;
        let text: CString = c_string(text)?;
        let mut config = GenerationConfig::default();
        config.sid = i32::try_from(sid).map_err(|_| failed)?;
        config.speed = speed;
        let api = &self.api;
        // SAFETY: `text` and `config` outlive the call, which takes no callback; the audio is copied before it is
        // destroyed, once.
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
