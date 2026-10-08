//! `cargo xtask e2e [DIR]`: the voice loop for real, through the engine's public API, as an app would run it: the
//! bundled catalogue, `Engine::models` (the build the plan names, which must run here), `Engine::load` (installing
//! through `NativeHost`, keeping everything in `DIR/engine`), then the loaded model's `speak` and `transcribe`.
//!
//! What it runs is data, `xtask/e2e.json`, which names catalogue builds, so one model can be heard on several backends
//! (Whisper on sherpa-onnx and on whisper.cpp): each text-to-speech build says its language's sentence with the voice the
//! plan names, and each speech-to-text build the plan pairs with that language transcribes it, at the speech's own
//! rate (the engine resamples it); then each real recorded clip (downloaded once into `DIR/clips`, checked against its sha256) is
//! transcribed by the same builds. Every transcript is printed and compared with what was said by its normalised word
//! error rate (`e2e/wer.rs`); the run fails if any is above the plan's `max_wer` (or a voice's own, given with its
//! `why`), or if anything fails to install, load, speak or transcribe. The table also goes to `$GITHUB_STEP_SUMMARY`
//! when it is set, with the accelerator each build was loaded on, and what each model said is kept as a WAV in
//! `DIR/speech`, to be listened to.
//!
//! DIR is `target/e2e` unless given. It downloads about 1.5 GB the first time; nothing the second.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::{env, fs};

use serde::Deserialize;
use sidevoice_engine::{
    Accelerator, BundledCatalog, Cancel, Engine, LoadedModel, NativeHost, Progress,
};

use crate::{read, repo, run_in, sha256, write, Result};

mod audio;
#[cfg(test)]
mod tests;
mod wer;

/// `xtask/e2e.json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    /// The highest word error rate a transcript may have.
    max_wer: f64,
    /// What is said, by primary language subtag (`en`, `es`).
    sentences: BTreeMap<String, String>,
    /// The text-to-speech builds, each with its voice per language it is run in.
    tts: Vec<Speaker>,
    /// The speech-to-text builds that hear each language, by primary language subtag.
    stt: BTreeMap<String, Vec<String>>,
    /// Real recordings.
    clips: Vec<Clip>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Speaker {
    /// A catalogue build id.
    build: String,
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
        .map_err(|e| format!("the engine: {e:?}"))?;

    let mut rows = Vec::new();
    let mut loaded_on = BTreeMap::new();
    let mut listeners: BTreeMap<String, LoadedModel> = BTreeMap::new();
    for build in plan.stt.values().flatten() {
        if !listeners.contains_key(build) {
            listeners.insert(build.clone(), load(&engine, build, &mut loaded_on)?);
        }
    }
    for speaker in &plan.tts {
        let loaded = load(&engine, &speaker.build, &mut loaded_on)?;
        let tts = loaded
            .as_tts()
            .ok_or(format!("{}: not a text-to-speech model", speaker.build))?;
        let voices: Vec<String> = block_on(tts.voices())
            .into_iter()
            .map(|voice| voice.id)
            .collect();
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
            let what = format!("{} ({voice}, {tag})", speaker.build);
            println!("{what} says {said:?}");
            let speech = block_on(tts.speak(said, &voice, Some(tag), None));
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
            let name = format!("{}-{tag}.wav", speaker.build.replace('/', "-"));
            write(
                &dir.join("speech").join(name),
                &audio::wav(&speech.samples, speech.sample_rate),
            )?;
            let audio = (&speech.samples[..], speech.sample_rate);
            for listener in plan.stt.get(&language).into_iter().flatten() {
                rows.push(Row {
                    max_wer: speaker.max_wer,
                    ..hear(&listeners, listener, &what, &language, said, audio)
                });
            }
        }
    }
    for clip in &plan.clips {
        let (samples, rate) = audio::read_wav(&fetch(&dir.join("clips"), clip)?)
            .map_err(|e| format!("{}: {e}", clip.url))?;
        let what = format!("clip {}", clip.url.rsplit('/').next().unwrap_or_default());
        println!(
            "{what} ({}, {}): {:?}",
            clip.source, clip.license, clip.text
        );
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
    report(&rows, plan.max_wer, &loaded_on)
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
                speaker.build
            ));
        }
    }
    Ok(plan)
}

/// The build `build`, loaded: installed first if it is not. It must run here; the accelerator the engine chose for it
/// goes into `loaded_on`.
fn load(
    engine: &Engine,
    build: &str,
    loaded_on: &mut BTreeMap<String, Option<Accelerator>>,
) -> Result<LoadedModel> {
    let models = block_on(engine.models()).map_err(|e| format!("the models: {e}"))?;
    let (model, entry) = models
        .iter()
        .find_map(|model| {
            let entry = model.builds.iter().find(|entry| entry.id == build)?;
            Some((model, entry))
        })
        .ok_or(format!("{build}: not in the catalogue"))?;
    if !entry.available {
        return Err(format!("{build}: does not run here: {:?}", entry.reasons));
    }
    println!("loading {build} on {:?}", entry.accelerator);
    loaded_on.insert(build.to_owned(), entry.accelerator);
    let progress = |progress: Progress| {
        if progress.received == 0 && progress.done < progress.files {
            println!("  file {} of {}", progress.done + 1, progress.files);
        }
    };
    block_on(engine.load(&model.id, Some(build), &progress, &Cancel::new()))
        .map_err(|e| format!("{build}: {e}"))
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

/// Prints the table and the accelerator each build was loaded on, writes them to `$GITHUB_STEP_SUMMARY` when set, and
/// fails if any row failed or is above its highest word error rate: its own, or `default`.
fn report(
    rows: &[Row],
    default: f64,
    loaded_on: &BTreeMap<String, Option<Accelerator>>,
) -> Result<()> {
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
    let mut builds = String::from("Builds, and the accelerator the engine loaded each on:\n\n");
    for (build, accelerator) in loaded_on {
        let on =
            accelerator.map_or_else(|| "?".to_owned(), |accelerator| format!("{accelerator:?}"));
        builds.push_str(&format!("- `{build}`: {on}\n"));
    }
    println!("\n{table}\n{verdict}\n\n{builds}");
    if let Some(summary) = env::var_os("GITHUB_STEP_SUMMARY") {
        let mut text =
            String::from_utf8_lossy(&fs::read(&summary).unwrap_or_default()).into_owned();
        text.push_str(&format!(
            "## Voice loop ({} {})\n\n{table}\n{verdict}\n\n{builds}\n",
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
