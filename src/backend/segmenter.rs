//! Speech segments from a voice activity model's probabilities, one per window, for a backend whose library only gives
//! the probability (Silero on ONNX Runtime Web). The rules are sherpa-onnx's own (`silero-vad-model.cc` and
//! `voice-activity-detector.cc`, at the version Cargo.lock pins), so a stream's events mean the same whichever backend
//! runs it:
//!
//! - a window is speech above `threshold`; speech is confirmed once it has lasted `min_speech`;
//! - once confirmed, it goes on while the probability stays above `threshold - 0.15` (at least 0.01), and ends after
//!   `min_silence` below `threshold`;
//! - a segment starts two model windows plus `min_speech` before the window that confirmed it (never before the end of
//!   the one before), and ends `min_silence` before the window that ended it.
//!
//! Positions are in samples, counted from the start; a segment is its first sample and the one after its last.
#![cfg_attr(
    native,
    allow(
        dead_code,
        reason = "only the web build's backend segments speech itself; its tests run in every build"
    )
)]

use std::ops::Range;

use crate::backend::Window;
use crate::capability::VadOptions;

#[cfg(test)]
mod tests;

/// One stream's segmentation state.
#[derive(Debug, Clone)]
pub(crate) struct Segmenter {
    threshold: f32,
    /// Below it, confirmed speech counts as silence.
    neg_threshold: f32,
    min_silence: u64,
    min_speech: u64,
    /// The samples each window moves the stream on by.
    shift: u64,
    /// How far before the confirming window a segment starts, `min_speech` aside: two windows as the model sees them
    /// (with their context).
    lookback: u64,
    /// The samples taken so far.
    tail: u64,
    /// Where the next segment may start at the earliest: the end of the last one, or where silence was let go.
    head: u64,
    /// Where speech above the threshold began, before it was confirmed (0: none).
    temp_start: u64,
    /// Where confirmed speech fell below the threshold (0: it has not).
    temp_end: u64,
    /// Whether the model is inside confirmed speech.
    triggered: bool,
    /// The start of the segment in progress.
    start: Option<u64>,
}

impl Segmenter {
    /// A segmenter for windows of `shift` new samples, each seen by the model with `context` samples of the one
    /// before, at `sample_rate` Hz.
    pub(crate) fn new(
        options: &VadOptions,
        sample_rate: u32,
        shift: usize,
        context: usize,
    ) -> Self {
        let samples = |ms: u32| u64::from(ms) * u64::from(sample_rate) / 1000;
        Self {
            threshold: options.threshold,
            neg_threshold: (options.threshold - 0.15).max(0.01),
            min_silence: samples(options.min_silence_ms),
            min_speech: samples(options.min_speech_ms),
            shift: shift as u64,
            lookback: 2 * (shift + context) as u64,
            tail: 0,
            head: 0,
            temp_start: 0,
            temp_end: 0,
            triggered: false,
            start: None,
        }
    }

    /// The next window's `probability`, and what is known after it.
    pub(crate) fn window(&mut self, probability: f32) -> Window {
        let is_speech = self.is_speech(probability);
        let mut ended = Vec::new();
        if is_speech {
            if self.start.is_none() {
                let start = self.tail.saturating_sub(self.lookback + self.min_speech);
                self.start = Some(start.max(self.head));
            }
        } else {
            match self.start.take() {
                Some(start) => {
                    let end = self.tail.saturating_sub(self.min_silence);
                    if end > start {
                        ended.push(start..end);
                    }
                    self.head = self.head.max(end);
                }
                None => {
                    let end = self.tail.saturating_sub(self.lookback + self.min_speech);
                    self.head = self.head.max(end);
                }
            }
        }
        Window {
            speech: self.start.is_some(),
            probability: Some(probability),
            ended,
        }
    }

    /// Ends the segment in progress, if any, at the last sample taken, and starts over.
    pub(crate) fn finish(&mut self) -> Option<Range<u64>> {
        let ended = self
            .start
            .filter(|start| *start < self.tail)
            .map(|start| start..self.tail);
        self.reset();
        ended
    }

    /// Back to the start: no speech, positions from zero.
    pub(crate) fn reset(&mut self) {
        self.tail = 0;
        self.head = 0;
        self.temp_start = 0;
        self.temp_end = 0;
        self.triggered = false;
        self.start = None;
    }

    /// Whether the window that ends now, with `probability`, is inside confirmed speech: sherpa-onnx's `IsSpeech`.
    fn is_speech(&mut self, probability: f32) -> bool {
        self.tail += self.shift;
        let above = probability > self.threshold;
        if above && self.temp_end != 0 {
            self.temp_end = 0;
        }
        if above && self.temp_start == 0 {
            self.temp_start = self.tail;
            return false;
        }
        if above && !self.triggered {
            if self.tail - self.temp_start < self.min_speech {
                return false;
            }
            self.triggered = true;
            return true;
        }
        if probability < self.threshold && !self.triggered {
            self.temp_start = 0;
            self.temp_end = 0;
            return false;
        }
        if probability > self.neg_threshold && self.triggered {
            return true;
        }
        if probability < self.threshold && self.triggered {
            if self.temp_end == 0 {
                self.temp_end = self.tail;
            }
            if self.tail - self.temp_end < self.min_silence {
                return true;
            }
            self.temp_start = 0;
            self.temp_end = 0;
            self.triggered = false;
            return false;
        }
        false
    }
}
