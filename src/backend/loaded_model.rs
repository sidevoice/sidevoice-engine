//! A loaded model, as the engine holds it after a backend's `load`: it transcribes, speaks, or both. The backend
//! creates it and never touches it again; the engine keeps it until it is unloaded.

use async_trait::async_trait;

use crate::maybe_send::MaybeSend;
use crate::Result;

/// A model in memory. Speech to text and text to speech (as the catalogue's `Task` names them) are capabilities of the
/// loaded model, not of the backend: one backend can load models of both kinds.
#[allow(
    dead_code,
    reason = "the engine does not use a loaded model yet: only the tests do"
)]
pub(crate) trait LoadedModel: MaybeSend {
    /// The model as speech to text, if it is one.
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        None
    }
    /// The model as text to speech, if it is one.
    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        None
    }
    /// The memory it takes, when the backend can tell.
    fn memory_mb(&self) -> Option<u32>;
}

/// A speech-to-text model: one whole turn at a time.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
#[allow(
    dead_code,
    reason = "the engine does not use a loaded model yet: only the tests do"
)]
pub(crate) trait SttModel {
    /// `pcm`: mono 16 kHz samples. `language`: a BCP 47 tag, or `None` to detect it.
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String>;
}

/// A text-to-speech model. It speaks a whole utterance at a time; streaming is not part of the contract yet (see
/// *Speech out* in `backend.rs`).
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
#[allow(
    dead_code,
    reason = "the engine does not use a loaded model yet: only the tests do"
)]
pub(crate) trait TtsModel {
    /// The voices it speaks with.
    fn voices(&self) -> Vec<String>;
    /// The sample rate of what [`TtsModel::speak`] returns, in Hz.
    fn sample_rate(&self) -> u32;
    /// `text` spoken with `voice` at `speed` (1.0 is normal), as mono samples at [`TtsModel::sample_rate`].
    async fn speak(&mut self, text: &str, voice: &str, speed: f32) -> Result<Vec<f32>>;
}
