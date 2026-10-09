//! Every voice activity detector through sherpa-onnx's `VoiceActivityDetector`, alike: Silero (`silero_vad.model`) and
//! TEN VAD (`ten_vad.model`) take the same options, and sherpa-onnx segments the speech itself. Each stream is a
//! detector of its own, made from the model's config with the stream's options, so streams share nothing.
//!
//! sherpa-onnx's C API (1.13.8) tells whether a stream is in speech and hands over each segment as it ends; it does
//! not tell the probability of each window, so frames carry none. Its detector cuts a segment that outlasts
//! `max_speech_duration`; ending a turn is the caller's, so that is set past any turn ([`MAX_SPEECH_S`]).

use std::ops::Range;

use async_trait::async_trait;
use sherpa_onnx::{VadModelConfig, VoiceActivityDetector};

use crate::backend::{BackendModel, VadModel, VadStreamModel, Window};
use crate::engine::VadOptions;
use crate::{Error, Result};

/// The rate sherpa-onnx's detectors take.
const SAMPLE_RATE: u32 = 16_000;

/// The samples Silero takes at a time at 16 kHz (v5 needs exactly these), and TEN VAD's hop.
const SILERO_WINDOW: usize = 512;
const TEN_WINDOW: usize = 256;

/// How long a segment may run before sherpa-onnx cuts it: an hour, which no turn reaches.
const MAX_SPEECH_S: f32 = 3_600.0;

/// The seconds of audio a detector's buffer holds at first; it grows when speech runs longer.
const BUFFER_S: f32 = 60.0;

/// A voice activity detector in memory: the config its streams are made from.
pub(super) struct Detector {
    config: VadModelConfig,
    window: usize,
}

impl Detector {
    /// From `config`, the build's files in it (`config.rs`), which it checks by making a detector with the default
    /// options. Fails with `unsupported-model` for a config with no model, and `model-load-failed`.
    pub(super) fn load(mut config: VadModelConfig) -> Result<Self> {
        let window = if config.silero_vad.model.is_some() {
            SILERO_WINDOW
        } else if config.ten_vad.model.is_some() {
            TEN_WINDOW
        } else {
            return Err(Error::new("unsupported-model"));
        };
        config.sample_rate = SAMPLE_RATE as i32;
        config.silero_vad.window_size = SILERO_WINDOW as i32;
        config.ten_vad.window_size = TEN_WINDOW as i32;
        let detector = Self { config, window };
        detector.detector(&VadOptions::default())?;
        Ok(detector)
    }

    /// A new detector, deciding with `options`.
    fn detector(&self, options: &VadOptions) -> Result<VoiceActivityDetector> {
        let mut config = self.config.clone();
        for (threshold, min_silence, min_speech, max_speech) in [
            (
                &mut config.silero_vad.threshold,
                &mut config.silero_vad.min_silence_duration,
                &mut config.silero_vad.min_speech_duration,
                &mut config.silero_vad.max_speech_duration,
            ),
            (
                &mut config.ten_vad.threshold,
                &mut config.ten_vad.min_silence_duration,
                &mut config.ten_vad.min_speech_duration,
                &mut config.ten_vad.max_speech_duration,
            ),
        ] {
            *threshold = options.threshold;
            *min_silence = options.min_silence_ms as f32 / 1000.0;
            *min_speech = options.min_speech_ms as f32 / 1000.0;
            *max_speech = MAX_SPEECH_S;
        }
        VoiceActivityDetector::create(&config, BUFFER_S).ok_or(Error::new("model-load-failed"))
    }
}

impl BackendModel for Detector {
    fn as_vad(&mut self) -> Option<&mut dyn VadModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

impl VadModel for Detector {
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    fn window(&self) -> usize {
        self.window
    }

    fn stream(&mut self, options: &VadOptions) -> Result<Box<dyn VadStreamModel>> {
        Ok(Box::new(DetectorStream(self.detector(options)?)))
    }
}

/// One stream: a detector of its own.
struct DetectorStream(VoiceActivityDetector);

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl VadStreamModel for DetectorStream {
    /// Runs on the calling thread. Silero v5 sees each window with the end of the one before, so sherpa-onnx runs a
    /// window once the next one has come: what a window says is about the one before it.
    async fn window(&mut self, pcm: &[f32]) -> Result<Window> {
        self.0.accept_waveform(pcm);
        Ok(Window {
            ended: segments(&self.0),
            speech: self.0.detected(),
            probability: None,
        })
    }

    fn finish(&mut self) -> Option<Range<u64>> {
        self.0.flush();
        let ended = segments(&self.0).pop();
        self.0.reset();
        ended
    }

    fn reset(&mut self) {
        self.0.reset();
    }
}

/// The segments `detector` has ended since it was last asked, taken out of it: each from its first sample to the one
/// after its last.
fn segments(detector: &VoiceActivityDetector) -> Vec<Range<u64>> {
    let mut ended = Vec::new();
    while let Some(segment) = detector.front() {
        let start = u64::try_from(segment.start()).unwrap_or_default();
        let samples = u64::try_from(segment.n()).unwrap_or_default();
        ended.push(start..start + samples);
        drop(segment);
        detector.pop();
    }
    ended
}
