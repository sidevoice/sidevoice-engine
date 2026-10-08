//! `cargo xtask e2e [DIR]`: the voice loop for real, through the engine's public API, as an app would run it: the
//! bundled catalogue, `select` (on sherpa-onnx), `prepare` (the installer and `NativeHost`, keeping everything in
//! `DIR/engine`), then `speak` and `transcribe`.
//!
//! What it runs is data, `xtask/e2e.json`: each text-to-speech model says its language's sentence with the voice the
//! plan names, the speech is brought to 16 kHz and each speech-to-text model the plan pairs with that language
//! transcribes it; then each real recorded clip (downloaded once into `DIR/clips`, checked against its sha256) is
//! transcribed by the same models. Every transcript is printed and compared with what was said by its normalised word
//! error rate (`e2e/wer.rs`); the run fails if any is above the plan's `max_wer` (or a voice's own, given with its
//! `why`), or if anything fails to install, load, speak or transcribe. The table also goes to `$GITHUB_STEP_SUMMARY` when it is set, and what each model said is kept
//! as a WAV in `DIR/speech`, to be listened to.
//!
//! DIR is `target/e2e` unless given. It downloads about 1.5 GB the first time; nothing the second.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;
use std::{env, fs};

use serde::Deserialize;
use sidevoice_engine::{
    BundledCatalog, Cancel, Capability, Engine, Handle, NativeHost, Preferences, Progress,
};

use crate::{read, repo, run_in, sha256, write, Result};

mod audio;
#[cfg(test)]
mod tests;
mod wer;

/// The backend the loop runs on: the one that runs every model in the plan on every platform it covers.
const BACKEND: &str = "sherpa-onnx";

/// `xtask/e2e.json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    /// The highest word error rate a transcript may have.
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
    /// A higher word error rate this voice's speech may have than the plan's, and `why`, which the plan must give.
    #[serde(default)]
    max_wer: Option<f64>,
    #[serde(default)]
    why: Option<String>,
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
    source: String,
    license: String,
}

/// One comparison: who said it to whom, in which language, what was said and what was heard.
struct Row {
    pair: String,
    language: String,
    said: String,
    heard: Result<String>,
    /// The highest word error rate it may have, when it is not the plan's.
    max_wer: Option<f64>,
}

/// `cargo xtask e2e [DIR]`.
pub(crate) fn run(dir: Option<&str>) -> Result<()> {
    let plan = plan(&String::from_utf8_lossy(&read(
        &repo().join("xtask/e2e.json"),
    )?))?;
    let dir = dir.map_or_else(|| repo().join("target/e2e"), PathBuf::from);
    for sub in ["engine", "clips", "speech"] {
        fs::create_dir_all(dir.join(sub)).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let host = NativeHost::new(dir.join("engine")).map_err(|e| format!("NativeHost: {e}"))?;
    let engine = Engine::new(Box::new(host), vec![Box::new(BundledCatalog)])
        .map_err(|e| format!("the engine: {e:?}"))?
        // The loop uses each model on and off for longer than an app's idle time.
        .with_idle_unload(Duration::from_secs(24 * 60 * 60));

    let mut rows = Vec::new();
    let mut listeners: BTreeMap<String, Handle> = BTreeMap::new();
    for model in plan.stt.values().flatten() {
        if !listeners.contains_key(model) {
            listeners.insert(model.clone(), prepare(&engine, Capability::Stt, model)?);
        }
    }
    for speaker in &plan.tts {
        let handle = prepare(&engine, Capability::Tts, &speaker.model)?;
        let voices = engine
            .voices(handle)
            .map_err(|e| format!("{}: {e}", speaker.model))?;
        for (tag, voice) in &speaker.voices {
            let language = primary(tag);
            let said = plan
                .sentences
                .get(&language)
                .ok_or(format!("e2e.json: no sentence in {language}"))?;
            let voice = voice
                .clone()
                .or_else(|| voices.first().cloned())
                .unwrap_or_default();
            let what = format!("{} ({voice}, {tag})", speaker.model);
            println!("{what} says {said:?}");
            let speech = block_on(engine.speak(handle, said, &voice, Some(tag), 1.0));
            let speech = match speech {
                Ok(speech) => speech,
                Err(error) => {
                    rows.push(Row {
                        max_wer: speaker.max_wer,
                        ..failed(&what, &language, said, error.code)
                    });
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
            let pcm = audio::to_stt_rate(&speech.samples, speech.sample_rate);
            for listener in plan.stt.get(&language).into_iter().flatten() {
                rows.push(Row {
                    max_wer: speaker.max_wer,
                    ..hear(&engine, &listeners, listener, &what, &language, said, &pcm)
                });
            }
        }
    }
    for clip in &plan.clips {
        let (samples, rate) = audio::read_wav(&fetch(&dir.join("clips"), clip)?)
            .map_err(|e| format!("{}: {e}", clip.url))?;
        let pcm = audio::to_stt_rate(&samples, rate);
        let what = format!("clip {}", clip.url.rsplit('/').next().unwrap_or_default());
        println!(
            "{what} ({}, {}): {:?}",
            clip.source, clip.license, clip.text
        );
        for listener in plan.stt.get(&clip.language).into_iter().flatten() {
            rows.push(hear(
                &engine,
                &listeners,
                listener,
                &what,
                &clip.language,
                &clip.text,
                &pcm,
            ));
        }
    }
    report(&rows, plan.max_wer)
}

/// The plan in `json`, read strictly, and consistent: a sentence for every language spoken.
fn plan(json: &str) -> Result<Plan> {
    let plan: Plan = serde_json::from_str(json).map_err(|e| format!("e2e.json: {e}"))?;
    let spoken = plan
        .tts
        .iter()
        .flat_map(|speaker| speaker.voices.keys().map(|tag| primary(tag)));
    for language in spoken {
        if !plan.sentences.contains_key(&language) {
            return Err(format!("e2e.json: no sentence in {language}"));
        }
    }
    for speaker in &plan.tts {
        if speaker.max_wer.is_some() != speaker.why.is_some() {
            return Err(format!(
                "e2e.json: {}: max_wer and why go together",
                speaker.model
            ));
        }
    }
    Ok(plan)
}

/// `model` selected on [`BACKEND`] and prepared: installed if it is not, and loaded.
fn prepare(engine: &Engine, capability: Capability, model: &str) -> Result<Handle> {
    let preferences = Preferences {
        model: Some(model.to_owned()),
        backend: Some(BACKEND.to_owned()),
        ..Preferences::default()
    };
    let Some(selection) = engine.select(capability, &preferences) else {
        let why: Vec<String> = engine
            .offers(capability)
            .into_iter()
            .map(|offer| format!("{offer:?}"))
            .filter(|offer| offer.contains(model))
            .collect();
        return Err(format!("{model}: not offered on {BACKEND} here: {why:?}"));
    };
    println!(
        "preparing {} on {:?}",
        selection.build.id, selection.accelerator
    );
    let progress = |progress: Progress| {
        if progress.received == 0 && progress.done < progress.files {
            println!("  file {} of {}", progress.done + 1, progress.files);
        }
    };
    block_on(engine.prepare(&selection, &progress, &Cancel::new()))
        .map_err(|e| format!("{}: {e}", selection.build.id))
}

/// What `listener` hears in `pcm`, told the language.
fn hear(
    engine: &Engine,
    listeners: &BTreeMap<String, Handle>,
    listener: &str,
    speaker: &str,
    language: &str,
    said: &str,
    pcm: &[f32],
) -> Row {
    let heard = listeners
        .get(listener)
        .ok_or(format!("{listener}: not prepared"))
        .and_then(|handle| {
            block_on(engine.transcribe(*handle, pcm, Some(language))).map_err(|e| e.code.to_owned())
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
        max_wer: None,
    }
}

fn failed(pair: &str, language: &str, said: &str, code: &str) -> Row {
    println!("  failed: {code}");
    Row {
        pair: pair.to_owned(),
        language: language.to_owned(),
        said: said.to_owned(),
        heard: Err(code.to_owned()),
        max_wer: None,
    }
}

/// The clip, from `clips` or downloaded into it, checked against its digest either way.
fn fetch(clips: &Path, clip: &Clip) -> Result<Vec<u8>> {
    let path = clips.join(&clip.sha256);
    if !path.is_file() {
        let partial = clips.join(format!("{}.partial", clip.sha256));
        let target = partial.to_string_lossy();
        run_in(clips, "curl -fsSL --retry 3 -o", &[&target, &clip.url])?;
        fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let bytes = read(&path)?;
    let digest = sha256(&bytes);
    if digest != clip.sha256 {
        let _ = fs::remove_file(&path);
        return Err(format!(
            "{}: sha256 {digest}, not {}",
            clip.url, clip.sha256
        ));
    }
    Ok(bytes)
}

/// Prints the table, writes it to `$GITHUB_STEP_SUMMARY` when set, and fails if any row failed or is above its highest
/// word error rate: its own, or `default`.
fn report(rows: &[Row], default: f64) -> Result<()> {
    let mut table =
        String::from("| Model pair | Language | Expected | Got | WER |\n|---|---|---|---|---|\n");
    let mut bad = 0;
    for row in rows {
        let max_wer = row.max_wer.unwrap_or(default);
        let (got, wer, ok) = match &row.heard {
            Ok(heard) => {
                let wer = wer::wer(&row.said, heard);
                let own = match row.max_wer {
                    Some(own) => format!(" (≤ {:.0}%)", own * 100.0),
                    None => String::new(),
                };
                let mark = if wer <= max_wer {
                    own
                } else {
                    format!("{own} ✗")
                };
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
        "{} of {} within {:.0}% WER, or the voice's own limit where the plan gives one (normalised: lower case, no \
         punctuation, vowel accents folded).",
        rows.len() - bad,
        rows.len(),
        default * 100.0
    );
    println!("\n{table}\n{verdict}");
    if let Some(summary) = env::var_os("GITHUB_STEP_SUMMARY") {
        let mut text =
            String::from_utf8_lossy(&fs::read(&summary).unwrap_or_default()).into_owned();
        text.push_str(&format!(
            "## Voice loop ({} {})\n\n{table}\n{verdict}\n",
            env::consts::OS,
            env::consts::ARCH
        ));
        write(Path::new(&summary), text.as_bytes())?;
    }
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

/// Runs `future` to its end on this thread, parking it while it waits (the engine needs no particular runtime).
fn block_on<F: Future>(future: F) -> F::Output {
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::park();
    }
}
