//! A stream over a fake detector: options checked, audio in pieces of any length taken a whole window at a time,
//! positions and events, finishing and resetting, a failing window, and the model kept in memory while a stream
//! lives.

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::{VadEvent, VadFrame, VadOptions};
use crate::backend::{BackendModel, Library, VadModel, VadStreamModel, Window};
use crate::capability::Resident;
use crate::catalog::BuildEntry;
use crate::install::Installed;
use crate::test_support::block_on;
use crate::{async_trait, Accelerator, Capability, Error, LocalModel, Result};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// Samples per window of the fake detector.
const WINDOW: usize = 4;

/// A detector whose windows are speech when their first sample is above the threshold, and fail when it is negative.
/// It counts how many of it are alive.
struct FakeVad(Arc<AtomicUsize>);

impl Drop for FakeVad {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl BackendModel for FakeVad {
    fn as_vad(&mut self) -> Option<&mut dyn VadModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

impl VadModel for FakeVad {
    fn sample_rate(&self) -> u32 {
        8_000
    }

    fn window(&self) -> usize {
        WINDOW
    }

    fn stream(&mut self, options: &VadOptions) -> Result<Box<dyn VadStreamModel>> {
        Ok(Box::new(FakeStream {
            threshold: options.threshold,
            at: 0,
            start: None,
        }))
    }
}

struct FakeStream {
    threshold: f32,
    at: u64,
    start: Option<u64>,
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl VadStreamModel for FakeStream {
    async fn window(&mut self, pcm: &[f32]) -> Result<Window> {
        assert_eq!(pcm.len(), WINDOW, "a whole window at a time");
        let level = pcm[0];
        if level < 0.0 {
            return Err(Error::new("detection-failed"));
        }
        let begins = self.at;
        self.at += WINDOW as u64;
        let speech = level > self.threshold;
        let mut ended = Vec::new();
        if speech {
            self.start.get_or_insert(begins);
        } else if let Some(start) = self.start.take() {
            ended.push(start..begins);
        }
        Ok(Window {
            speech,
            probability: Some(level),
            ended,
        })
    }

    fn finish(&mut self) -> Option<Range<u64>> {
        let ended = self.start.take().map(|start| start..self.at);
        self.at = 0;
        ended
    }

    fn reset(&mut self) {
        self.at = 0;
        self.start = None;
    }
}

/// A library the fake detector never needs.
struct NoLibrary;

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Library for NoLibrary {
    async fn load(
        &self,
        _build: &BuildEntry,
        _accelerator: Accelerator,
        _files: &Installed,
    ) -> Result<Box<dyn BackendModel>> {
        Err(Error::new("not-implemented"))
    }
}

/// The fake detector, loaded, and the count of it alive.
fn loaded() -> (LocalModel, Arc<AtomicUsize>) {
    let alive = Arc::new(AtomicUsize::new(1));
    let model = Box::new(FakeVad(Arc::clone(&alive)));
    let resident = Resident::new(model, Some(Arc::new(NoLibrary)), Vec::new(), Vec::new());
    (LocalModel::new("vad", "vad/fake", resident), alive)
}

/// `windows` windows at `level`.
fn audio(windows: usize, level: f32) -> Vec<f32> {
    vec![level; windows * WINDOW]
}

#[test]
fn a_detector_is_a_voice_activity_model_only() {
    let (loaded, _) = loaded();
    assert_eq!(loaded.capabilities(), [Capability::Vad]);
    assert!(loaded.as_vad().is_some());
    assert!(loaded.as_stt().is_none() && loaded.as_tts().is_none());
}

#[test]
fn options_out_of_their_bounds_are_refused() {
    let (loaded, _) = loaded();
    let vad = loaded.as_vad().expect("a detector");
    let code = |options| block_on(vad.stream(options)).map(drop).unwrap_err().code;
    let defaults = VadOptions::default();
    for threshold in [0.0, 0.005, 1.0, 1.5, f32::NAN] {
        let options = VadOptions {
            threshold,
            ..defaults
        };
        assert_eq!(code(options), "invalid-vad-options", "{threshold}");
    }
    let silence = VadOptions {
        min_silence_ms: 0,
        ..defaults
    };
    let speech = VadOptions {
        min_speech_ms: 0,
        ..defaults
    };
    assert_eq!(code(silence), "invalid-vad-options");
    assert_eq!(code(speech), "invalid-vad-options");
    assert_eq!(
        (
            defaults.threshold,
            defaults.min_silence_ms,
            defaults.min_speech_ms
        ),
        (0.5, 500, 250)
    );
}

#[test]
fn audio_in_pieces_of_any_length_is_taken_a_whole_window_at_a_time() {
    let (loaded, _) = loaded();
    let vad = loaded.as_vad().expect("a detector");
    let mut stream = block_on(vad.stream(VadOptions::default())).expect("a stream");
    assert_eq!((stream.sample_rate(), stream.window()), (8_000, WINDOW));

    let output = block_on(stream.accept(&[0.0; 6])).expect("accepted");
    assert_eq!(
        output.frames,
        [VadFrame {
            end: 4,
            speech: false,
            probability: Some(0.0)
        }]
    );
    assert!(output.events.is_empty());
    let output = block_on(stream.accept(&[0.0; 1])).expect("accepted");
    assert!(output.frames.is_empty(), "7 samples: one window and 3 kept");
    let output = block_on(stream.accept(&[0.9; 9])).expect("accepted");
    let ends: Vec<_> = output.frames.iter().map(|frame| frame.end).collect();
    assert_eq!(ends, [8, 12, 16], "the kept samples come first");
    assert_eq!(output.events, [VadEvent::SpeechStart { at: 12 }]);
    assert!(output.frames[1].speech && output.frames[2].speech);
}

#[test]
fn speech_starts_and_ends_with_its_positions() {
    let (loaded, _) = loaded();
    let vad = loaded.as_vad().expect("a detector");
    let mut stream = block_on(vad.stream(VadOptions::default())).expect("a stream");
    let pcm = [audio(2, 0.0), audio(3, 0.9), audio(2, 0.0), audio(1, 0.9)].concat();
    let output = block_on(stream.accept(&pcm)).expect("accepted");
    assert_eq!(
        output.events,
        [
            VadEvent::SpeechStart { at: 12 },
            VadEvent::SpeechEnd { start: 8, end: 20 },
            VadEvent::SpeechStart { at: 32 },
        ]
    );
    let speech: Vec<_> = output.frames.iter().map(|frame| frame.speech).collect();
    assert_eq!(speech, [false, false, true, true, true, false, false, true]);

    assert_eq!(
        stream.finish(),
        Some(VadEvent::SpeechEnd { start: 28, end: 32 })
    );
    assert_eq!(stream.finish(), None, "nothing in progress any more");
    let output = block_on(stream.accept(&audio(1, 0.0))).expect("accepted");
    assert_eq!(output.frames[0].end, 4, "positions start over");
}

#[test]
fn a_failing_window_fails_the_call_and_a_reset_starts_over() {
    let (loaded, _) = loaded();
    let vad = loaded.as_vad().expect("a detector");
    let mut stream = block_on(vad.stream(VadOptions::default())).expect("a stream");
    let pcm = [audio(1, 0.9), audio(1, -1.0), audio(1, 0.9)].concat();
    let failed = block_on(stream.accept(&pcm)).map(drop).unwrap_err();
    assert_eq!(failed.code, "detection-failed");
    stream.reset();
    let output = block_on(stream.accept(&audio(1, 0.0))).expect("accepted");
    assert_eq!(output.frames[0].end, 4);
    assert!(output.events.is_empty() && stream.finish().is_none());
}

#[test]
fn a_stream_keeps_its_model_in_memory_until_it_is_dropped() {
    let (loaded, alive) = loaded();
    let vad = loaded.as_vad().expect("a detector");
    let mut stream = block_on(vad.stream(VadOptions::default())).expect("a stream");
    let mut other = block_on(vad.stream(VadOptions::default())).expect("another stream");
    drop(loaded);
    assert_eq!(alive.load(Ordering::Relaxed), 1);
    let one = block_on(stream.accept(&audio(2, 0.9))).expect("accepted");
    let two = block_on(other.accept(&audio(1, 0.0))).expect("accepted");
    assert_eq!(
        (one.frames.len(), two.frames.len()),
        (2, 1),
        "states of their own"
    );
    drop(stream);
    assert_eq!(alive.load(Ordering::Relaxed), 1, "`other` still holds it");
    drop(other);
    assert_eq!(alive.load(Ordering::Relaxed), 0);
}
