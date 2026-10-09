//! The remote providers for real, through the engine's public API only, as an app uses them: a host with the keys,
//! each provider's catalogue (`Engine::catalog`, `refresh`, `models`, `load`), then the remote model's `as_tts` and
//! `as_stt`. Few calls, and cheap ones, per provider whose key is in the environment (`OPENAI_API_KEY_CI`,
//! `ELEVENLABS_API_KEY_CI`); a provider without one is skipped, and says so:
//!
//! 1. the provider's OpenAPI spec, which the engine reads at run time for what the API does not say: it must still give
//!    the facts the provider is listed by (no `provider-spec-unreadable`);
//! 2. the key and the listing: `refresh` must list models, and, for a provider whose voices are the account's, voices;
//!    where the provider's own answer says which models speak (ElevenLabs'), every one of them must be offered. A
//!    failure shows the status and what the provider said (its own status and message: a scoped key's missing
//!    permission, say);
//! 3. one short speech (about 20 characters) from one text-to-speech model, which must last a plausible time at the
//!    model's rate and not be silent;
//! 4. one short transcription by one speech-to-text model: the voice loop's English LibriSpeech clip
//!    (`tests/voice_loop.json`), held to the loop's `max_wer`; the request must carry the language where the model's
//!    request takes one;
//! 5. a deliberately invalid key, whose listing must read `credential-rejected`: that call costs nothing.
//!
//! It spends money, so it is ignored unless asked for; the `remote-live` workflow asks, by hand and before each
//! release, through `cargo xtask remote-live`:
//!
//! ```sh
//! OPENAI_API_KEY_CI=... cargo test --locked --test remote_live -- --ignored --nocapture
//! ```
//!
//! Its table goes to `$SIDEVOICE_REMOTE_LIVE` (`summary.md`) when that is set. No key is ever printed.

// NativeHost is native only.
#![cfg(native)]

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::{env, fs};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use sidevoice_engine::{
    async_trait, Cancel, Capabilities, Capability, CatalogModel, CatalogStatus, Credentials,
    Engine, Fetcher, Host, HttpClient, HttpRequest, HttpResponse, LoadedModel, NativeHost,
    Result as EngineResult, Storage,
};

#[path = "voice_loop/audio.rs"]
#[allow(
    dead_code,
    reason = "the voice loop's audio: this test only reads a WAV file"
)]
mod audio;
#[path = "voice_loop/wer.rs"]
mod wer;

type Result<T> = std::result::Result<T, String>;

/// What it asks of each provider: the environment variable its key is in, the models it tries first (any other of
/// the listing's will do), and what its speech-to-text request calls the language.
const PROVIDERS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "openai",
        "OPENAI_API_KEY_CI",
        "gpt-4o-mini-tts",
        "gpt-4o-mini-transcribe",
        "name=\"language\"\r\n\r\nen\r\n",
    ),
    (
        "elevenlabs",
        "ELEVENLABS_API_KEY_CI",
        "eleven_flash_v2_5",
        "scribe_v2",
        "name=\"language_code\"\r\n\r\nen\r\n",
    ),
];

/// About 20 characters, said in English.
const SAID: &str = "Hello from Sidevoice.";

/// The parts of `tests/voice_loop.json` it reads.
#[derive(Deserialize)]
struct Plan {
    max_wer: f64,
    clips: Vec<Clip>,
}

#[derive(Deserialize)]
struct Clip {
    language: String,
    url: String,
    sha256: String,
    text: String,
}

#[test]
#[ignore = "real provider calls, with keys and money: `cargo xtask remote-live`"]
fn each_provider_with_a_key_lists_speaks_transcribes_and_refuses_a_bad_key() {
    let mut rows = Vec::new();
    let mut failed = false;
    for &(provider, variable, speaker, listener, language) in PROVIDERS {
        let Some(key) = env::var(variable).ok().filter(|key| !key.is_empty()) else {
            println!("{provider}: skipped, no {variable}");
            rows.push(format!("| {provider} | skipped: no `{variable}` | | | | |"));
            continue;
        };
        let checks = check(provider, &key, speaker, listener, language);
        failed |= checks.iter().any(|(_, outcome)| outcome.is_err());
        let cells: Vec<String> = checks
            .iter()
            .map(|(_, outcome)| match outcome {
                Ok(what) => what.clone(),
                Err(why) => format!("✗ {why}"),
            })
            .collect();
        for (what, outcome) in &checks {
            println!("{provider}: {what}: {outcome:?}");
        }
        rows.push(format!("| {provider} | {} |", cells.join(" | ")));
    }
    let table = format!(
        "## Remote providers\n\n| Provider | Spec | Listing | Speech | Transcription | Invalid key |\n|---|---|---|---|---|---|\n{}\n",
        rows.join("\n")
    );
    println!("{table}");
    if let Ok(dir) = env::var("SIDEVOICE_REMOTE_LIVE") {
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(Path::new(&dir).join("summary.md"), &table);
    }
    assert!(!failed, "a provider failed a check: the table says which");
}

/// A provider's catalogue as refreshed: its status and its models.
struct Listed {
    status: CatalogStatus,
    models: Vec<CatalogModel>,
}

/// The status `status` reads as, with what the provider said, if anything.
fn reads(status: &CatalogStatus) -> String {
    let code = status.reason.map_or("current", |reason| reason.code);
    match &status.detail {
        Some(detail) => format!("{code} ({detail})"),
        None => code.to_owned(),
    }
}

/// The checks of `provider`, each with what it found or why it failed.
fn check(
    provider: &str,
    key: &str,
    speaker: &str,
    listener: &str,
    language: &str,
) -> Vec<(&'static str, Result<String>)> {
    let host = Recording::new(provider, key);
    let requests = Arc::clone(&host.requests);
    let answered = Arc::clone(&host.models);
    let engine = match Engine::new(Box::new(host), Vec::new()) {
        Ok(engine) => engine,
        Err(error) => return vec![("engine", Err(error.to_string()))],
    };
    let listed = engine
        .catalog(provider)
        .map_err(|e| e.code.to_owned())
        .map(|catalog| {
            let status = block_on(catalog.refresh());
            let models = block_on(catalog.models(None)).unwrap_or_default();
            Listed { status, models }
        });
    let spec = listed.as_ref().map_err(Clone::clone).and_then(spec);
    let listing = listed
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|listed| listing(listed, lock(&answered).as_ref()));
    let speech = listed
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|listed| speak(&engine, provider, listed, speaker));
    let transcription = listed
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|listed| transcribe(&engine, provider, listed, listener, language, &requests));
    vec![
        ("spec", spec),
        ("listing", listing),
        ("speech", speech),
        ("transcription", transcription),
        ("invalid key", refused(provider)),
    ]
}

/// That the provider's spec was read, and gave the facts it lists models by.
fn spec(listed: &Listed) -> Result<String> {
    match listed.status.reason.map(|reason| reason.code) {
        Some("provider-spec-unreadable") => Err(reads(&listed.status)),
        _ => Ok("read".to_owned()),
    }
}

/// How many models a provider's own `/v1/models` answer says speak, where it says (ElevenLabs' `can_do_text_to_speech`;
/// OpenAI's lists ids alone).
fn speakers_answered(answer: &serde_json::Value) -> Option<usize> {
    let models = answer.as_array()?;
    let flagged = models
        .iter()
        .all(|model| model.get("can_do_text_to_speech").is_some());
    flagged.then(|| {
        models
            .iter()
            .filter(|model| model["can_do_text_to_speech"] == true)
            .count()
    })
}

/// That the listing is current and has models, and voices on its speakers; and, where the provider's answer says which
/// models speak, that every one of them is offered.
fn listing(listed: &Listed, answered: Option<&serde_json::Value>) -> Result<String> {
    if listed.status.reason.is_some() {
        return Err(reads(&listed.status));
    }
    let count = |capability| {
        listed
            .models
            .iter()
            .filter(|model| model.capabilities().contains(&capability))
            .count()
    };
    let (stt, tts) = (count(Capability::Stt), count(Capability::Tts));
    let voices = listed
        .models
        .iter()
        .find(|model| model.capabilities().contains(&Capability::Tts))
        .map_or(0, |model| model.voices().len());
    let speakers = answered.and_then(speakers_answered);
    let found = match speakers {
        Some(speakers) => {
            format!("{stt} stt, {tts} tts (of {speakers} the listing says speak), {voices} voices")
        }
        None => format!("{stt} stt, {tts} tts, {voices} voices"),
    };
    if stt == 0 || tts == 0 || voices == 0 || speakers.is_some_and(|speakers| speakers != tts) {
        return Err(found);
    }
    Ok(found)
}

/// The listed model of `capability`, `preferred` if listed.
fn pick<'a>(listed: &'a Listed, capability: Capability, preferred: &str) -> Result<&'a str> {
    let mut models = listed
        .models
        .iter()
        .filter(|model| model.capabilities().contains(&capability));
    let first = models.clone().next().map(CatalogModel::id);
    models
        .find(|model| model.id() == preferred)
        .map(CatalogModel::id)
        .or(first)
        .ok_or(format!("no {capability:?} model listed"))
}

/// `provider`'s model `id`, loaded from its catalogue.
fn load(engine: &Engine, provider: &str, id: &str) -> Result<LoadedModel> {
    let catalog = engine.catalog(provider).map_err(|e| e.code.to_owned())?;
    block_on(catalog.load(id, &|_| {}, &Cancel::new())).map_err(|e| e.code.to_owned())
}

/// `SAID`, spoken by one text-to-speech model with its first voice: it must last between half a second and eight, and
/// not be silent.
fn speak(engine: &Engine, provider: &str, listed: &Listed, preferred: &str) -> Result<String> {
    let id = pick(listed, Capability::Tts, preferred)?;
    let model = load(engine, provider, id)?;
    let tts = model.as_tts().ok_or("not text to speech")?;
    let voices = block_on(tts.voices());
    let voice = voices.first().ok_or("no voice")?;
    let audio =
        block_on(tts.speak(SAID, &voice.id, Some("en"), None)).map_err(|e| e.code.to_owned())?;
    let seconds = audio.samples.len() as f64 / f64::from(audio.sample_rate);
    let rms = (audio
        .samples
        .iter()
        .map(|s| f64::from(*s).powi(2))
        .sum::<f64>()
        / audio.samples.len().max(1) as f64)
        .sqrt();
    let found = format!("{id}: {seconds:.1} s, RMS {rms:.3}");
    if !(0.5..=8.0).contains(&seconds) || rms < 0.005 {
        return Err(found);
    }
    Ok(found)
}

/// The English clip transcribed by one speech-to-text model, in English, within the loop's word error rate, with the
/// language in its request.
fn transcribe(
    engine: &Engine,
    provider: &str,
    listed: &Listed,
    preferred: &str,
    language: &str,
    requests: &Mutex<Vec<HttpRequest>>,
) -> Result<String> {
    let plan: Plan =
        serde_json::from_str(include_str!("voice_loop.json")).map_err(|e| e.to_string())?;
    let clip = plan
        .clips
        .iter()
        .find(|clip| clip.language == "en")
        .ok_or("no English clip")?;
    let (samples, rate) = audio::read_wav(&download(clip)?)?;
    let id = pick(listed, Capability::Stt, preferred)?;
    let model = load(engine, provider, id)?;
    let stt = model.as_stt().ok_or("not speech to text")?;
    let heard =
        block_on(stt.transcribe(&samples, rate, Some("en"))).map_err(|e| e.code.to_owned())?;
    let wer = wer::wer(&clip.text, &heard);
    let sent = lock(requests)
        .iter()
        .rev()
        .find(|request| request.method == "POST")
        .is_some_and(|request| {
            request
                .body
                .windows(language.len())
                .any(|part| part == language.as_bytes())
        });
    let found = format!("{id}: \"{heard}\", WER {:.0}%", wer * 100.0);
    if wer > plan.max_wer || !sent {
        let language = if sent { "" } else { ", no language sent" };
        return Err(format!("{found}{language}"));
    }
    Ok(found)
}

/// That a key the provider does not know reads `credential-rejected`.
fn refused(provider: &str) -> Result<String> {
    let host = Recording::new(provider, "sidevoice-ci-invalid-key");
    let engine = Engine::new(Box::new(host), Vec::new()).map_err(|e| e.to_string())?;
    let catalog = engine.catalog(provider).map_err(|e| e.code.to_owned())?;
    let status = block_on(catalog.refresh());
    match status.reason.map(|reason| reason.code) {
        Some("credential-rejected") => Ok("credential-rejected".to_owned()),
        _ => Err(reads(&status)),
    }
}

/// The clip, downloaded and checked against its digest.
fn download(clip: &Clip) -> Result<Vec<u8>> {
    let dir = env::temp_dir().join("sidevoice-remote-live");
    let host = NativeHost::new(&dir).map_err(|e| e.code.to_owned())?;
    let bytes = block_on(async {
        let mut download = host.fetcher().fetch(&clip.url).await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = download.chunk().await? {
            bytes.extend(chunk);
        }
        Ok::<_, sidevoice_engine::Error>(bytes)
    })
    .map_err(|e| format!("{}: {e}", clip.url))?;
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest != clip.sha256 {
        return Err(format!("{}: sha256 {digest}", clip.url));
    }
    Ok(bytes)
}

/// The native host with one provider's key, which keeps every API request it sends (to see what reached the
/// provider).
struct Recording {
    host: NativeHost,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
    /// The provider's last answer to `GET /v1/models`, as JSON.
    models: Arc<Mutex<Option<serde_json::Value>>>,
}

impl Recording {
    fn new(provider: &str, key: &str) -> Self {
        let dir: PathBuf = env::temp_dir().join("sidevoice-remote-live");
        let host = NativeHost::new(dir)
            .expect("a native host")
            .with_credentials(Key(provider.to_owned(), key.to_owned()));
        Self {
            host,
            requests: Arc::default(),
            models: Arc::default(),
        }
    }
}

impl Host for Recording {
    fn capabilities(&self) -> Capabilities {
        self.host.capabilities()
    }

    fn storage(&self) -> &dyn Storage {
        self.host.storage()
    }

    fn fetcher(&self) -> &dyn Fetcher {
        self.host.fetcher()
    }

    fn http(&self) -> &dyn HttpClient {
        self
    }

    fn credentials(&self) -> &dyn Credentials {
        self.host.credentials()
    }
}

#[async_trait]
impl HttpClient for Recording {
    async fn send(&self, request: HttpRequest) -> EngineResult<HttpResponse> {
        lock(&self.requests).push(request.clone());
        let url = request.url.clone();
        let response = self.host.http().send(request).await?;
        if url.ends_with("/v1/models") && (200..=299).contains(&response.status) {
            *lock(&self.models) = serde_json::from_slice(&response.body).ok();
        }
        Ok(response)
    }
}

/// One provider's key.
struct Key(String, String);

#[async_trait]
impl Credentials for Key {
    async fn credential(&self, provider: &str) -> EngineResult<Option<String>> {
        Ok((provider == self.0).then(|| self.1.clone()))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `future` on a Tokio runtime, as the voice loop does: the native host's HTTP needs one.
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
