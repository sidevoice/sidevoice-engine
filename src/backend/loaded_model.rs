//! A loaded model, as the engine holds it after a backend's `load`: it transcribes, speaks, or both. The backend
//! creates it and never touches it again; the engine keeps it until it is unloaded.

use async_trait::async_trait;

use crate::maybe_send::MaybeSend;
use crate::Result;

/// A model in memory. Speech to text and text to speech (as the catalogue's `Capability` names them) are what the
/// loaded model can do, not the backend: one backend can load models of both kinds.
pub(crate) trait BackendModel: MaybeSend {
    /// The model as speech to text, if it is one.
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        None
    }
    /// The model as text to speech, if it is one.
    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        None
    }
    /// The memory it takes, when the backend can tell.
    #[allow(dead_code, reason = "the engine does not read it yet")]
    fn memory_mb(&self) -> Option<u32>;
}

/// A speech-to-text model: one whole turn at a time.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait SttModel {
    /// `pcm`: mono 16 kHz samples. `language`: a BCP 47 tag, or `None` to detect it. sherpa-onnx passes it on only
    /// where the build's `call_params` map it into the config (sidevoice-engine#46).
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String>;
}

/// A text-to-speech model. It speaks a whole utterance at a time; streaming is not part of the contract yet (see
/// *Speech out* in `backend.rs`).
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait TtsModel {
    /// The voices it speaks with.
    fn voices(&self) -> Vec<String>;
    /// The sample rate of what [`TtsModel::speak`] returns, in Hz.
    fn sample_rate(&self) -> u32;
    /// `text` spoken with `voice` at `speed` (1.0 is normal), as mono samples at [`TtsModel::sample_rate`].
    /// `language`: `text`'s, as a BCP 47 tag, for a model that speaks several; `None` leaves it to the model, and a
    /// model of one language ignores it.
    async fn speak(
        &mut self,
        text: &str,
        voice: &str,
        language: Option<&str>,
        speed: f32,
    ) -> Result<Vec<f32>>;
}
