//! A loaded model, as the engine holds it after a backend's `load`: it transcribes, speaks, or both. The backend
//! creates it and never touches it again; the engine keeps it until it is unloaded.

use async_trait::async_trait;

use crate::Result;

/// A model in memory. Transcribing and speaking are capabilities of the loaded model, not of the backend: one
/// backend can load models of both kinds.
pub trait LoadedModel: Send {
    fn as_transcriber(&mut self) -> Option<&mut dyn Transcriber> {
        None
    }
    fn as_synthesizer(&mut self) -> Option<&mut dyn Synthesizer> {
        None
    }
    fn memory_mb(&self) -> Option<u32>;
}

/// Speech to text, one whole turn at a time.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Transcriber {
    /// `pcm`: mono 16 kHz samples. `language`: a BCP 47 tag, or `None` to detect it.
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String>;
}

/// Text to speech.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Synthesizer {
    fn voices(&self) -> Vec<String>;
    async fn speak(&mut self, text: &str, voice: &str, speed: f32) -> Result<Vec<f32>>;
}
