//! Real models through the real library: Whisper transcribes a clip whose text is known, Kokoro speaks, and what
//! Kokoro says Whisper hears back. They download what they need (about 250 MB: the library `backends.json` pins for
//! this platform, Whisper tiny.en and Kokoro, both int8, pinned in `inference_tests.json`), so they are ignored by
//! default and CI's inference job runs them:
//!
//! ```sh
//! cargo test --lib sherpa_onnx::inference_tests -- --ignored --nocapture
//! ```
//!
//! They install what they need as the engine does, with its installer and `NativeHost`, whose directory
//! (`target/test-models/`, or `SIDEVOICE_TEST_MODELS`) keeps everything by digest between runs. Core ML is what macOS
//! runs Whisper on; the CPU elsewhere.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use super::SherpaOnnx;
use crate::backend::{runtime_files, Backend, LoadedModel};
use crate::host::{Accelerator, Host, Platform};
use crate::install::{Artifact, Cancel, Installed, Installer};
use crate::test_support::{block_on, build};
use crate::NativeHost;

/// What these tests download besides the library: `inference_tests.json`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixtures {
    /// Whisper's files, each by its key in `Installed`.
    whisper: Vec<Download>,
    /// A clip for Whisper, and what it says.
    clip: Clip,
    /// Kokoro's archive, and its files' paths inside it by their keys in `Installed`.
    kokoro: Archive,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Download {
    key: String,
    url: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Clip {
    url: String,
    sha256: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    url: String,
    sha256: String,
    files: std::collections::BTreeMap<String, String>,
}

/// Everything installed, once for all the tests: each model's `Installed`, and the clip.
struct Prepared {
    whisper: Installed,
    kokoro: Installed,
    clip: Vec<f32>,
    clip_text: String,
}

fn prepared() -> &'static Prepared {
    static PREPARED: OnceLock<Prepared> = OnceLock::new();
    PREPARED.get_or_init(|| {
        let fixtures: Fixtures = serde_json::from_str(include_str!("inference_tests.json"))
            .expect("inference_tests.json");
        let cache = std::env::var_os("SIDEVOICE_TEST_MODELS").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-models"),
            PathBuf::from,
        );
        let host = NativeHost::new(cache).expect("the cache directory");
        let platform = Platform::of(&host.capabilities()).expect("a platform backends.json knows");
        let library = runtime_files("sherpa-onnx", platform).expect("sherpa-onnx runs here");

        let file = |key: &str, url: &str, sha256: &str| Artifact {
            key: key.to_owned(),
            url: url.to_owned(),
            sha256: sha256.to_owned(),
            archive_path: None,
        };
        let install = |artifacts: Vec<Artifact>| {
            let installed = block_on(Installer.install(&artifacts, &host, &|_| {}, &Cancel::new()));
            installed.expect("installed")
        };
        let whisper = fixtures
            .whisper
            .iter()
            .map(|download| file(&download.key, &download.url, &download.sha256));
        let kokoro = fixtures.kokoro.files.iter().map(|(key, inside)| Artifact {
            archive_path: Some(inside.clone()),
            ..file(key, &fixtures.kokoro.url, &fixtures.kokoro.sha256)
        });
        let clip = install(vec![file(
            "clip",
            &fixtures.clip.url,
            &fixtures.clip.sha256,
        )]);
        let clip = fs::read(clip.file("clip").expect("the clip")).expect("the clip");
        Prepared {
            whisper: install(whisper.chain(library.clone()).collect()),
            kokoro: install(kokoro.chain(library).collect()),
            clip: wav_16k_mono(&clip),
            clip_text: fixtures.clip.text,
        }
    })
}

/// The samples of a 16-bit PCM WAV file at 16 kHz, mono, as floats in [-1, 1).
fn wav_16k_mono(wav: &[u8]) -> Vec<f32> {
    let u16_at = |at: usize| u16::from_le_bytes([wav[at], wav[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes([wav[at], wav[at + 1], wav[at + 2], wav[at + 3]]);
    assert_eq!((&wav[..4], &wav[8..12]), (&b"RIFF"[..], &b"WAVE"[..]));
    let mut at = 12;
    let mut format_ok = false;
    while at + 8 <= wav.len() {
        let (id, len) = (&wav[at..at + 4], u32_at(at + 4) as usize);
        let body = at + 8;
        match id {
            b"fmt " => {
                // PCM, one channel, 16 kHz, 16 bits.
                format_ok = (
                    u16_at(body),
                    u16_at(body + 2),
                    u32_at(body + 4),
                    u16_at(body + 14),
                ) == (1, 1, 16_000, 16);
            }
            b"data" => {
                assert!(format_ok, "the clip is 16-bit PCM, mono, at 16 kHz");
                return wav[body..body + len]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|sample| f32::from(i16::from_le_bytes(*sample)) / 32_768.0)
                    .collect();
            }
            _ => {}
        }
        at = body + len + (len & 1);
    }
    panic!("the clip has no data");
}

/// What the app would pick: Core ML on macOS for Whisper, and the CPU for everything else (Kokoro runs on the CPU
/// only, see *Accelerators* in `sherpa_onnx.rs`).
fn accelerator(files: &Installed) -> Accelerator {
    if std::env::consts::OS == "macos" && files.file("encoder").is_some() {
        Accelerator::CoreMl
    } else {
        Accelerator::Cpu
    }
}

fn load(files: &Installed) -> Box<dyn LoadedModel> {
    let build = build("test", "sherpa-onnx", 0);
    let model = if files.file("encoder").is_some() {
        "Whisper"
    } else {
        "Kokoro"
    };
    eprintln!("loading {model} on {:?}", accelerator(files));
    let library = block_on(SherpaOnnx.open(files)).expect("the library opens");
    let loaded =
        block_on(library.load(&build, accelerator(files), files)).expect("the model loads");
    eprintln!("loaded {model}");
    loaded
}

/// Lower case, letters, digits and single spaces: what is compared of two texts.
fn normalised(text: &str) -> String {
    let kept: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `samples` at `from` Hz, linearly resampled to 16 kHz: enough for Whisper to hear speech.
fn to_16k(samples: &[f32], from: u32) -> Vec<f32> {
    let step = f64::from(from) / 16_000.0;
    let len = (samples.len() as f64 / step) as usize;
    (0..len)
        .map(|i| {
            let at = i as f64 * step;
            let (index, frac) = (at as usize, (at.fract()) as f32);
            let next = samples.get(index + 1).copied().unwrap_or(samples[index]);
            samples[index] + (next - samples[index]) * frac
        })
        .collect()
}

#[test]
#[ignore = "downloads about 250 MB: cargo test --lib sherpa_onnx::inference_tests -- --ignored"]
fn whisper_transcribes_a_clip() {
    let prepared = prepared();
    let mut model = load(&prepared.whisper);
    assert!(model.as_tts().is_none());
    let stt = model.as_stt().expect("Whisper transcribes");
    let text = block_on(stt.transcribe(&prepared.clip, Some("en"))).expect("a transcript");
    println!("Whisper heard: {text:?}");
    assert_eq!(normalised(&text), prepared.clip_text);
    // Detecting the language, and a regional tag, give the same text.
    let detected = block_on(stt.transcribe(&prepared.clip, None)).expect("a transcript");
    let regional = block_on(stt.transcribe(&prepared.clip, Some("en-GB"))).expect("a transcript");
    assert_eq!(normalised(&detected), prepared.clip_text);
    assert_eq!(normalised(&regional), prepared.clip_text);
}

#[test]
#[ignore = "downloads about 250 MB: cargo test --lib sherpa_onnx::inference_tests -- --ignored"]
fn kokoro_speaks_a_sentence() {
    let mut model = load(&prepared().kokoro);
    assert!(model.as_stt().is_none());
    let tts = model.as_tts().expect("Kokoro speaks");
    let voices = tts.voices();
    println!("Kokoro's voices: {voices:?}");
    assert!(
        voices.iter().any(|voice| voice == "af_bella"),
        "named voices"
    );
    assert_eq!(tts.sample_rate(), 24_000);
    let audio =
        block_on(tts.speak("Hello from the engine.", "af_bella", None, 1.0)).expect("speech");
    let seconds = audio.len() as f32 / tts.sample_rate() as f32;
    println!("Kokoro spoke {seconds:.2} s");
    assert!((0.5..5.0).contains(&seconds), "{seconds} s of speech");
    let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt();
    assert!(rms > 0.01, "audible: RMS {rms}");
    // Faster is shorter.
    let fast =
        block_on(tts.speak("Hello from the engine.", "af_bella", None, 1.5)).expect("speech");
    assert!(fast.len() < audio.len());
    let unknown = block_on(tts.speak("Hello.", "nobody", None, 1.0)).map(|_| ());
    assert_eq!(unknown.unwrap_err().code, "unknown-voice");
}

#[test]
#[ignore = "downloads about 250 MB: cargo test --lib sherpa_onnx::inference_tests -- --ignored"]
fn whisper_hears_what_kokoro_says() {
    let prepared = prepared();
    let sentence = "The quick brown fox jumps over the lazy dog.";
    let mut kokoro = load(&prepared.kokoro);
    let tts = kokoro.as_tts().expect("Kokoro speaks");
    let speech = block_on(tts.speak(sentence, "am_adam", None, 1.0)).expect("speech");
    let speech = to_16k(&speech, tts.sample_rate());
    let mut whisper = load(&prepared.whisper);
    let stt = whisper.as_stt().expect("Whisper transcribes");
    let heard = block_on(stt.transcribe(&speech, Some("en"))).expect("a transcript");
    println!("Kokoro said {sentence:?}; Whisper heard {heard:?}");
    assert_eq!(normalised(&heard), normalised(sentence));
}
