//! The voice loop for real, through the engine's public API only, as an app runs it: `NativeHost`, the bundled
//! catalogue, `Engine::models` (the build the plan names, which must run here), `Engine::load`, then the loaded model's `as_tts`
//! (`voices`, `speak`) and `as_stt` (`transcribe`).
//!
//! What it runs is data, `tests/voice_loop.json`, which names catalogue builds, so one model can be heard on several
//! backends (Whisper on sherpa-onnx and on whisper.cpp): each text-to-speech build says its language's sentence with the voice
//! the plan names, and each speech-to-text build the plan pairs with that language transcribes it, at the speech's own
//! rate (the engine resamples it); then each real recorded clip (downloaded once through the host, checked against its
//! sha256) is transcribed by the same models. Every transcript is printed and compared with what was said by its
//! normalised word error rate (`voice_loop/wer.rs`); the test fails if any is above the plan's one `max_wer`, or if
//! anything fails to install, load, speak or transcribe. Each voice activity detector of the plan then hears each clip
//! between two stretches of silence, through a stream fed 20 ms at a time, and must find its speech there and nowhere
//! else (`voice_loop/vad.rs`). Each end-of-turn model hears each clip whole and cut mid-phrase, each followed
//! by a short pause, and must call the first a complete turn and the second not (`voice_loop/end_of_turn.rs`).
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
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::{env, fs};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use sidevoice_engine::{
    Accelerator, BundledCatalog, Cancel, Engine, Host, LoadedModel, NativeHost, Progress, Vad,
    VadEvent, VadOptions,
};

#[path = "voice_loop/audio.rs"]
mod audio;
#[path = "voice_loop/end_of_turn.rs"]
mod end_of_turn;
#[path = "voice_loop/tests.rs"]
mod tests;
#[path = "voice_loop/vad.rs"]
mod vad;
#[path = "voice_loop/wer.rs"]
mod wer;

type Result<T> = std::result::Result<T, String>;

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
    /// The text-to-speech builds, each with its voice per language it is run in.
    tts: Vec<Speaker>,
    /// The speech-to-text builds that hear each language, by primary language subtag.
    stt: BTreeMap<String, Vec<String>>,
    /// Real recordings.
    clips: Vec<Clip>,
    /// The voice activity detectors that hear the clips, and how they are judged.
    vad: vad::VadPlan,
    /// The end-of-turn models that hear the clips, whole and cut, and how they are judged.
    end_of_turn: end_of_turn::EndOfTurnPlan,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Speaker {
    /// A catalogue build id.
    build: String,
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

/// One clip heard by one voice activity detector: where the clip lies in what it heard, the speech it found, in seconds,
/// and the verdict.
struct Detection {
    pair: String,
    clip: Range<f64>,
    segments: Vec<Range<f64>>,
    verdict: Result<()>,
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
                .ok_or(format!("voice_loop.json: no sentence in {language}"))?;
            let voice = voice
                .clone()
                .or_else(|| voices.first().cloned())
                .unwrap_or_default();
            let what = format!("{} ({voice}, {tag})", speaker.build);
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
            let name = format!("{}-{tag}.wav", speaker.build.replace('/', "-"));
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
    let mut recorded = Vec::new();
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
        recorded.push((what, samples, rate));
    }
    let mut detections = Vec::new();
    for build in &plan.vad.builds {
        let loaded = load(&engine, build, &mut loaded_on)?;
        let vad = loaded
            .as_vad()
            .ok_or(format!("{build}: not a voice activity detector"))?;
        for (what, samples, rate) in &recorded {
            detections.push(detect(&vad, build, what, (samples, *rate), &plan.vad));
        }
    }
    let mut turns = Vec::new();
    for build in &plan.end_of_turn.builds {
        let loaded = load(&engine, build, &mut loaded_on)?;
        let model = loaded
            .as_end_of_turn()
            .ok_or(format!("{build}: not an end-of-turn model"))?;
        for (what, samples, rate) in &recorded {
            let (whole, cut) = end_of_turn::heard(samples, *rate, &plan.end_of_turn);
            let probability = |audio: &[f32]| {
                block_on(model.probability(audio, *rate)).map_err(|e| format!("`{}`", e.code))
            };
            let (whole, cut) = (probability(&whole), probability(&cut));
            let verdict = match (&whole, &cut) {
                (Ok(whole), Ok(cut)) => end_of_turn::judge(*whole, *cut, &plan.end_of_turn),
                (Err(why), _) | (_, Err(why)) => Err(why.clone()),
            };
            let pair = format!("{what} → {build}");
            println!("{pair}: whole {whole:?}, cut {cut:?}: {verdict:?}");
            turns.push(end_of_turn::Turn {
                pair,
                whole: whole.ok(),
                cut: cut.ok(),
                verdict,
            });
        }
    }
    report(
        &rows,
        plan.max_wer,
        &detections,
        &turns,
        &loaded_on,
        &dir.join("summary.md"),
    )
}

/// What `vad` finds in the clip `what` (its samples and their rate), set between silences and fed 20 ms at a time,
/// judged by the plan's rule.
fn detect(
    vad: &Vad<'_>,
    build: &str,
    what: &str,
    (samples, rate): (&[f32], u32),
    plan: &vad::VadPlan,
) -> Detection {
    let pair = format!("{what} → {build}");
    let heard = (|| {
        let mut stream = block_on(vad.stream(VadOptions::default())).map_err(|e| e.code)?;
        let at = stream.sample_rate();
        let clip = audio::resample(samples, rate, at);
        let (padded, span) = vad::padded(&clip, at, plan.silence_s);
        let mut events = Vec::new();
        for piece in padded.chunks((at / 50) as usize) {
            events.extend(block_on(stream.accept(piece)).map_err(|e| e.code)?.events);
        }
        let finished = stream.finish();
        let starts = events
            .iter()
            .filter(|event| matches!(event, VadEvent::SpeechStart { .. }))
            .count();
        let seconds = |sample: u64| sample as f64 / f64::from(at);
        let segments: Vec<Range<f64>> = events
            .iter()
            .chain(&finished)
            .filter_map(|event| match event {
                VadEvent::SpeechEnd { start, end } => Some(seconds(*start)..seconds(*end)),
                _ => None,
            })
            .collect();
        Ok::<_, &str>((span, segments, finished.is_some(), starts))
    })();
    let (clip, segments, verdict) = match heard {
        Ok((span, segments, finished, starts)) => {
            let verdict = if starts == segments.len() {
                vad::judge(&segments, finished, &span, plan)
            } else {
                Err(format!(
                    "{starts} speech starts for {} ends",
                    segments.len()
                ))
            };
            (span, segments, verdict)
        }
        Err(code) => (0.0..0.0, Vec::new(), Err(format!("`{code}`"))),
    };
    let found: Vec<String> = segments
        .iter()
        .map(|segment| format!("{:.2}–{:.2}", segment.start, segment.end))
        .collect();
    println!(
        "{pair}: clip at {:.2}–{:.2} s, speech at {}: {}",
        clip.start,
        clip.end,
        found.join(", "),
        verdict.as_ref().map_or_else(Clone::clone, |()| "ok".into())
    );
    Detection {
        pair,
        clip,
        segments,
        verdict,
    }
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

/// Prints the tables (transcripts, detections, turns) and the accelerator each build was loaded on, writes them to
/// `summary` with a heading for this platform, and fails if any row failed or is above `max_wer`, or any detection or
/// turn failed.
fn report(
    rows: &[Row],
    max_wer: f64,
    detections: &[Detection],
    turns: &[end_of_turn::Turn],
    loaded_on: &BTreeMap<String, Option<Accelerator>>,
    summary: &Path,
) -> Result<()> {
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
    let (detected, missed) = vad::table(detections.iter().map(|detection| {
        (
            detection.pair.as_str(),
            &detection.clip,
            &detection.segments[..],
            &detection.verdict,
        )
    }));
    let (ended, unended) = end_of_turn::table(turns);
    let mut builds = String::from("Builds, and the accelerator the engine loaded each on:\n\n");
    for (build, accelerator) in loaded_on {
        let on =
            accelerator.map_or_else(|| "?".to_owned(), |accelerator| format!("{accelerator:?}"));
        builds.push_str(&format!("- `{build}`: {on}\n"));
    }
    println!("\n{table}\n{verdict}\n\n{detected}\n{ended}\n{builds}");
    let heading = format!("## Voice loop ({} {})", env::consts::OS, env::consts::ARCH);
    write(
        summary,
        format!("{heading}\n\n{table}\n{verdict}\n\n{detected}\n{ended}\n{builds}").as_bytes(),
    )?;
    if bad > 0 || rows.is_empty() {
        return Err(format!("{bad} of {} comparisons failed", rows.len()));
    }
    if missed > 0 || detections.is_empty() {
        return Err(format!(
            "{missed} of {} detections failed",
            detections.len()
        ));
    }
    if unended > 0 || turns.is_empty() {
        return Err(format!("{unended} of {} turns failed", turns.len()));
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
