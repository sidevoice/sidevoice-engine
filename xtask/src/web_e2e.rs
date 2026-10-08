//! `cargo xtask web-e2e [DIR]`: the voice loop for real in a headless Chrome, through the npm package as a page uses
//! it: the package built and installed into `DIR/site` as a consumer installs it (with transformers.js and eSpeak NG,
//! its dependencies), and a page (`xtask/web-e2e/page.mjs`) that installs, cancels, loads and uninstalls through
//! `WebEngine`, so the files go through the engine's own OPFS storage and `fetch` downloads, then transcribes the native
//! loop's recorded clips (`xtask/e2e.json`) with the plan's speech-to-text build and what each text-to-speech build says
//! in its language's sentence. `xtask/web-e2e/run.mjs` serves the page and drives Chrome.
//!
//! What it runs is data, `xtask/web-e2e.json`: the accelerators the page reports, the speech-to-text build, and each
//! text-to-speech build with its voice and language. Every check the page makes must pass, and every transcript must
//! stay within the plan's word error rate (or a voice's own, with its `why`), judged as the native loop judges
//! (`voice_loop.rs`). Chrome is `CHROME`, else `google-chrome` on the `PATH`. DIR is `target/web-e2e` unless given;
//! the clips are kept there by digest, and what each model said in `DIR/speech`, to be listened to. The models
//! download every run, into the browser profile's OPFS, which is thrown away.

use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::voice_loop::{fetch, primary, report, Clip, Row};
use crate::{empty_dir, npm, read, repo, run_in, write, Result};

const PAGE: &str = include_str!("../web-e2e/page.mjs");
const RUN: &str = include_str!("../web-e2e/run.mjs");

/// `xtask/web-e2e.json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    /// The highest word error rate a transcript may have.
    max_wer: f64,
    /// The accelerators the page reports: `wasm`, `webgpu`.
    accelerators: Vec<String>,
    /// What transcribes.
    stt: Build,
    /// What speaks.
    tts: Vec<Speaker>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Build {
    model: String,
    build: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Speaker {
    model: String,
    build: String,
    voice: String,
    /// The BCP 47 tag the model is told; its primary subtag picks the sentence.
    language: String,
    /// A higher word error rate this voice's speech may have than the plan's, and `why`, which the plan must give.
    #[serde(default)]
    max_wer: Option<f64>,
    #[serde(default)]
    why: Option<String>,
}

/// What the web loop takes from the native one's plan (`xtask/e2e.json`): its sentences and its recorded clips.
#[derive(Debug, Deserialize)]
struct Shared {
    sentences: BTreeMap<String, String>,
    clips: Vec<Clip>,
}

/// What the page posts.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    #[serde(default)]
    checks: Vec<Check>,
    #[serde(default)]
    rows: Vec<PageRow>,
    #[serde(default)]
    error: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct Check {
    name: String,
    ok: bool,
    detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageRow {
    pair: String,
    language: String,
    said: String,
    heard: Option<String>,
    error: Option<String>,
    max_wer: Option<f64>,
}

/// `cargo xtask web-e2e [DIR]`.
pub(crate) fn run(dir: Option<&str>) -> Result<()> {
    let plan: Plan = parse(
        &read(&repo().join("xtask/web-e2e.json"))?,
        "xtask/web-e2e.json",
    )?;
    if let Some(speaker) = plan
        .tts
        .iter()
        .find(|s| s.max_wer.is_some() && s.why.is_none())
    {
        return Err(format!(
            "{}: a max_wer of its own needs its why",
            speaker.build
        ));
    }
    let shared: Shared = parse(&read(&repo().join("xtask/e2e.json"))?, "xtask/e2e.json")?;
    let dir = dir.map_or_else(|| repo().join("target/web-e2e"), PathBuf::from);
    let (site, clips, speech) = (dir.join("site"), dir.join("clips"), dir.join("speech"));
    empty_dir(&site)?;
    empty_dir(&speech)?;
    std::fs::create_dir_all(&clips).map_err(|e| format!("{}: {e}", clips.display()))?;

    // The package, installed as a consumer installs it.
    let tarball = npm::packed()?;
    write(
        &site.join("package.json"),
        br#"{"private": true, "type": "module"}"#,
    )?;
    let tarball = tarball.to_string_lossy();
    run_in(&site, "npm install --no-audit --no-fund", &[&tarball])?;

    // The page's plan: the builds, each speaker's sentence, and the clips beside it.
    std::fs::create_dir_all(site.join("clips")).map_err(|e| format!("{}: {e}", site.display()))?;
    let mut page_clips = Vec::new();
    for clip in &shared.clips {
        let file = format!("clips/{}.wav", clip.sha256);
        write(&site.join(&file), &fetch(&clips, clip)?)?;
        let name = clip.url.rsplit('/').next().unwrap_or_default();
        page_clips.push(json!({
            "name": name,
            "language": clip.language,
            "file": file,
            "text": clip.text,
            "source": clip.source,
            "license": clip.license,
        }));
    }
    let mut speakers = Vec::new();
    for speaker in &plan.tts {
        let language = primary(&speaker.language);
        let sentence = shared
            .sentences
            .get(&language)
            .ok_or(format!("{}: no sentence in {language}", speaker.build))?;
        speakers.push(json!({
            "model": speaker.model,
            "build": speaker.build,
            "voice": speaker.voice,
            "language": speaker.language,
            "sentence": sentence,
            "max_wer": speaker.max_wer,
        }));
    }
    let page_plan = json!({
        "accelerators": plan.accelerators,
        "stt": {"model": plan.stt.model, "build": plan.stt.build},
        "tts": speakers,
        "clips": page_clips,
    });
    write(
        &site.join("plan.json"),
        format!("{page_plan:#}\n").as_bytes(),
    )?;
    write(&site.join("page.mjs"), PAGE.as_bytes())?;
    let runner = dir.join("run.mjs");
    write(&runner, RUN.as_bytes())?;

    let chrome = env::var("CHROME").unwrap_or_else(|_| "google-chrome".into());
    let out = Command::new("node")
        .arg(&runner)
        .arg(&site)
        .arg(&chrome)
        .arg(&speech)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("node run.mjs: {error}"))?;
    if !out.status.success() {
        return Err(format!("node run.mjs: {}", out.status));
    }
    let report_json = String::from_utf8_lossy(&out.stdout);
    let page: Report = parse(report_json.trim().as_bytes(), "the page's report")?;

    let mut failed: Vec<String> = Vec::new();
    for check in &page.checks {
        println!(
            "{} {}: {}",
            if check.ok { "ok" } else { "FAILED" },
            check.name,
            check.detail
        );
        if !check.ok {
            failed.push(check.name.clone());
        }
    }
    if let Some(error) = &page.error {
        failed.push(format!("the page stopped: {error}"));
    }
    let rows: Vec<Row> = page
        .rows
        .into_iter()
        .map(|row| Row {
            pair: row.pair,
            language: row.language,
            said: row.said,
            heard: match (row.heard, row.error) {
                (Some(heard), None) => Ok(heard),
                (_, error) => Err(error.unwrap_or_else(|| "no transcript".into())),
            },
            max_wer: row.max_wer,
        })
        .collect();
    let accelerators = plan.accelerators.join(", ");
    let title = format!("Voice loop (web: headless Chrome, {accelerators})");
    let judged = report(&title, &rows, plan.max_wer);
    if !failed.is_empty() {
        return Err(format!(
            "{} check(s) failed: {}",
            failed.len(),
            failed.join("; ")
        ));
    }
    judged
}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|error| format!("{what}: {error}"))
}
