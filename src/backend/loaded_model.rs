//! A loaded model, as the engine holds it after a backend's `load`: it transcribes, speaks, or detects speech. The
//! backend creates it and never touches it again; the engine keeps it until it is unloaded.

use std::ops::Range;

use async_trait::async_trait;

use crate::catalog::Voice;
use crate::engine::VadOptions;
use crate::maybe_send::MaybeSend;
use crate::Result;

/// A model in memory. Speech to text, text to speech and voice activity detection (as the catalogue's `Capability`
/// names them) are what the loaded model can do, not the backend: one backend can load models of every kind.
pub(crate) trait BackendModel: MaybeSend {
    /// The model as speech to text, if it is one.
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        None
    }
    /// The model as text to speech, if it is one.
    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        None
    }
    /// The model as a voice activity detector, if it is one.
    fn as_vad(&mut self) -> Option<&mut dyn VadModel> {
        None
    }
    /// The model as an end-of-turn classifier, if it is one.
    fn as_end_of_turn(&mut self) -> Option<&mut dyn EndOfTurnModel> {
        None
    }
    /// The memory it takes, when the backend can tell.
    #[allow(dead_code, reason = "the engine does not read it yet")]
    fn memory_mb(&self) -> Option<u32>;
}

/// A speech-to-text model: one whole turn at a time.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait SttModel: MaybeSend {
    /// `pcm`: mono 16 kHz samples. `language`: a BCP 47 tag, or `None` to detect it. sherpa-onnx passes it on only
    /// where the build's `call_params` map it into the config (sidevoice-engine#46).
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String>;
}

/// A text-to-speech model. It speaks a whole utterance at a time; streaming is not part of the contract yet (see
/// *Speech out* in `backend.rs`).
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait TtsModel: MaybeSend {
    /// The voices it speaks with.
    fn voices(&self) -> Vec<String>;
    /// The sample rate of what [`TtsModel::speak`] returns, in Hz.
    fn sample_rate(&self) -> u32;
    /// `text` spoken with `voice` at `speed` (1.0 is normal), as mono samples at [`TtsModel::sample_rate`].
    /// `voice` is one of [`TtsModel::voices`] by its `id`, with the languages the catalogue declares for it.
    /// `language`: `text`'s, as a BCP 47 tag, for a model that speaks several; `None` leaves it to the model, and a
    /// model of one language ignores it.
    async fn speak(
        &mut self,
        text: &str,
        voice: &Voice,
        language: Option<&str>,
        speed: f32,
    ) -> Result<Vec<f32>>;
}

/// A voice activity detector: it makes streams, each with a state of its own, so several can run at once.
pub(crate) trait VadModel: MaybeSend {
    /// The rate its streams take, in Hz.
    fn sample_rate(&self) -> u32;
    /// How many samples [`VadStreamModel::window`] takes at a time.
    fn window(&self) -> usize;
    /// A new stream, starting at sample 0 with no speech, detecting with `options` (already checked by the engine).
    fn stream(&mut self, options: &VadOptions) -> Result<Box<dyn VadStreamModel>>;
}

/// One stream of audio through a voice activity detector. Positions are in samples at the model's rate, counted from
/// the stream's start or its last reset.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait VadStreamModel: MaybeSend {
    /// Takes the next window, exactly [`VadModel::window`] samples, and says what the detector knows after it.
    async fn window(&mut self, pcm: &[f32]) -> Result<Window>;
    /// Ends the speech in progress, if any, at the last sample taken, and returns it; then starts over from sample 0.
    fn finish(&mut self) -> Option<Range<u64>>;
    /// Starts over from sample 0, with no speech and nothing remembered.
    fn reset(&mut self);
}

/// What a voice activity detector knows after one window.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Window {
    /// Whether it is inside speech: confirmed, and not yet over.
    pub(crate) speech: bool,
    /// The probability of speech the model gave this window, when the backend can tell it.
    pub(crate) probability: Option<f32>,
    /// The speech that ended with this window, from its first sample to the one after its last.
    pub(crate) ended: Vec<Range<u64>>,
}

/// An end-of-turn classifier: how likely it is that a speaker who paused has finished.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait EndOfTurnModel: MaybeSend {
    /// How many seconds of the end of a turn it hears: earlier audio does not count.
    fn seconds(&self) -> u32;
    /// The probability, from 0 to 1, that the turn in `pcm` (mono 16 kHz, from its start to now) is complete.
    async fn probability(&mut self, pcm: &[f32]) -> Result<f32>;
}
