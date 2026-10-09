//! The voice loop for real, through the engine's public API only, as an app runs it: `NativeHost`, the bundled
//! catalogue, `Engine::models` (each model's sherpa-onnx build), `Engine::load`, then the loaded model's `as_tts`
//! (`voices`, `speak`) and `as_stt` (`transcribe`).
//!
//! What it runs is data, `tests/voice_loop.json`: each text-to-speech model says its language's sentence with the voice
//! the plan names, and each speech-to-text model the plan pairs with that language transcribes it, at the speech's own
//! rate (the engine resamples it); then each real recorded clip (downloaded once through the host, checked against its
//! sha256) is transcribed by the same models. Every transcript is printed and compared with what was said by its
//! normalised word error rate (`voice_loop/wer.rs`); the test fails if any is above the plan's one `max_wer`, or if
//! anything fails to install, load, speak or transcribe.
//!
//! It downloads about 1.5 GB the first time, so it is ignored unless asked for; the `e2e` workflow asks, on each native
//! platform, through `cargo xtask e2e`:
//!
//! ```sh
//! cargo test --locked --test voice_loop -- --ignored --nocapture
//! ```
//!
//! Everything is kept in `$SIDEVOICE_VOICE_LOOP` (Cargo's temporary directory for tests unless set): `engine/` (the
//! host's storage), `clips/`, `speech/` (what each model said, as WAV files, to be listened to) and `summary.md` (the
//! table, for the job's summary).

// NativeHost, and the models it runs, are native only.
#![cfg(native)]

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::{env, fs};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use sidevoice_engine::{BundledCatalog, Cancel, Engine, Host, LoadedModel, NativeHost, Progress};

#[path = "voice_loop/audio.rs"]
mod audio;
#[path = "voice_loop/tests.rs"]
mod tests;
#[path = "voice_loop/wer.rs"]
mod wer;

type Result<T> = std::result::Result<T, String>;

/// The backend the loop runs on: the one that runs every model in the plan on every platform it covers.
const BACKEND: &str = "sherpa-onnx";

/// `tests/voice_loop.json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    /// The highest word error rate any transcript may have, one for every row. The loop checks that the circuit works end
    /// to end and catches a wrong configuration (a wrong language or voice, a model that hears nothing): it does not
    /// measure quality, so the limit is loose and no row has its own.
    max_wer: f64,
    /// What is said, by primary language subtag (`en`, `es`).
    sentences: BTreeMap<String, String>,
    /// The text-to-speech models, each with its voice per language it is run in.
    tts: Vec<Speaker>,
    /// The speech-to-text models that hear each language, by primary language subtag.
    stt: BTreeMap<String, Vec<String>>,
    /// Real recordings.
    clips: Vec<Clip>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Speaker {
    model: String,
    /// By the BCP 47 tag the model is told: the voice to speak with, or `null` for the model's first.
    voices: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Clip {
    /// Its primary language subtag.
    language: String,
    url: String,
    sha256: String,
    /// What is said in it, as its source transcribes it.
    text: String,
    /// Where it comes from, and its licence: for people, not read.
    #[allow(dead_code, reason = "for people reading the plan")]
    source: String,
    #[allow(dead_code, reason = "for people reading the plan")]
    license: String,
}

/// One comparison: who said it to whom, in which language, what was said and what was heard.
struct Row {
    pair: String,
    language: String,
    said: String,
    heard: Result<String>,
}

#[test]
#[ignore = "downloads about 1.5 GB of models: the e2e workflow runs it (cargo xtask e2e)"]
fn the_voice_loop_stays_within_its_word_error_rates() {
    if let Err(error) = run() {
        panic!("{error}");
    }
}

fn run() -> Result<()> {
    let plan =
        plan(&fs::read_to_string(plan_path()).map_err(|e| format!("voice_loop.json: {e}"))?)?;
    let dir = env::var_os("SIDEVOICE_VOICE_LOOP").map_or_else(
        || Path::new(env!("CARGO_TARGET_TMPDIR")).join("voice-loop"),
        PathBuf::from,
    );
    for sub in ["engine", "clips", "speech"] {
        fs::create_dir_all(dir.join(sub)).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let host = NativeHost::new(dir.join("engine")).map_err(|e| format!("NativeHost: {e}"))?;
    let engine = Engine::new(Box::new(host), vec![Box::new(BundledCatalog)])
        .map_err(|e| format!("the engine: {e:?}"))?;
    // A second host on the same directory, for the clips: only its fetcher is used.
    let fetcher = NativeHost::new(dir.join("engine")).map_err(|e| format!("NativeHost: {e}"))?;

    let mut rows = Vec::new();
    let mut listeners: BTreeMap<String, LoadedModel> = BTreeMap::new();
    for model in plan.stt.values().flatten() {
        if !listeners.contains_key(model) {
            listeners.insert(model.clone(), load(&engine, model)?);
        }
    }
    for speaker in &plan.tts {
        let loaded = load(&engine, &speaker.model)?;
        let tts = loaded
            .as_tts()
            .ok_or(format!("{}: not a text-to-speech model", speaker.model))?;
        let voices: Vec<String> = block_on(tts.voices())
            .into_iter()
            .map(|voice| voice.id)
            .collect();
        for (tag, voice) in &speaker.voices {
            let language = primary(tag);
            let said = plan
                .sentences
                .get(&language)
                .ok_or(format!("voice_loop.json: no sentence in {language}"))?;
            let voice = voice
                .clone()
                .or_else(|| voices.first().cloned())
                .unwrap_or_default();
            let what = format!("{} ({voice}, {tag})", speaker.model);
            println!("{what} says {said:?}");
            let speech = match block_on(tts.speak(said, &voice, Some(tag), None)) {
                Ok(speech) => speech,
                Err(error) => {
                    rows.push(failed(&what, &language, said, error.code));
                    continue;
                }
            };
            let seconds = speech.samples.len() as f64 / f64::from(speech.sample_rate.max(1));
            println!("  {seconds:.2} s at {} Hz", speech.sample_rate);
            let name = format!("{}-{tag}.wav", speaker.model);
            write(
                &dir.join("speech").join(name),
                &audio::wav(&speech.samples, speech.sample_rate),
            )?;
            let audio = (&speech.samples[..], speech.sample_rate);
            for listener in plan.stt.get(&language).into_iter().flatten() {
                rows.push(hear(&listeners, listener, &what, &language, said, audio));
            }
        }
    }
    for clip in &plan.clips {
        let bytes = fetch(&fetcher, &dir.join("clips"), clip)?;
        let (samples, rate) = audio::read_wav(&bytes).map_err(|e| format!("{}: {e}", clip.url))?;
        let what = format!("clip {}", clip.url.rsplit('/').next().unwrap_or_default());
        println!("{what}: {:?}", clip.text);
        for listener in plan.stt.get(&clip.language).into_iter().flatten() {
            let audio = (&samples[..], rate);
            rows.push(hear(
                &listeners,
                listener,
                &what,
                &clip.language,
                &clip.text,
                audio,
            ));
        }
    }
    report(&rows, plan.max_wer, &dir.join("summary.md"))
}

fn plan_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/voice_loop.json")
}

/// The plan in `json`, read strictly, and consistent: a sentence for every language spoken.
fn plan(json: &str) -> Result<Plan> {
    let plan: Plan = serde_json::from_str(json).map_err(|e| format!("voice_loop.json: {e}"))?;
    let spoken = plan
        .tts
        .iter()
        .flat_map(|speaker| speaker.voices.keys().map(|tag| primary(tag)));
    for language in spoken {
        if !plan.sentences.contains_key(&language) {
            return Err(format!("voice_loop.json: no sentence in {language}"));
        }
    }
    Ok(plan)
}

/// `model`'s build on [`BACKEND`], loaded: installed first if it is not.
fn load(engine: &Engine, model: &str) -> Result<LoadedModel> {
    let models = block_on(engine.models()).map_err(|e| format!("the models: {e}"))?;
    let entry = models
        .iter()
        .find(|entry| entry.id == model)
        .ok_or(format!("{model}: not in the catalogue"))?;
    let build = entry
        .builds
        .iter()
        .find(|build| build.backend == BACKEND)
        .ok_or(format!("{model}: no {BACKEND} build"))?;
    if !build.available {
        return Err(format!(
            "{}: does not run here: {:?}",
            build.id, build.reasons
        ));
    }
    println!("loading {} on {:?}", build.id, build.accelerator);
    let progress = |progress: Progress| {
        if progress.received == 0 && progress.done < progress.files {
            println!("  file {} of {}", progress.done + 1, progress.files);
        }
    };
    block_on(engine.load(model, Some(&build.id), &progress, &Cancel::new()))
        .map_err(|e| format!("{}: {e}", build.id))
}

/// What `listener` hears in `audio` (its samples and their rate), told the language.
fn hear(
    listeners: &BTreeMap<String, LoadedModel>,
    listener: &str,
    speaker: &str,
    language: &str,
    said: &str,
    (samples, rate): (&[f32], u32),
) -> Row {
    let heard = listeners
        .get(listener)
        .and_then(LoadedModel::as_stt)
        .ok_or(format!("{listener}: not loaded as speech to text"))
        .and_then(|stt| {
            block_on(stt.transcribe(samples, rate, Some(language))).map_err(|e| e.code.to_owned())
        });
    match &heard {
        Ok(text) => println!("  {listener} heard {text:?}"),
        Err(code) => println!("  {listener} failed: {code}"),
    }
    Row {
        pair: format!("{speaker} → {listener}"),
        language: language.to_owned(),
        said: said.to_owned(),
        heard,
    }
}

fn failed(pair: &str, language: &str, said: &str, code: &str) -> Row {
    println!("  failed: {code}");
    Row {
        pair: pair.to_owned(),
        language: language.to_owned(),
        said: said.to_owned(),
        heard: Err(code.to_owned()),
    }
}

/// The clip, from `clips` or downloaded into it through the host's fetcher, checked against its digest either way.
fn fetch(host: &NativeHost, clips: &Path, clip: &Clip) -> Result<Vec<u8>> {
    let path = clips.join(&clip.sha256);
    if !path.is_file() {
        let bytes = block_on(async {
            let mut download = host.fetcher().fetch(&clip.url).await?;
            let mut bytes = Vec::new();
            while let Some(chunk) = download.chunk().await? {
                bytes.extend(chunk);
            }
            Ok::<_, sidevoice_engine::Error>(bytes)
        })
        .map_err(|e| format!("{}: {e}", clip.url))?;
        write(&path, &bytes)?;
    }
    let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest != clip.sha256 {
        let _ = fs::remove_file(&path);
        return Err(format!(
            "{}: sha256 {digest}, not {}",
            clip.url, clip.sha256
        ));
    }
    Ok(bytes)
}

/// Prints the table, writes it to `summary` with a heading for this platform, and fails if any row failed or is above
/// `max_wer`.
fn report(rows: &[Row], max_wer: f64, summary: &Path) -> Result<()> {
    let mut table =
        String::from("| Model pair | Language | Expected | Got | WER |\n|---|---|---|---|---|\n");
    let mut bad = 0;
    for row in rows {
        let (got, wer, ok) = match &row.heard {
            Ok(heard) => {
                let wer = wer::wer(&row.said, heard);
                let mark = if wer <= max_wer { "" } else { " ✗" };
                (
                    heard.clone(),
                    format!("{:.0}%{mark}", wer * 100.0),
                    wer <= max_wer,
                )
            }
            Err(code) => (format!("`{code}`"), "✗".to_owned(), false),
        };
        bad += usize::from(!ok);
        let cell = |text: &str| text.replace('|', "\\|");
        table.push_str(&format!(
            "| {} | {} | {} | {} | {wer} |\n",
            cell(&row.pair),
            row.language,
            cell(&row.said),
            cell(&got)
        ));
    }
    let verdict = format!(
        "{} of {} within {:.0}% WER (normalised: lower case, no punctuation, vowel accents folded).",
        rows.len() - bad,
        rows.len(),
        max_wer * 100.0
    );
    println!("\n{table}\n{verdict}");
    let heading = format!("## Voice loop ({} {})", env::consts::OS, env::consts::ARCH);
    write(
        summary,
        format!("{heading}\n\n{table}\n{verdict}\n").as_bytes(),
    )?;
    if bad > 0 || rows.is_empty() {
        return Err(format!("{bad} of {} comparisons failed", rows.len()));
    }
    Ok(())
}

/// The primary language subtag of the BCP 47 tag `tag`, lower-cased.
fn primary(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Runs `future` to its end on the loop's Tokio runtime, as an app does: the native engine downloads through reqwest
/// and unpacks archives on Tokio's blocking threads, so its futures need one.
fn block_on<F: Future>(future: F) -> F::Output {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("a Tokio runtime")
        })
        .block_on(future)
}
