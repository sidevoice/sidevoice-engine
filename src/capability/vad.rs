//! A model as a voice activity detector: [`Vad`], which opens [`VadStream`]s, each fed audio as it comes and
//! answering with what it heard ([`VadOutput`]: a [`VadFrame`] per window and the [`VadEvent`]s).

use std::fmt;

use std::sync::Arc;

use super::Resident;
use crate::backend::VadStreamModel;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// How a [`VadStream`] decides what is speech. The defaults are sherpa-onnx's for Silero: a threshold of 0.5, 500 ms
/// of silence to end speech and 250 ms of speech to confirm it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadOptions {
    /// The probability above which a window is speech, from 0.01 to just under 1. Once speech is confirmed, it goes
    /// on until the probability falls 0.15 below this (to 0.01 at least).
    pub threshold: f32,
    /// How long speech must stay below the threshold to end, in milliseconds (at least 1).
    pub min_silence_ms: u32,
    /// How long speech must stay above the threshold to be confirmed, in milliseconds (at least 1).
    pub min_speech_ms: u32,
}

impl Default for VadOptions {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_silence_ms: 500,
            min_speech_ms: 250,
        }
    }
}

impl VadOptions {
    /// Whether these options are within their bounds (`invalid-vad-options` otherwise).
    fn check(&self) -> Result<()> {
        let threshold = (0.01..1.0).contains(&self.threshold);
        if threshold && self.min_silence_ms > 0 && self.min_speech_ms > 0 {
            Ok(())
        } else {
            Err(Error::new("invalid-vad-options"))
        }
    }
}

/// Where speech starts and ends in a stream. Positions are in samples at the model's rate
/// ([`VadStream::sample_rate`]), counted from the stream's start or its last [`VadStream::finish`] or
/// [`VadStream::reset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VadEvent {
    /// Speech was confirmed at the end of the window that ends at `at`. It began earlier, by up to `min_speech_ms`
    /// and two windows: the [`VadEvent::SpeechEnd`] that follows says where.
    SpeechStart {
        /// The end of the window that confirmed it.
        at: u64,
    },
    /// Speech ended: it runs from `start` to just before `end`. It is known `min_silence_ms` (and up to a window)
    /// after `end`.
    SpeechEnd {
        /// Its first sample.
        start: u64,
        /// The sample after its last.
        end: u64,
    },
}

/// What the detector knows after one window of the stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadFrame {
    /// The sample after the window's last.
    pub end: u64,
    /// Whether the stream is inside speech after it: confirmed and not yet over.
    pub speech: bool,
    /// The probability of speech the model gave the window, where its backend can tell it: transformers.js gives it;
    /// sherpa-onnx (1.13.8) segments speech inside its library and does not.
    pub probability: Option<f32>,
}

/// What [`VadStream::accept`] heard: a frame per whole window taken, and the events, in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VadOutput {
    /// One per window taken, in order.
    pub frames: Vec<VadFrame>,
    /// Speech that started or ended in those windows, in order.
    pub events: Vec<VadEvent>,
}

/// A model, as a voice activity detector.
#[derive(Debug, Clone, Copy)]
pub struct Vad<'a>(pub(super) &'a Arc<Resident>);

impl Vad<'_> {
    /// A new stream, from sample 0 and with no speech, deciding with `options`. Each stream has a state of its own, so
    /// several can run at once (a microphone and a playback, say); each keeps the model in memory while it lives.
    ///
    /// # Errors
    ///
    /// `invalid-vad-options` for options out of their bounds, and the backend's `model-load-failed`.
    pub async fn stream(&self, options: VadOptions) -> Result<VadStream> {
        options.check()?;
        let mut model = self.0.model.lock().await;
        let vad = model.as_vad().ok_or(Error::new("model-cannot-detect"))?;
        Ok(VadStream {
            sample_rate: vad.sample_rate(),
            window: vad.window(),
            stream: vad.stream(&options)?,
            pending: Vec::new(),
            position: 0,
            speaking: false,
            model: Arc::clone(self.0),
        })
    }
}

/// One stream of audio through a voice activity detector: feed it mono samples at [`VadStream::sample_rate`] as they
/// come, in pieces of any length; it runs the model on each whole window ([`VadStream::window`] samples) and keeps
/// the rest for the next piece. Dropping it ends it.
pub struct VadStream {
    // Dropped before `model`, which keeps its model in memory.
    stream: Box<dyn VadStreamModel>,
    sample_rate: u32,
    window: usize,
    /// Samples short of a whole window, kept for the next piece.
    pending: Vec<f32>,
    /// The samples taken in whole windows.
    position: u64,
    /// Whether a [`VadEvent::SpeechStart`] has no [`VadEvent::SpeechEnd`] yet.
    speaking: bool,
    model: Arc<Resident>,
}

impl fmt::Debug for VadStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VadStream")
            .field("model", &self.model)
            .field("sample_rate", &self.sample_rate)
            .field("window", &self.window)
            .field("position", &self.position)
            .field("speaking", &self.speaking)
            .finish_non_exhaustive()
    }
}

impl VadStream {
    /// The rate the stream takes, in Hz: the model's own (16 000 for Silero). The engine does not resample a stream.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// How many samples one window holds (512 for Silero at 16 kHz, 32 ms): one [`VadFrame`] each.
    #[must_use]
    pub fn window(&self) -> usize {
        self.window
    }

    /// Takes `pcm`, the next mono samples at [`VadStream::sample_rate`], and runs the model on every whole window it
    /// now has. It runs on the calling task: the app decides where.
    ///
    /// # Errors
    ///
    /// The backend's `detection-failed`. The stream may then have taken part of `pcm`: [`VadStream::reset`] it.
    pub async fn accept(&mut self, pcm: &[f32]) -> Result<VadOutput> {
        self.pending.extend_from_slice(pcm);
        let whole = self.pending.len() / self.window * self.window;
        let pending = std::mem::take(&mut self.pending);
        let mut output = VadOutput::default();
        let mut taken = 0;
        let mut result = Ok(());
        for samples in pending[..whole].chunks(self.window) {
            match self.stream.window(samples).await {
                Ok(window) => {
                    taken += samples.len();
                    self.position += samples.len() as u64;
                    for ended in window.ended {
                        if !self.speaking {
                            output
                                .events
                                .push(VadEvent::SpeechStart { at: ended.start });
                        }
                        output.events.push(VadEvent::SpeechEnd {
                            start: ended.start,
                            end: ended.end,
                        });
                        self.speaking = false;
                    }
                    if window.speech && !self.speaking {
                        output
                            .events
                            .push(VadEvent::SpeechStart { at: self.position });
                        self.speaking = true;
                    }
                    output.frames.push(VadFrame {
                        end: self.position,
                        speech: self.speaking,
                        probability: window.probability,
                    });
                }
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
        }
        self.pending = pending[taken..].to_vec();
        result.map(|()| output)
    }

    /// Ends the stream's audio: the speech in progress, if any, ends at the last sample of the last whole window taken
    /// (what is short of a window is dropped), and the stream starts over from sample 0.
    pub fn finish(&mut self) -> Option<VadEvent> {
        let ended = self.stream.finish();
        self.pending.clear();
        self.position = 0;
        self.speaking = false;
        ended.map(|ended| VadEvent::SpeechEnd {
            start: ended.start,
            end: ended.end,
        })
    }

    /// Starts over from sample 0, forgetting any speech in progress.
    pub fn reset(&mut self) {
        self.stream.reset();
        self.pending.clear();
        self.position = 0;
        self.speaking = false;
    }
}
