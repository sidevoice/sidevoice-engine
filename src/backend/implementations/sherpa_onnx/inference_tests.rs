//! Real models through the linked library: Whisper transcribes a clip whose text is known, Kokoro speaks, and what
//! Kokoro says Whisper hears back. And each catalogue build `inference_tests.json` names under `transcribe` is installed
//! from the bundled catalogue, as an app installs it, and transcribes the shared clips it names (LibriSpeech in
//! English, FLEURS in Spanish, both CC-BY-4.0) within the word error rate `max_wer`: adding such a test is one entry.
//!
//! They download the models they need (Whisper tiny.en and Kokoro, about 150 MB, then each build named; several GB),
//! so they are ignored by default and CI's inference job runs them, with the plain tests beside them:
//!
//! ```sh
//! cargo test --lib sherpa_onnx::inference_tests -- --include-ignored --nocapture
//! ```
//!
//! The first three predate the installer: each of their files is downloaded with `curl`, checked against its digest,
//! kept in a cache by digest (`target/test-models/`, or `SIDEVOICE_TEST_MODELS`), and the archives are unpacked there;
//! the clips are kept there too. The catalogue builds are installed in a directory of their own, one at a time. They
//! run on the CPU, the one accelerator the linked libraries have.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde::Deserialize;

use super::SherpaOnnx;
use crate::backend::{Backend, BackendModel};
use crate::catalog::Voice;
use crate::host::Accelerator;
use crate::install::Installed;
use crate::test_support::{build, ready, sha256 as sha256_of, Loading};

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
    /// The highest word error rate a transcription of [`Fixtures::clips`] may have.
    max_wer: f32,
    /// Clips of known text, by language: the shared fixtures of [`Fixtures::transcribe`].
    clips: BTreeMap<String, NamedClip>,
    /// Catalogue builds, each with the clips it must transcribe.
    transcribe: Vec<Transcribe>,
}

/// A clip of known text, and where it comes from.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NamedClip {
    url: String,
    sha256: String,
    text: String,
    source: String,
    license: String,
}

/// A build of the bundled catalogue, installed as an app installs it, and the clips it must transcribe, each told its
/// language (which reaches the model where the build's `call_params` map it); `detect`, the clips it must also
/// transcribe told none, after the others (so the model switches recognizers); `note` says why a language it has is
/// left out.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transcribe {
    build: String,
    clips: Vec<String>,
    #[serde(default)]
    detect: Vec<String>,
    #[serde(default)]
    note: Option<String>,
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
    files: BTreeMap<String, String>,
}

/// Everything downloaded and unpacked, once for all the tests: each model's `Installed`, and the clip.
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
        fs::create_dir_all(&cache).expect("the cache directory");

        let whisper: BTreeMap<_, _> = fixtures
            .whisper
            .iter()
            .map(|file| {
                (
                    file.key.clone(),
                    path_text(&downloaded(&cache, &file.url, &file.sha256)),
                )
            })
            .collect();

        let archive = unpacked(&cache, &fixtures.kokoro.url, &fixtures.kokoro.sha256);
        let kokoro: BTreeMap<_, _> = fixtures
            .kokoro
            .files
            .iter()
            .map(|(key, inside)| (key.clone(), path_text(&archive.join(inside))))
            .collect();

        let clip = downloaded(&cache, &fixtures.clip.url, &fixtures.clip.sha256);
        Prepared {
            whisper: Installed { files: whisper },
            kokoro: Installed { files: kokoro },
            clip: wav_16k_mono(&fs::read(clip).expect("the clip")),
            clip_text: fixtures.clip.text,
        }
    })
}

/// The file at `url`, downloaded into `cache` under its digest unless it is there already.
fn downloaded(cache: &Path, url: &str, sha256: &str) -> PathBuf {
    let path = cache.join(sha256);
    if path.is_file() {
        return path;
    }
    let partial = cache.join(format!("{sha256}.partial"));
    let status = Command::new("curl")
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(&partial)
        .arg(url)
        .status()
        .expect("curl runs");
    assert!(status.success(), "downloading {url}: {status}");
    let digest = sha256_of(&fs::read(&partial).expect("the download"));
    assert_eq!(digest, sha256, "the digest of {url}");
    fs::rename(&partial, &path).expect("the download kept");
    path
}

/// The `tar.bz2` archive at `url`, downloaded and unpacked into `cache` unless it is there already.
fn unpacked(cache: &Path, url: &str, sha256: &str) -> PathBuf {
    let dir = cache.join(format!("{sha256}.d"));
    let complete = dir.join(".complete");
    if complete.is_file() {
        return dir;
    }
    let archive = downloaded(cache, url, sha256);
    let _ = fs::remove_dir_all(&dir);
    let file = File::open(&archive).expect("the archive");
    tar::Archive::new(bzip2::read::BzDecoder::new(file))
        .unpack(&dir)
        .expect("the archive unpacks");
    fs::write(&complete, "").expect("the archive marked unpacked");
    // Unpacked, the archive is not needed again: the cache keeps one copy.
    let _ = fs::remove_file(archive);
    dir
}

fn path_text(path: &Path) -> String {
    path.to_str().expect("a UTF-8 cache path").to_owned()
}

/// The samples of a 16-bit PCM WAV file, mixed down to mono and resampled to 16 kHz, as floats in [-1, 1).
fn wav_16k_mono(wav: &[u8]) -> Vec<f32> {
    let u16_at = |at: usize| u16::from_le_bytes([wav[at], wav[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes([wav[at], wav[at + 1], wav[at + 2], wav[at + 3]]);
    assert_eq!((&wav[..4], &wav[8..12]), (&b"RIFF"[..], &b"WAVE"[..]));
    let mut at = 12;
    let mut format = None;
    while at + 8 <= wav.len() {
        let (id, len) = (&wav[at..at + 4], u32_at(at + 4) as usize);
        let body = at + 8;
        match id {
            b"fmt " => {
                // PCM, 16 bits: its channels and rate.
                assert_eq!((u16_at(body), u16_at(body + 14)), (1, 16), "16-bit PCM");
                format = Some((usize::from(u16_at(body + 2)), u32_at(body + 4)));
            }
            b"data" => {
                let (channels, rate) = format.expect("the format before the data");
                let samples: Vec<f32> = wav[body..(body + len).min(wav.len())]
                    .as_chunks::<2>()
                    .0
                    .chunks(channels)
                    .map(|frame| {
                        let sum: f32 = frame
                            .iter()
                            .map(|sample| f32::from(i16::from_le_bytes(*sample)) / 32_768.0)
                            .sum();
                        sum / frame.len() as f32
                    })
                    .collect();
                return if rate == 16_000 {
                    samples
                } else {
                    to_16k(&samples, rate)
                };
            }
            _ => {}
        }
        at = body + len + (len & 1);
    }
    panic!("the clip has no data");
}

/// What the app would pick: the CPU, the one accelerator the linked libraries have (see *Accelerators* in
/// `sherpa_onnx.rs`).
fn accelerator(_files: &Installed) -> Accelerator {
    Accelerator::Cpu
}

fn load(files: &Installed) -> Box<dyn BackendModel> {
    let model = if files.file("whisper.encoder").is_some() {
        "Whisper"
    } else {
        "Kokoro"
    };
    eprintln!("loading {model} on {:?}", accelerator(files));
    let library = ready(SherpaOnnx.open(files)).expect("the linked library");
    let (loading, build) = (Loading::new(), build("test", "sherpa-onnx", 0));
    let loaded = ready(library.load(loading.of(&build, accelerator(files), files)))
        .expect("the model loads");
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
    let text = ready(stt.transcribe(&prepared.clip, Some("en"))).expect("a transcript");
    println!("Whisper heard: {text:?}");
    assert_eq!(normalised(&text), prepared.clip_text);
    // This build maps no `call_params`, so the language does not reach the model: with it, without it, the same text.
    let detected = ready(stt.transcribe(&prepared.clip, None)).expect("a transcript");
    let regional = ready(stt.transcribe(&prepared.clip, Some("en-GB"))).expect("a transcript");
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
        ready(tts.speak("Hello from the engine.", &voice("af_bella"), None, 1.0)).expect("speech");
    let seconds = audio.len() as f32 / tts.sample_rate() as f32;
    println!("Kokoro spoke {seconds:.2} s");
    assert!((0.5..5.0).contains(&seconds), "{seconds} s of speech");
    let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt();
    assert!(rms > 0.01, "audible: RMS {rms}");
    // Faster is shorter.
    let fast =
        ready(tts.speak("Hello from the engine.", &voice("af_bella"), None, 1.5)).expect("speech");
    assert!(fast.len() < audio.len());
    let unknown = ready(tts.speak("Hello.", &voice("nobody"), None, 1.0)).map(|_| ());
    assert_eq!(unknown.unwrap_err().code, "unknown-voice");
}

#[test]
#[ignore = "downloads about 250 MB: cargo test --lib sherpa_onnx::inference_tests -- --ignored"]
fn whisper_hears_what_kokoro_says() {
    let prepared = prepared();
    let sentence = "The quick brown fox jumps over the lazy dog.";
    let mut kokoro = load(&prepared.kokoro);
    let tts = kokoro.as_tts().expect("Kokoro speaks");
    let speech = ready(tts.speak(sentence, &voice("am_adam"), Some("en-US"), 1.0)).expect("speech");
    let speech = to_16k(&speech, tts.sample_rate());
    let mut whisper = load(&prepared.whisper);
    let stt = whisper.as_stt().expect("Whisper transcribes");
    let heard = ready(stt.transcribe(&speech, Some("en"))).expect("a transcript");
    println!("Kokoro said {sentence:?}; Whisper heard {heard:?}");
    assert_eq!(normalised(&heard), normalised(sentence));
}

/// `text` for a word error rate: [`normalised`], with the acute and diaeresis accents of vowels folded (Whisper writes
/// "Tierra" and "tierra", "movimiento" with or without them alike).
fn words(text: &str) -> Vec<String> {
    let folded: String = text
        .chars()
        .map(|c| match c {
            'á' | 'Á' => 'a',
            'é' | 'É' => 'e',
            'í' | 'Í' => 'i',
            'ó' | 'Ó' => 'o',
            'ú' | 'Ú' | 'ü' | 'Ü' => 'u',
            c => c,
        })
        .collect();
    normalised(&folded)
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The word error rate of `heard` against `said`: the words to substitute, insert or delete, over the words said.
fn wer(said: &str, heard: &str) -> f32 {
    let (said, heard) = (words(said), words(heard));
    let mut row: Vec<usize> = (0..=heard.len()).collect();
    for (i, word) in said.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, other) in heard.iter().enumerate() {
            let substituted = diagonal + usize::from(word != other);
            diagonal = row[j + 1];
            row[j + 1] = substituted.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[heard.len()] as f32 / said.len().max(1) as f32
}

#[test]
fn the_word_error_rate_counts_edits_over_the_words_said() {
    assert_eq!(wer("Hola, ¿qué tal?", "hola que tal"), 0.0);
    assert_eq!(wer("one two three four", "one too three"), 0.5);
    assert_eq!(wer("one", ""), 1.0);
}

/// Each catalogue build `inference_tests.json` names under `transcribe`, installed from the bundled catalogue through
/// the engine's installer and `NativeHost`, as an app installs it, transcribes the clips it names within `max_wer`. One
/// build at a time, uninstalled afterwards, so that the largest is all the disk holds. Every row is printed before the
/// verdict.
#[test]
#[ignore = "downloads every build it names (several GB): cargo test --lib sherpa_onnx::inference_tests -- --ignored"]
fn catalogue_builds_transcribe_the_clips_they_name() {
    use crate::catalog::{BundledCatalog, CatalogSource, ModelFile};
    use crate::host::NativeHost;
    use crate::install::{Cancel, Installer};
    use crate::test_support::block_on;

    let fixtures: Fixtures =
        serde_json::from_str(include_str!("inference_tests.json")).expect("inference_tests.json");
    let cache = std::env::var_os("SIDEVOICE_TEST_MODELS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-models"),
        PathBuf::from,
    );
    fs::create_dir_all(&cache).expect("the cache directory");
    let catalogue = BundledCatalog.load().expect("the bundled catalogue");
    let data = std::env::temp_dir().join(format!("sidevoice-inference-{}", std::process::id()));
    let _ = fs::remove_dir_all(&data);
    let host = NativeHost::new(&data).expect("a host");

    let mut failed = Vec::new();
    println!("| build | clip | WER | heard | load s | transcribe s |");
    for entry in &fixtures.transcribe {
        let build = catalogue
            .families
            .iter()
            .flat_map(|family| &family.models)
            .flat_map(|model| &model.builds)
            .find(|build| build.id == entry.build)
            .unwrap_or_else(|| panic!("{}: not in the bundled catalogue", entry.build));
        if let Some(note) = &entry.note {
            println!("{}: {note}", build.id);
        }
        let artifacts: Vec<_> = build.files.iter().map(ModelFile::artifact).collect();
        let installed =
            block_on(Installer.install(&build.id, &artifacts, &host, &|_| {}, &Cancel::new()))
                .unwrap_or_else(|error| panic!("{}: not installed: {}", build.id, error.code));
        let started = std::time::Instant::now();
        let library = ready(SherpaOnnx.open(&installed)).expect("the linked library");
        let mut model = ready(library.load(build, Accelerator::Cpu, &installed))
            .unwrap_or_else(|error| panic!("{}: not loaded: {}", build.id, error.code));
        let loading = started.elapsed().as_secs_f32();
        let stt = model.as_stt().expect("speech to text");
        let told = entry.clips.iter().map(|clip| (clip, true));
        for (language, tell) in told.chain(entry.detect.iter().map(|clip| (clip, false))) {
            let clip = &fixtures.clips[language];
            let audio = wav_16k_mono(
                &fs::read(downloaded(&cache, &clip.url, &clip.sha256)).expect("the clip"),
            );
            let started = std::time::Instant::now();
            let told = tell.then_some(language.as_str());
            let heard = ready(stt.transcribe(&audio, told)).map_err(|error| error.code);
            let seconds = started.elapsed().as_secs_f32();
            let heard = heard.unwrap_or_else(|code| format!("<{code}>"));
            let rate = wer(&clip.text, &heard);
            let clip_name = if tell {
                language.clone()
            } else {
                format!("{language}, detected")
            };
            println!(
                "| {} | {clip_name} | {rate:.2} | {heard} | {loading:.1} | {seconds:.1} |",
                build.id
            );
            if rate > fixtures.max_wer {
                failed.push(format!(
                    "{} {clip_name}: WER {rate:.2} > {}",
                    build.id, fixtures.max_wer
                ));
            }
        }
        drop(model);
        drop(library);
        block_on(Installer.uninstall(&build.id, &artifacts, crate::host::Host::storage(&host)))
            .expect("uninstalled");
    }
    let _ = fs::remove_dir_all(&data);
    for clip in fixtures.clips.values() {
        assert!(
            !clip.source.is_empty() && !clip.license.is_empty(),
            "{}: its source and licence",
            clip.url
        );
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The voice `id`, declaring no language: what these tests speak with.
fn voice(id: &str) -> Voice {
    Voice {
        id: id.to_owned(),
        languages: Vec::new(),
        gender: None,
    }
}
