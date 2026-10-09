//! What smart-turn v3 is given, for every backend that runs it (`onnx_runtime.rs` natively, transformers.js on the
//! web): the same input on every platform, made here rather than by each runtime. As its reference inference
//! (pipecat-ai/smart-turn, `inference.py`) makes it:
//!
//! - the turn's last [`SECONDS`] at 16 kHz, zero-padded at the start when shorter;
//! - normalized to zero mean and unit variance over those samples (Whisper's `do_normalize`);
//! - Whisper's log-mel features (`WhisperFeatureExtractor`): a 400-sample periodic Hann window every 160 samples, the
//!   signal reflected 200 samples at each end (an FFT), the power spectrum through 80 Slaney mel filters from 0 to 8 kHz,
//!   `log10` floored at 1e-10, the last frame dropped, floored at 8 below the maximum, and `(x + 4) / 4`.
//!
//! The result is `input_features`, [1, [`MELS`], [`FRAMES`]], row-major by mel. The model answers the probability that
//! the turn is complete.

use std::f64::consts::PI;
use std::sync::{Arc, OnceLock};

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

#[cfg(test)]
mod tests;

/// The rate the model hears, in Hz.
pub(crate) const SAMPLE_RATE: u32 = 16_000;
/// How much of the end of a turn it hears.
pub(crate) const SECONDS: u32 = 8;
/// Its mel bins and frames.
pub(crate) const MELS: usize = 80;
pub(crate) const FRAMES: usize = 800;
/// The name of its input.
pub(crate) const INPUT: &str = "input_features";

const SAMPLES: usize = (SAMPLE_RATE * SECONDS) as usize;
const WINDOW: usize = 400;
const HOP: usize = 160;
const BINS: usize = WINDOW / 2 + 1;

/// `input_features` for the turn `pcm` (mono, 16 kHz): [`MELS`] rows of [`FRAMES`], all at once on the calling thread
/// (a native backend's call runs there: the app decides where).
#[cfg_attr(
    web,
    allow(dead_code, reason = "the web build extracts a chunk at a time")
)]
pub(crate) fn features(pcm: &[f32]) -> Vec<f32> {
    let mut extractor = Extractor::new(pcm);
    extractor.frames(0..FRAMES);
    extractor.finish()
}

/// The same, [`CHUNK`] frames at a time, letting the page run between chunks: in a page the engine runs on the page's
/// own thread, as all of its work there does, so a long computation is cut into short ones rather than block it.
#[cfg_attr(
    native,
    allow(dead_code, reason = "native backends extract on the caller's thread")
)]
pub(crate) async fn features_yielding(pcm: &[f32]) -> Vec<f32> {
    let mut extractor = Extractor::new(pcm);
    for start in (0..FRAMES).step_by(CHUNK) {
        extractor.frames(start..(start + CHUNK).min(FRAMES));
        yield_now().await;
    }
    extractor.finish()
}

/// How many frames are computed between two chances for the page to run.
pub(crate) const CHUNK: usize = 100;

/// One extraction under way: the signal, centred, and the log-mel energies computed so far.
struct Extractor {
    padded: Vec<f64>,
    log: Vec<f64>,
    buffer: Vec<Complex<f64>>,
    scratch: Vec<Complex<f64>>,
}

impl Extractor {
    fn new(pcm: &[f32]) -> Self {
        let audio = normalized(&last_seconds(pcm));
        // Centred frames: the signal reflected half a window at each end.
        let half = WINDOW / 2;
        let padded = (0..SAMPLES + WINDOW)
            .map(|i| {
                let at = i as isize - half as isize;
                let reflected = if at < 0 {
                    -at
                } else if at as usize >= SAMPLES {
                    2 * (SAMPLES as isize - 1) - at
                } else {
                    at
                };
                audio[reflected as usize]
            })
            .collect();
        let fft = &tables().fft;
        Self {
            padded,
            log: vec![0.0; MELS * FRAMES],
            buffer: vec![Complex::default(); WINDOW],
            scratch: vec![Complex::default(); fft.get_inplace_scratch_len()],
        }
    }

    /// The log-mel energies of `frames`.
    fn frames(&mut self, frames: std::ops::Range<usize>) {
        let tables = tables();
        let mut power = [0.0f64; BINS];
        for frame in frames {
            let samples = &self.padded[frame * HOP..frame * HOP + WINDOW];
            for ((slot, sample), weight) in self.buffer.iter_mut().zip(samples).zip(&tables.window)
            {
                *slot = Complex::new(sample * weight, 0.0);
            }
            tables
                .fft
                .process_with_scratch(&mut self.buffer, &mut self.scratch);
            for (power, bin) in power.iter_mut().zip(&self.buffer) {
                *power = bin.norm_sqr();
            }
            for (mel, bins) in tables.spans.iter().enumerate() {
                let filter = &tables.filters[mel * BINS..(mel + 1) * BINS];
                let energy: f64 = filter[bins.clone()]
                    .iter()
                    .zip(&power[bins.clone()])
                    .map(|(f, p)| f * p)
                    .sum();
                self.log[mel * FRAMES + frame] = energy.max(1e-10).log10();
            }
        }
    }

    /// The features, once every frame is computed: floored at 8 below the maximum, and `(x + 4) / 4`.
    fn finish(self) -> Vec<f32> {
        let floor = self.log.iter().copied().fold(f64::NEG_INFINITY, f64::max) - 8.0;
        self.log
            .iter()
            .map(|value| ((value.max(floor) + 4.0) / 4.0) as f32)
            .collect()
    }
}

/// Lets the page run whatever is waiting (input, rendering, other work), then carries on: `scheduler.yield()` where
/// the page has it, else a message to itself, which no timer clamps.
#[cfg(web)]
async fn yield_now() {
    #[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function yieldToPage() {
  if (globalThis.scheduler?.yield) return globalThis.scheduler.yield();
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => { channel.port1.close(); resolve(); };
    channel.port2.postMessage(null);
  });
}
"#)]
    extern "C" {
        #[wasm_bindgen(js_name = yieldToPage)]
        fn yield_to_page() -> js_sys::Promise;
    }
    // A page that cannot yield carries on at once: the result is the same.
    let _ = wasm_bindgen_futures::JsFuture::from(yield_to_page()).await;
}

/// Natively the caller's thread is the caller's: nothing to yield to.
#[cfg(native)]
async fn yield_now() {}

/// The last [`SECONDS`] of `pcm`, zero-padded at the start when shorter.
fn last_seconds(pcm: &[f32]) -> Vec<f64> {
    let kept = &pcm[pcm.len().saturating_sub(SAMPLES)..];
    let mut audio = vec![0.0; SAMPLES - kept.len()];
    audio.extend(kept.iter().map(|sample| f64::from(*sample)));
    audio
}

/// `audio` with zero mean and unit variance, as Whisper's feature extractor normalizes it.
fn normalized(audio: &[f64]) -> Vec<f64> {
    let n = audio.len() as f64;
    let mean = audio.iter().sum::<f64>() / n;
    let variance = audio.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let scale = (variance + 1e-7).sqrt();
    audio.iter().map(|x| (x - mean) / scale).collect()
}

/// The window, the FFT and the mel filters, made once.
struct Tables {
    window: Vec<f64>,
    fft: Arc<dyn Fft<f64>>,
    /// [`MELS`] rows of [`BINS`].
    filters: Vec<f64>,
    /// The bins where each filter is not zero: a triangle spans a few of them.
    spans: Vec<std::ops::Range<usize>>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let turn = |k: usize| 2.0 * PI * k as f64 / WINDOW as f64;
        let filters = mel_filters();
        let spans = filters
            .chunks(BINS)
            .map(|filter| {
                let first = filter.iter().position(|weight| *weight > 0.0).unwrap_or(0);
                let last = filter
                    .iter()
                    .rposition(|weight| *weight > 0.0)
                    .map_or(0, |last| last + 1);
                first..last.max(first)
            })
            .collect();
        Tables {
            // Periodic Hann.
            window: (0..WINDOW).map(|n| 0.5 - 0.5 * turn(n).cos()).collect(),
            fft: FftPlanner::new().plan_fft_forward(WINDOW),
            filters,
            spans,
        }
    })
}

/// Slaney's mel scale: linear below 1 kHz, logarithmic above.
fn hz_to_mel(hz: f64) -> f64 {
    let step = 27.0 / 6.4f64.ln();
    if hz < 1000.0 {
        3.0 * hz / 200.0
    } else {
        15.0 + (hz / 1000.0).ln() * step
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    let step = 6.4f64.ln() / 27.0;
    if mel < 15.0 {
        200.0 * mel / 3.0
    } else {
        1000.0 * ((mel - 15.0) * step).exp()
    }
}

/// [`MELS`] triangular filters over [`BINS`] frequency bins from 0 to 8 kHz, Slaney-normalized, as transformers'
/// `mel_filter_bank` makes them for Whisper.
fn mel_filters() -> Vec<f64> {
    let nyquist = f64::from(SAMPLE_RATE) / 2.0;
    let (low, high) = (hz_to_mel(0.0), hz_to_mel(nyquist));
    let edges: Vec<f64> = (0..MELS + 2)
        .map(|i| mel_to_hz(low + (high - low) * i as f64 / (MELS + 1) as f64))
        .collect();
    let frequencies: Vec<f64> = (0..BINS)
        .map(|bin| nyquist * bin as f64 / (BINS - 1) as f64)
        .collect();
    let mut filters = vec![0.0; MELS * BINS];
    for mel in 0..MELS {
        let (left, centre, right) = (edges[mel], edges[mel + 1], edges[mel + 2]);
        let norm = 2.0 / (right - left);
        for (bin, frequency) in frequencies.iter().enumerate() {
            let down = (frequency - left) / (centre - left);
            let up = (right - frequency) / (right - centre);
            filters[mel * BINS + bin] = down.min(up).max(0.0) * norm;
        }
    }
    filters
}
