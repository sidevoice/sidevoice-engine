//! The bridge to JavaScript: the engine as the web sees it, `WebEngine.create(host)`, where `host` is any object
//! with the methods of [`JsHost`]. Only in the wasm32 build (the npm package).
//!
//! A thin wrapper over [`Engine`]: the same operations under JavaScript's names (`models`, `install`, `uninstall`,
//! `load`, `catalogs`, and on each `Catalog` `status`, `models`, `refresh` and `load`, and on the `LocalModel` and
//! `RemoteModel` they return, `capabilities`,
//! `asStt().transcribe`, `asTts().voices` and `speak`, and `asVad().stream`, whose stream `accept`s audio, and
//! `asEndOfTurn().probability`), with a
//! progress callback and an `AbortSignal` where the engine takes a [`ProgressSink`](crate::ProgressSink) and a
//! [`Cancel`]. Every failure rejects with an `Error` that carries the engine's stable `code` and its `params`, which
//! the page translates; its message is the code too. One a remote provider caused also carries `detail`, what the
//! provider said, for the page to show as it is.
//!
//! Inside: `host` (the JavaScript host as the engine sees it, with the web build's storage, downloads and API calls),
//! `opfs` (the browser's private file system, which the storage and the transformers.js backend use) and `values` (the
//! engine's values as JavaScript objects).

use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::Poll;

use js_sys::{Array, Function, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{future_to_promise, JsFuture};
use web_sys::AbortSignal;

use crate::capability::Resident;
use crate::{BundledCatalog, Cancel, Engine, Error, LocalModel, Progress, RemoteModel, VadStream};

mod host;
pub(crate) mod opfs;
#[cfg(test)]
mod tests;
mod values;

pub use host::JsHost;
use host::WebHost;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &'static str = r#"
/** What every promise of the engine rejects with: the engine's stable code and its parameters, and, for a call a
 * remote provider refused, what the provider said (`detail`: its own code and message), to show as it is. */
export interface EngineError extends Error { code: string; params: Record<string, number>; detail?: string; }
/** Why a build does not run here: a stable code and its numbers (`needs`, `has`) when there are any. */
export interface Reason { code: string; params: { needs?: number; has?: number }; }
export interface Voice { id: string; name?: string; languages: string[]; gender?: "female" | "male"; }
export interface ModelBuild {
  id: string; backend: string; accelerator?: string; precision: string; downloadBytes: number; memoryMb: number;
  available: boolean; reasons: Reason[]; installed: boolean;
}
/** The speeds a model takes, 1 being its normal pace. A model with no `speed` takes none. */
export interface SpeedRange { min: number; max: number; }
/** A model as a catalogue lists it, whichever: what every model has. */
export interface ModelInfo {
  id: string; capabilities: ("stt" | "tts" | "vad" | "end-of-turn")[]; languages: string[];
  voices: Voice[]; speed?: SpeedRange;
}
/** A model of the local catalogue: its family, size, licence, builds ranked and what is installed. */
export interface LocalModelInfo extends ModelInfo {
  family: string; parametersM: number; license: string; installed: boolean; builds: ModelBuild[];
  recommendedBuild?: string;
}
/** A model of a remote provider, as listed. */
export interface RemoteModelInfo extends ModelInfo {}
/** A model ready to use, from a catalogue's `load`: either hands out the same capabilities. */
export type Model = LocalModel | RemoteModel;
/** A place models come from: what the local catalogue and each provider's have in common (`id`, `name`, `status()`,
 * `models(capability?)`, `refresh()`, `load(model)`). Each lists and loads its own types; nothing tells them apart but
 * which catalogue was asked. */
export type Catalog = LocalCatalog | RemoteCatalog;
/** How a catalogue stands: why it is not current (absent when it is), whether its models are the last kept, and what
 * the provider said when it last refused, for a developer to read. */
export interface CatalogStatus { reason?: Reason; stale: boolean; detail?: string; }
/** How far an install has got: files done of all, and the bytes of the file being downloaded. */
export interface Progress { files: number; done: number; received: number; size?: number; }
export interface Audio { samples: Float32Array; sampleRate: number; }
/** How a voice activity stream decides what is speech; each left out takes its default (0.5, 500 ms, 250 ms). */
export interface VadOptions { threshold?: number; minSilenceMs?: number; minSpeechMs?: number; }
/** One window of a stream: where it ends (in samples), whether the stream is in speech, the model's probability. */
export interface VadFrame { end: number; speech: boolean; probability?: number; }
/** Speech confirmed at `at`, or speech that ran from `start` to just before `end`: positions in samples. */
export type VadEvent = { type: "speech-start"; at: number } | { type: "speech-end"; start: number; end: number };
export interface VadOutput { frames: VadFrame[]; events: VadEvent[]; }
"#;

/// The engine, for JavaScript.
#[wasm_bindgen]
pub struct WebEngine {
    engine: Rc<Engine>,
}

#[wasm_bindgen]
impl WebEngine {
    /// Asks the host for its capabilities once, and builds the engine on the bundled catalogue. The host may also have
    /// `credential(provider)`, which returns (or resolves to) the key of a remote provider (`"openai"`, `"elevenlabs"`)
    /// or `null`: the engine asks it each time a provider is listed or a remote model called, and keeps no key.
    /// Rejects with `host-capabilities` when the host's `capabilities()` fails, `host-capabilities-<field>` when what it
    /// reports is malformed.
    pub async fn create(host: JsHost) -> Result<WebEngine, JsValue> {
        let caps = host
            .capabilities()
            .await
            .map_err(|_| coded(Error::new("host-capabilities")))?;
        let host = WebHost::new(host, &caps).map_err(|error| {
            let error = JsValue::from(error);
            let code = Reflect::get(&error, &"message".into()).unwrap_or_default();
            set(&error, "code", &code);
            set(&error, "params", &js_sys::Object::new());
            error
        })?;
        let engine = Engine::new(Box::new(host), vec![Box::new(BundledCatalog)])
            .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(WebEngine {
            engine: Rc::new(engine),
        })
    }

    /// The ids of the backends in this build.
    pub fn backends(&self) -> Vec<String> {
        self.engine
            .backends()
            .into_iter()
            .map(|backend| backend.id.to_owned())
            .collect()
    }

    /// Installs `build` of `model` (the recommended one when left out), telling `onProgress` as it goes; `signal`
    /// aborts it, and the promise rejects with `cancelled`, leaving nothing half downloaded.
    #[wasm_bindgen(unchecked_return_type = "Promise<void>")]
    pub fn install(
        &self,
        model: String,
        build: Option<String>,
        #[wasm_bindgen(js_name = onProgress, unchecked_param_type = "(progress: Progress) => void")]
        on_progress: Option<Function>,
        signal: Option<AbortSignal>,
    ) -> Promise {
        let engine = Rc::clone(&self.engine);
        promise(async move {
            let cancel = Cancel::new();
            let progress = reporter(on_progress);
            let installing = engine.install(&model, build.as_deref(), &progress, &cancel);
            until_aborted(signal.as_ref(), &cancel, installing).await?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Removes `model`'s files, except those another model uses; rejects with `model-in-use` while it is loaded.
    #[wasm_bindgen(unchecked_return_type = "Promise<void>")]
    pub fn uninstall(&self, model: String) -> Promise {
        let engine = Rc::clone(&self.engine);
        promise(async move {
            engine.uninstall(&model).await?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Loads `build` of `model` (an installed one that runs here, else the recommended one, when left out),
    /// installing it first if it is not, as [`WebEngine::install`] does. The model stays in memory while a
    /// `LocalModel` of its build lives, or an `Stt` or `Tts` one handed out: freeing them all (`free()`, or letting
    /// them be collected) unloads it.
    #[wasm_bindgen(unchecked_return_type = "Promise<LocalModel>")]
    pub fn load(
        &self,
        model: String,
        build: Option<String>,
        #[wasm_bindgen(js_name = onProgress, unchecked_param_type = "(progress: Progress) => void")]
        on_progress: Option<Function>,
        signal: Option<AbortSignal>,
    ) -> Promise {
        let engine = Rc::clone(&self.engine);
        promise(async move {
            let cancel = Cancel::new();
            let progress = reporter(on_progress);
            let loading = engine.load(&model, build.as_deref(), &progress, &cancel);
            let loaded = until_aborted(signal.as_ref(), &cancel, loading).await?;
            Ok(WebLocalModel { loaded }.into())
        })
    }

    /// Every catalogue of this engine: the `LocalCatalog` first, then each provider's `RemoteCatalog`, by id. Both have
    /// `id`, `name`, `status()`, `models(capability?)`, `refresh()` and `load(model)`; each lists and loads its own
    /// types.
    #[wasm_bindgen(unchecked_return_type = "(LocalCatalog | RemoteCatalog)[]")]
    pub fn catalogs(&self) -> Array {
        let local = JsValue::from(self.local_catalog());
        let providers = self.engine.catalogs().into_iter().skip(1).map(|catalog| {
            JsValue::from(WebRemoteCatalog {
                engine: Rc::clone(&self.engine),
                id: catalog.id().to_owned(),
            })
        });
        std::iter::once(local).chain(providers).collect()
    }

    /// The catalogue `id` (`"local"`, `"openai"`, `"elevenlabs"`); throws `catalog-not-found` for any other.
    #[wasm_bindgen(unchecked_return_type = "LocalCatalog | RemoteCatalog")]
    pub fn catalog(&self, id: String) -> Result<JsValue, JsValue> {
        if id == crate::LOCAL_CATALOG {
            return Ok(self.local_catalog().into());
        }
        self.remote_catalog(id).map(JsValue::from)
    }

    /// The local catalogue: its models are `LocalModelInfo`s (builds, install state) and it loads `LocalModel`s.
    #[wasm_bindgen(js_name = localCatalog)]
    pub fn local_catalog(&self) -> WebLocalCatalog {
        WebLocalCatalog {
            engine: Rc::clone(&self.engine),
        }
    }

    /// The remote provider `id`: its models are `RemoteModelInfo`s and it loads `RemoteModel`s; throws
    /// `catalog-not-found` for an id that is no provider.
    #[wasm_bindgen(js_name = remoteCatalog)]
    pub fn remote_catalog(&self, id: String) -> Result<WebRemoteCatalog, JsValue> {
        self.engine.remote_catalog(&id).map_err(coded)?;
        Ok(WebRemoteCatalog {
            engine: Rc::clone(&self.engine),
            id,
        })
    }
}

/// The capability JavaScript names, `None` when left out; `invalid-capability` for any other.
fn capability_of(capability: Option<String>) -> Result<Option<crate::Capability>, Error> {
    capability
        .as_deref()
        .map(values::capability_named)
        .transpose()
}

/// The local catalogue, for JavaScript.
#[wasm_bindgen(js_name = LocalCatalog)]
pub struct WebLocalCatalog {
    engine: Rc<Engine>,
}

#[wasm_bindgen(js_class = LocalCatalog)]
impl WebLocalCatalog {
    /// Its id: `"local"`.
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        crate::LOCAL_CATALOG.to_owned()
    }

    /// Absent: the app names the local catalogue.
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> Option<String> {
        None
    }

    /// How it stands: always current.
    #[wasm_bindgen(unchecked_return_type = "Promise<CatalogStatus>")]
    pub fn status(&self) -> Promise {
        let engine = Rc::clone(&self.engine);
        promise(async move {
            let local = engine.local_catalog();
            Ok(values::catalog_status(
                &crate::Catalog::status(&local).await,
            ))
        })
    }

    /// Its models, those of `capability` alone when given (`"stt"`, `"tts"`, `"vad"`, `"end-of-turn"`), each with its
    /// builds ranked and what is installed; rejects with `invalid-capability` for any other.
    #[wasm_bindgen(unchecked_return_type = "Promise<LocalModelInfo[]>")]
    pub fn models(
        &self,
        #[wasm_bindgen(unchecked_param_type = "\"stt\" | \"tts\" | \"vad\" | \"end-of-turn\"")]
        capability: Option<String>,
    ) -> Promise {
        let engine = Rc::clone(&self.engine);
        promise(async move {
            let capability = capability_of(capability)?;
            let models = engine.local_catalog().models(capability).await?;
            Ok(models.iter().map(values::model).collect::<Array>().into())
        })
    }

    /// Nothing to read again: compiled in. Its status.
    #[wasm_bindgen(unchecked_return_type = "Promise<CatalogStatus>")]
    pub fn refresh(&self) -> Promise {
        self.status()
    }

    /// The model `model`, loaded as `WebEngine.load` does with no build (installing it first and telling
    /// `onProgress`; `signal` aborts it).
    #[wasm_bindgen(unchecked_return_type = "Promise<LocalModel>")]
    pub fn load(
        &self,
        model: String,
        #[wasm_bindgen(js_name = onProgress, unchecked_param_type = "(progress: Progress) => void")]
        on_progress: Option<Function>,
        signal: Option<AbortSignal>,
    ) -> Promise {
        let engine = Rc::clone(&self.engine);
        promise(async move {
            let cancel = Cancel::new();
            let progress = reporter(on_progress);
            let local = engine.local_catalog();
            let loading = local.load(&model, &progress, &cancel);
            let loaded = until_aborted(signal.as_ref(), &cancel, loading).await?;
            Ok(WebLocalModel { loaded }.into())
        })
    }
}

/// A remote provider's catalogue, for JavaScript.
#[wasm_bindgen(js_name = RemoteCatalog)]
pub struct WebRemoteCatalog {
    engine: Rc<Engine>,
    id: String,
}

#[wasm_bindgen(js_class = RemoteCatalog)]
impl WebRemoteCatalog {
    /// Its id: the provider's.
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.id.clone()
    }

    /// The provider's name ("OpenAI").
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> Option<String> {
        let catalog = self.engine.remote_catalog(&self.id).ok()?;
        crate::Catalog::name(&catalog).map(str::to_owned)
    }

    /// How it stands: `{ reason?, stale, detail? }`, listing the provider first if its listing is missing or old.
    #[wasm_bindgen(unchecked_return_type = "Promise<CatalogStatus>")]
    pub fn status(&self) -> Promise {
        let (engine, id) = (Rc::clone(&self.engine), self.id.clone());
        promise(async move {
            let catalog = engine.remote_catalog(&id)?;
            Ok(values::catalog_status(
                &crate::Catalog::status(&catalog).await,
            ))
        })
    }

    /// Its models as listed, those of `capability` alone when given; rejects with `invalid-capability` for any other.
    #[wasm_bindgen(unchecked_return_type = "Promise<RemoteModelInfo[]>")]
    pub fn models(
        &self,
        #[wasm_bindgen(unchecked_param_type = "\"stt\" | \"tts\" | \"vad\" | \"end-of-turn\"")]
        capability: Option<String>,
    ) -> Promise {
        let (engine, id) = (Rc::clone(&self.engine), self.id.clone());
        promise(async move {
            let capability = capability_of(capability)?;
            let models = engine.remote_catalog(&id)?.models(capability).await;
            Ok(models
                .iter()
                .map(values::provider_model)
                .collect::<Array>()
                .into())
        })
    }

    /// Reads it again now, whatever its age: its status after.
    #[wasm_bindgen(unchecked_return_type = "Promise<CatalogStatus>")]
    pub fn refresh(&self) -> Promise {
        let (engine, id) = (Rc::clone(&self.engine), self.id.clone());
        promise(async move {
            let catalog = engine.remote_catalog(&id)?;
            Ok(values::catalog_status(
                &crate::Catalog::refresh(&catalog).await,
            ))
        })
    }

    /// The model `model`, as listed: it calls the provider each time it is used. `onProgress` is never called (nothing
    /// is installed); `signal` aborts it.
    #[wasm_bindgen(unchecked_return_type = "Promise<RemoteModel>")]
    pub fn load(
        &self,
        model: String,
        #[wasm_bindgen(js_name = onProgress, unchecked_param_type = "(progress: Progress) => void")]
        _on_progress: Option<Function>,
        signal: Option<AbortSignal>,
    ) -> Promise {
        let (engine, id) = (Rc::clone(&self.engine), self.id.clone());
        promise(async move {
            let cancel = Cancel::new();
            let catalog = engine.remote_catalog(&id)?;
            let loading = catalog.load(&model, &cancel);
            let remote = until_aborted(signal.as_ref(), &cancel, loading).await?;
            Ok(WebRemoteModel { remote }.into())
        })
    }
}

/// The capabilities `resident` has, for JavaScript.
fn capabilities(resident: &Arc<Resident>) -> Vec<String> {
    let capabilities = resident.capabilities().iter();
    capabilities
        .map(|c| values::capability(*c).to_owned())
        .collect()
}

/// A local model in memory, for JavaScript. It and the `Stt` and `Tts` it hands out each keep the model in memory:
/// freeing them all (`free()`, or letting them be collected) unloads it.
#[wasm_bindgen(js_name = LocalModel)]
pub struct WebLocalModel {
    loaded: LocalModel,
}

#[wasm_bindgen(js_class = LocalModel)]
impl WebLocalModel {
    /// The model's id.
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.loaded.id().to_owned()
    }

    /// The id of the build that was loaded.
    #[wasm_bindgen(getter)]
    pub fn build(&self) -> String {
        self.loaded.build().to_owned()
    }

    /// What it can do: `"stt"`, `"tts"`, `"vad"`, `"end-of-turn"`.
    #[wasm_bindgen(unchecked_return_type = "(\"stt\" | \"tts\" | \"vad\" | \"end-of-turn\")[]")]
    pub fn capabilities(&self) -> Vec<String> {
        capabilities(self.loaded.resident())
    }

    /// The model as speech to text, if it is one.
    #[wasm_bindgen(js_name = asStt)]
    pub fn as_stt(&self) -> Option<WebStt> {
        WebStt::of(self.loaded.resident())
    }

    /// The model as text to speech, if it is one.
    #[wasm_bindgen(js_name = asTts)]
    pub fn as_tts(&self) -> Option<WebTts> {
        WebTts::of(self.loaded.resident())
    }

    /// The model as a voice activity detector, if it is one.
    #[wasm_bindgen(js_name = asVad)]
    pub fn as_vad(&self) -> Option<WebVad> {
        WebVad::of(self.loaded.resident())
    }

    /// The model as an end-of-turn classifier, if it is one.
    #[wasm_bindgen(js_name = asEndOfTurn)]
    pub fn as_end_of_turn(&self) -> Option<WebEndOfTurn> {
        WebEndOfTurn::of(self.loaded.resident())
    }
}

/// A remote provider's model, for JavaScript: each call on what it hands out goes to the provider.
#[wasm_bindgen(js_name = RemoteModel)]
pub struct WebRemoteModel {
    remote: RemoteModel,
}

#[wasm_bindgen(js_class = RemoteModel)]
impl WebRemoteModel {
    /// The provider's id.
    #[wasm_bindgen(getter)]
    pub fn provider(&self) -> String {
        self.remote.provider().to_owned()
    }

    /// The provider's id of the model.
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.remote.id().to_owned()
    }

    /// What it can do: `"stt"`, `"tts"`, `"vad"`, `"end-of-turn"`.
    #[wasm_bindgen(unchecked_return_type = "(\"stt\" | \"tts\" | \"vad\" | \"end-of-turn\")[]")]
    pub fn capabilities(&self) -> Vec<String> {
        capabilities(self.remote.resident())
    }

    /// The model as speech to text, if it is one.
    #[wasm_bindgen(js_name = asStt)]
    pub fn as_stt(&self) -> Option<WebStt> {
        WebStt::of(self.remote.resident())
    }

    /// The model as text to speech, if it is one.
    #[wasm_bindgen(js_name = asTts)]
    pub fn as_tts(&self) -> Option<WebTts> {
        WebTts::of(self.remote.resident())
    }

    /// The model as a voice activity detector, if it is one.
    #[wasm_bindgen(js_name = asVad)]
    pub fn as_vad(&self) -> Option<WebVad> {
        WebVad::of(self.remote.resident())
    }

    /// The model as an end-of-turn classifier, if it is one.
    #[wasm_bindgen(js_name = asEndOfTurn)]
    pub fn as_end_of_turn(&self) -> Option<WebEndOfTurn> {
        WebEndOfTurn::of(self.remote.resident())
    }
}

/// A model, local or remote, as speech to text.
#[wasm_bindgen(js_name = Stt)]
pub struct WebStt {
    model: Arc<Resident>,
}

impl WebStt {
    /// `model` as speech to text, if it is one.
    fn of(model: &Arc<Resident>) -> Option<Self> {
        (model.capabilities().contains(&crate::Capability::Stt)).then(|| Self {
            model: Arc::clone(model),
        })
    }
}

#[wasm_bindgen(js_class = Stt)]
impl WebStt {
    /// What is said in `audio` (mono samples at `sampleRate` Hz, one whole turn; the engine resamples it), in
    /// `language` (a BCP 47 tag), or in the one the model detects when left out.
    #[wasm_bindgen(unchecked_return_type = "Promise<string>")]
    pub fn transcribe(
        &self,
        audio: Vec<f32>,
        #[wasm_bindgen(js_name = sampleRate)] sample_rate: u32,
        language: Option<String>,
    ) -> Promise {
        let model = Arc::clone(&self.model);
        promise(async move {
            let stt = model
                .as_stt()
                .ok_or(Error::new("model-cannot-transcribe"))?;
            let text = stt
                .transcribe(&audio, sample_rate, language.as_deref())
                .await?;
            Ok(text.into())
        })
    }
}

/// A model, local or remote, as text to speech.
#[wasm_bindgen(js_name = Tts)]
pub struct WebTts {
    model: Arc<Resident>,
}

impl WebTts {
    /// `model` as text to speech, if it is one.
    fn of(model: &Arc<Resident>) -> Option<Self> {
        (model.capabilities().contains(&crate::Capability::Tts)).then(|| Self {
            model: Arc::clone(model),
        })
    }
}

#[wasm_bindgen(js_class = Tts)]
impl WebTts {
    /// The voices it speaks with: `{ id, languages, gender? }`.
    #[wasm_bindgen(unchecked_return_type = "Promise<Voice[]>")]
    pub fn voices(&self) -> Promise {
        let model = Arc::clone(&self.model);
        promise(async move {
            let tts = model.as_tts().ok_or(Error::new("model-cannot-speak"))?;
            let voices = tts.voices().await;
            Ok(voices.iter().map(values::voice).collect::<Array>().into())
        })
    }

    /// `text` spoken with `voice` (one of `voices()`), in `language` (a BCP 47 tag) for a model that speaks several,
    /// at `speed` (1 when left out): `{ samples, sampleRate }`, at the model's own rate.
    #[wasm_bindgen(unchecked_return_type = "Promise<Audio>")]
    pub fn speak(
        &self,
        text: String,
        voice: String,
        language: Option<String>,
        speed: Option<f32>,
    ) -> Promise {
        let model = Arc::clone(&self.model);
        promise(async move {
            let tts = model.as_tts().ok_or(Error::new("model-cannot-speak"))?;
            let audio = tts.speak(&text, &voice, language.as_deref(), speed).await?;
            Ok(values::audio(&audio))
        })
    }
}

/// A model, local or remote, as an end-of-turn classifier.
#[wasm_bindgen(js_name = EndOfTurn)]
pub struct WebEndOfTurn {
    model: Arc<Resident>,
}

impl WebEndOfTurn {
    /// `model` as an end-of-turn classifier, if it is one.
    fn of(model: &Arc<Resident>) -> Option<Self> {
        (model.capabilities().contains(&crate::Capability::EndOfTurn)).then(|| Self {
            model: Arc::clone(model),
        })
    }
}

#[wasm_bindgen(js_class = EndOfTurn)]
impl WebEndOfTurn {
    /// How many seconds of the end of a turn the model hears: earlier audio does not count.
    #[wasm_bindgen(getter)]
    pub fn seconds(&self) -> u32 {
        self.model
            .as_end_of_turn()
            .map_or(0, |model| model.seconds())
    }

    /// The probability, from 0 to 1, that the turn in `audio` (mono samples at `sampleRate` Hz, from its start to now)
    /// is complete; the engine keeps the last `seconds` and resamples them.
    #[wasm_bindgen(unchecked_return_type = "Promise<number>")]
    pub fn probability(
        &self,
        audio: Vec<f32>,
        #[wasm_bindgen(js_name = sampleRate)] sample_rate: u32,
    ) -> Promise {
        let model = Arc::clone(&self.model);
        promise(async move {
            let classifier = model
                .as_end_of_turn()
                .ok_or(Error::new("model-cannot-end-turns"))?;
            Ok(classifier.probability(&audio, sample_rate).await?.into())
        })
    }
}

/// A model, local or remote, as a voice activity detector.
#[wasm_bindgen(js_name = Vad)]
pub struct WebVad {
    model: Arc<Resident>,
}

impl WebVad {
    /// `model` as a voice activity detector, if it is one.
    fn of(model: &Arc<Resident>) -> Option<Self> {
        (model.capabilities().contains(&crate::Capability::Vad)).then(|| Self {
            model: Arc::clone(model),
        })
    }
}

#[wasm_bindgen(js_class = Vad)]
impl WebVad {
    /// A new stream, from sample 0 and with no speech, deciding with `options` (each left out takes its default).
    /// Rejects with `invalid-vad-options` for options out of their bounds or of the wrong type.
    #[wasm_bindgen(unchecked_return_type = "Promise<VadStream>")]
    pub fn stream(
        &self,
        #[wasm_bindgen(unchecked_param_type = "VadOptions")] options: Option<js_sys::Object>,
    ) -> Promise {
        let model = Arc::clone(&self.model);
        promise(async move {
            let options = values::vad_options(options.as_ref())?;
            let vad = model.as_vad().ok_or(Error::new("model-cannot-detect"))?;
            let stream = vad.stream(options).await?;
            Ok(WebVadStream {
                sample_rate: stream.sample_rate(),
                window: stream.window(),
                stream: Rc::new(async_lock::Mutex::new(stream)),
            }
            .into())
        })
    }
}

/// One stream of audio through a voice activity detector: feed it mono samples at `sampleRate` as they come, in pieces
/// of any length. It keeps the model in memory until it is freed. Calls on it wait for one another.
#[wasm_bindgen(js_name = VadStream)]
pub struct WebVadStream {
    stream: Rc<async_lock::Mutex<VadStream>>,
    sample_rate: u32,
    window: usize,
}

#[wasm_bindgen(js_class = VadStream)]
impl WebVadStream {
    /// The rate the stream takes, in Hz: the model's own. It is not resampled.
    #[wasm_bindgen(getter, js_name = sampleRate)]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// How many samples one window, one frame, holds.
    #[wasm_bindgen(getter)]
    pub fn window(&self) -> usize {
        self.window
    }

    /// Takes the next `samples` and runs the model on every whole window it now has: `{ frames, events }`.
    #[wasm_bindgen(unchecked_return_type = "Promise<VadOutput>")]
    pub fn accept(&self, samples: Vec<f32>) -> Promise {
        let stream = Rc::clone(&self.stream);
        promise(async move {
            let output = stream.lock().await.accept(&samples).await?;
            Ok(values::vad_output(&output))
        })
    }

    /// Ends the stream's audio: the speech in progress, if any, as its `speech-end` event (else `undefined`); the
    /// stream then starts over from sample 0.
    #[wasm_bindgen(unchecked_return_type = "Promise<VadEvent | undefined>")]
    pub fn finish(&self) -> Promise {
        let stream = Rc::clone(&self.stream);
        promise(async move {
            let ended = stream.lock().await.finish();
            Ok(ended.map_or(JsValue::UNDEFINED, |event| values::vad_event(&event)))
        })
    }

    /// Starts over from sample 0, forgetting any speech in progress.
    #[wasm_bindgen(unchecked_return_type = "Promise<void>")]
    pub fn reset(&self) -> Promise {
        let stream = Rc::clone(&self.stream);
        promise(async move {
            stream.lock().await.reset();
            Ok(JsValue::UNDEFINED)
        })
    }
}

/// `work` as a promise, rejected with the error's code and parameters.
fn promise(work: impl Future<Output = Result<JsValue, Error>> + 'static) -> Promise {
    future_to_promise(async move { work.await.map_err(coded) })
}

/// `error` as JavaScript sees it: an `Error` whose message is its code, with `code` and `params` (none: an engine
/// error has no parameters).
fn coded(error: Error) -> JsValue {
    let js = js_sys::Error::new(error.code);
    set(&js, "code", &error.code.into());
    set(&js, "params", &js_sys::Object::new());
    if let Some(detail) = &error.detail {
        set(&js, "detail", &detail.into());
    }
    js.into()
}

/// What tells `on_progress` how far an install has got, if there is one.
fn reporter(on_progress: Option<Function>) -> impl Fn(Progress) {
    move |progress: Progress| {
        if let Some(on_progress) = &on_progress {
            let progress = values::progress(progress);
            if let Err(error) = on_progress.call1(&JsValue::UNDEFINED, &progress) {
                web_sys::console::warn_2(&"sidevoice-engine: onProgress threw:".into(), &error);
            }
        }
    }
}

/// Runs `work` until it ends or `signal` aborts: then `cancel` is cancelled and `work` dropped, which stops a download
/// at once (its request is aborted) and discards what it had written. Rejects with `cancelled` if `signal` is already
/// aborted.
async fn until_aborted<T>(
    signal: Option<&AbortSignal>,
    cancel: &Cancel,
    work: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    let Some(signal) = signal else {
        return work.await;
    };
    if signal.aborted() {
        return Err(Error::new("cancelled"));
    }
    let mut resolve = None;
    let aborted = Promise::new(&mut |done, _| resolve = Some(done));
    let resolve = resolve.expect("a promise calls its executor at once");
    let cancelled = cancel.clone();
    let on_abort = Closure::<dyn FnMut()>::new(move || {
        cancelled.cancel();
        let _ = resolve.call0(&JsValue::UNDEFINED);
    });
    let listener: &Function = on_abort.as_ref().unchecked_ref();
    signal
        .add_event_listener_with_callback("abort", listener)
        .map_err(|_| Error::new("cancelled"))?;
    let mut work = pin!(work);
    let mut aborted = JsFuture::from(aborted);
    let ended = std::future::poll_fn(|context| {
        if let Poll::Ready(ended) = work.as_mut().poll(context) {
            return Poll::Ready(ended);
        }
        match std::pin::Pin::new(&mut aborted).poll(context) {
            Poll::Ready(_) => Poll::Ready(Err(Error::new("cancelled"))),
            Poll::Pending => Poll::Pending,
        }
    })
    .await;
    let _ = signal.remove_event_listener_with_callback("abort", listener);
    ended
}

fn set(object: &JsValue, key: &str, value: &JsValue) {
    Reflect::set(object, &key.into(), value).expect("setting a property of a plain object");
}
