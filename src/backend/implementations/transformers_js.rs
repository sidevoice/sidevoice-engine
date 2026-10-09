// transformers.js is a JavaScript module of the npm package: it only exists in the web build.
#![cfg(web)]
//! transformers.js, on ONNX Runtime Web: speech to text with Whisper, text to speech with Kokoro and Supertonic, and
//! voice activity detection with Silero, on WebGPU or WebAssembly.
//!
//! # Binding
//!
//! `@huggingface/transformers` is a dependency of the npm package, pinned exactly in `npm/package.json`; nothing of it
//! is compiled into the engine. `open` imports it with a dynamic `import()` the first time a model of this backend is
//! loaded, so a page that never loads one never fetches it, and the `Library` is that module: its classes and
//! pipelines, called through `wasm-bindgen`. Kokoro also needs eSpeak NG for its phonemes (`kokoro.rs`), another
//! dependency of the npm package, imported the same way when a Kokoro model is loaded.
//!
//! # Files
//!
//! transformers.js reads a model's files through its hub: `config.json`, `onnx/…`, `tokenizer.json`, by their path in
//! the model's repository. The engine's installer has them in storage, by key; `hub.rs` hands transformers.js those
//! and nothing else: each loaded model is served under a name of its own, and each path it asks for is the path of a
//! build file's URL in its repository, opened from where the host stored it. transformers.js never downloads a model
//! file, and never caches one twice. Which model a build is follows from its files ([`Kind::of`]):
//!
//! - Whisper: `encoder`, `decoder`, its configs and tokenizer (the `automatic-speech-recognition` pipeline);
//! - Kokoro: `model`, a `voices/<id>` per voice, its config and tokenizer (`StyleTextToSpeech2Model`);
//! - Supertonic: `text_encoder`, `latent_denoiser`, `voice_decoder` (each with its external data), a `voices/<id>` per
//!   voice, its config and tokenizer (the `text-to-speech` pipeline).
//! - Silero VAD: `vad`, its ONNX model alone (a custom model, run window by window: `silero.rs`).
//!
//! The build's `precision` is transformers.js's `dtype` ("q8", "fp16", "fp32"), which picks the ONNX files by suffix.
//!
//! # Accelerators
//!
//! WebGPU first, then WebAssembly: transformers.js's `device`, `webgpu` or `wasm`. The default probe keeps what the
//! page reported; whether the browser grants a WebGPU adapter is found out when a model loads.
//!
//! # What it does not own
//!
//! ONNX Runtime Web's own WebAssembly is loaded by transformers.js, from the CDN at the version it pins, unless the
//! page set `env.backends.onnx.wasm.wasmPaths` before; it is not one of the engine's installed files yet.

use async_trait::async_trait;
use js_sys::{Function, Object, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::backend::registry::BackendFactory;
use crate::backend::{Backend, BackendModel, BackendSpec, Library, Load};
use crate::host::Accelerator;
use crate::install::Installed;
use crate::{Error, Result};

mod hub;
mod kokoro;
mod phonemes;
mod silero;
mod supertonic;
#[cfg(test)]
mod tests;
mod whisper;

use hub::Hub;

struct TransformersJs;

const SPEC: BackendSpec = BackendSpec {
    id: "transformers-js",
    name: "Transformers.js",
    description:
        "Models in the browser on ONNX Runtime Web: Whisper, Kokoro, Supertonic and Silero VAD.",
    upstream: "https://github.com/huggingface/transformers.js",
    accelerators: &[Accelerator::WebGpu, Accelerator::Wasm],
    requirements: &[],
    provider: None,
};

inventory::submit! { BackendFactory(|| Box::new(TransformersJs)) }

#[wasm_bindgen(
    inline_js = "export function importTransformers() { return import('@huggingface/transformers'); }"
)]
extern "C" {
    /// The npm package's `@huggingface/transformers`, imported when first asked for.
    #[wasm_bindgen(catch, js_name = importTransformers)]
    async fn import_transformers() -> Result<JsValue, JsValue>;
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for TransformersJs {
    fn spec(&self) -> &BackendSpec {
        &SPEC
    }

    /// Imports transformers.js and points its hub at the engine's files. Nothing installed is read: transformers.js
    /// comes with the npm package. Fails with `library-open-failed`.
    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        let module = import_transformers()
            .await
            .map_err(|error| failed("library-open-failed", &error))?;
        let hub = Hub::open(&module).map_err(|error| failed("library-open-failed", &error))?;
        Ok(Box::new(TransformersJsLibrary { module, hub }))
    }
}

/// The imported module, and its hub serving the engine's files.
struct TransformersJsLibrary {
    module: JsValue,
    hub: std::rc::Rc<Hub>,
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Library for TransformersJsLibrary {
    async fn load(&self, load: Load<'_>) -> Result<Box<dyn BackendModel>> {
        let (build, files) = (load.build, load.files);
        let kind = Kind::of(files)?;
        let model = Model {
            module: self.module.clone(),
            served: Hub::serve(&self.hub, build, files)?,
            device: device(load.accelerator)?,
            dtype: build.precision.clone(),
        };
        Ok(match kind {
            Kind::Whisper => Box::new(whisper::Whisper::load(model).await?),
            Kind::Kokoro => Box::new(kokoro::Kokoro::load(model, files).await?),
            Kind::Supertonic => Box::new(supertonic::Supertonic::load(model, files).await?),
            Kind::Silero => Box::new(silero::Silero::load(model).await?),
        })
    }
}

/// What every model of this backend loads with: the module, its files served under a name of their own, and where
/// and at which precision it runs.
struct Model {
    module: JsValue,
    served: hub::Served,
    device: &'static str,
    dtype: String,
}

impl Model {
    /// `{ device, dtype }`, the options transformers.js loads a model with.
    fn options(&self) -> JsValue {
        object(&[
            ("device", self.device.into()),
            ("dtype", self.dtype.as_str().into()),
        ])
    }

    /// The export `name` of the module (a class, `pipeline`, `Tensor`).
    fn export(&self, name: &str) -> Result<JsValue, JsValue> {
        Reflect::get(&self.module, &name.into())
    }
}

/// Which of the models this backend runs a build is: it follows from its files (see *Files*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Whisper,
    Kokoro,
    Supertonic,
    Silero,
}

impl Kind {
    /// `encoder` and `decoder` make it Whisper, `text_encoder` and `latent_denoiser` Supertonic, a `model` with
    /// `voices/…` Kokoro, and `vad` Silero; anything else is `unsupported-model`.
    fn of(files: &Installed) -> Result<Self> {
        let has = |key| files.file(key).is_some();
        if has("encoder") && has("decoder") {
            Ok(Self::Whisper)
        } else if has("text_encoder") && has("latent_denoiser") {
            Ok(Self::Supertonic)
        } else if has("model") && files.files.keys().any(|key| key.starts_with(VOICES)) {
            Ok(Self::Kokoro)
        } else if has("vad") {
            Ok(Self::Silero)
        } else {
            Err(Error::new("unsupported-model"))
        }
    }
}

/// The prefix of a voice's key: `voices/<id>`.
const VOICES: &str = "voices/";

/// The ids of the voices among `files`, by their keys, in order.
fn voices(files: &Installed) -> Vec<String> {
    let keys = files.files.keys();
    keys.filter_map(|key| key.strip_prefix(VOICES))
        .map(str::to_owned)
        .collect()
}

/// transformers.js's `device` for `accelerator`, or `unsupported-accelerator`.
fn device(accelerator: Accelerator) -> Result<&'static str> {
    match accelerator {
        Accelerator::WebGpu => Ok("webgpu"),
        Accelerator::Wasm => Ok("wasm"),
        _ => Err(Error::new("unsupported-accelerator")),
    }
}

/// A plain object with these properties.
fn object(properties: &[(&str, JsValue)]) -> JsValue {
    let object = Object::new();
    for (key, value) in properties {
        Reflect::set(&object, &(*key).into(), value).expect("a plain object");
    }
    object.into()
}

/// Calls `function` (a function, or a callable object such as a pipeline) on `this` with `args`, and waits for what
/// it returns when that is a promise. Through `Reflect.apply`: transformers.js's callables (pipelines, tokenizers, models) are
/// callable without inheriting `Function.prototype`, so they have no `apply` of their own.
async fn call(function: &JsValue, this: &JsValue, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let function: &Function = function.unchecked_ref();
    let args: js_sys::Array = args.iter().collect();
    let returned = Reflect::apply(function, this, &args)?;
    match returned.dyn_into::<Promise>() {
        Ok(promise) => JsFuture::from(promise).await,
        Err(value) => Ok(value),
    }
}

/// Calls the method `name` of `object` with `args`, waiting for it.
async fn call_method(object: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let method = Reflect::get(object, &name.into())?;
    call(&method, object, args).await
}

/// Frees what `object` holds in the background (its `dispose()`, which may return a promise): dropping cannot wait.
fn dispose(object: JsValue) {
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(error) = call_method(&object, "dispose", &[]).await {
            web_sys::console::warn_2(&"sidevoice-engine: dispose failed:".into(), &error);
        }
    });
}

/// `samples` as spoken, or `speech-failed` if any is not a finite number: a model run in a precision its device
/// cannot hold (Kokoro in fp16 on WebGPU) gives NaN, which a WAV writer turns into silence.
fn finite(samples: Vec<f32>) -> Result<Vec<f32>> {
    match samples.iter().position(|sample| !sample.is_finite()) {
        None => Ok(samples),
        Some(at) => Err(failed(
            "speech-failed",
            &format!("sample {at} of {} is not a finite number", samples.len()).into(),
        )),
    }
}

/// The stable `code`, with the cause in the console.
fn failed(code: &'static str, cause: &JsValue) -> Error {
    web_sys::console::warn_3(
        &"sidevoice-engine: transformers.js:".into(),
        &code.into(),
        cause,
    );
    Error::new(code)
}
