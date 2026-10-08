//! The engine's values as JavaScript objects, with JavaScript's names (`parametersM`, `recommendedBuild`, ...): what
//! the `WebEngine` resolves its promises to. A field the engine leaves empty (`None`) is left out.

use js_sys::{Array, Float32Array, Object};
use wasm_bindgen::JsValue;

use crate::{Accelerator, Audio, Capability, Gender, Model, ModelBuild, Progress, Reason, Voice};

#[cfg(test)]
mod tests;

/// `{ id, capabilities, parametersM, languages, license, voices, installed, builds, recommendedBuild? }`.
pub(super) fn model(model: &Model) -> JsValue {
    let capabilities: Array = model
        .capabilities
        .iter()
        .map(|c| JsValue::from(capability(*c)))
        .collect();
    object(&[
        ("id", Some(model.id.as_str().into())),
        ("capabilities", Some(capabilities.into())),
        ("parametersM", Some(model.parameters_m.into())),
        ("languages", Some(strings(&model.languages))),
        ("license", Some(model.license.as_str().into())),
        (
            "voices",
            Some(model.voices.iter().map(voice).collect::<Array>().into()),
        ),
        ("installed", Some(model.installed.into())),
        (
            "builds",
            Some(model.builds.iter().map(build).collect::<Array>().into()),
        ),
        (
            "recommendedBuild",
            model.recommended_build.as_deref().map(JsValue::from),
        ),
    ])
}

/// `{ id, backend, accelerator?, precision, downloadBytes, memoryMb, available, reasons, installed }`.
fn build(build: &ModelBuild) -> JsValue {
    object(&[
        ("id", Some(build.id.as_str().into())),
        ("backend", Some(build.backend.as_str().into())),
        (
            "accelerator",
            build.accelerator.map(|a| accelerator(a).into()),
        ),
        ("precision", Some(build.precision.as_str().into())),
        // A number: a download is far below 2^53 bytes.
        ("downloadBytes", Some((build.download_bytes as f64).into())),
        ("memoryMb", Some(build.memory_mb.into())),
        ("available", Some(build.available.into())),
        (
            "reasons",
            Some(build.reasons.iter().map(reason).collect::<Array>().into()),
        ),
        ("installed", Some(build.installed.into())),
    ])
}

/// `{ code, params: { needs?, has? } }`.
pub(super) fn reason(reason: &Reason) -> JsValue {
    let params = object(&[
        ("needs", reason.needs.map(JsValue::from)),
        ("has", reason.has.map(JsValue::from)),
    ]);
    object(&[("code", Some(reason.code.into())), ("params", Some(params))])
}

/// `{ id, languages, gender? }`.
pub(super) fn voice(voice: &Voice) -> JsValue {
    let gender = voice.gender.map(|gender| match gender {
        Gender::Female => "female",
        Gender::Male => "male",
    });
    object(&[
        ("id", Some(voice.id.as_str().into())),
        ("languages", Some(strings(&voice.languages))),
        ("gender", gender.map(JsValue::from)),
    ])
}

/// `{ files, done, received, size? }`.
pub(super) fn progress(progress: Progress) -> JsValue {
    object(&[
        ("files", Some((progress.files as f64).into())),
        ("done", Some((progress.done as f64).into())),
        ("received", Some((progress.received as f64).into())),
        ("size", progress.size.map(|size| (size as f64).into())),
    ])
}

/// `{ samples, sampleRate }`.
pub(super) fn audio(audio: &Audio) -> JsValue {
    object(&[
        (
            "samples",
            Some(Float32Array::from(audio.samples.as_slice()).into()),
        ),
        ("sampleRate", Some(audio.sample_rate.into())),
    ])
}

/// A capability's id, as the catalogue names it: `stt`, `tts`.
pub(super) fn capability(capability: Capability) -> &'static str {
    match capability {
        Capability::Stt => "stt",
        Capability::Tts => "tts",
    }
}

/// An accelerator's stable id, as a JavaScript host and the catalogue name it.
fn accelerator(accelerator: Accelerator) -> &'static str {
    match accelerator {
        Accelerator::Cpu => "cpu",
        Accelerator::Cuda => "cuda",
        Accelerator::CoreMl => "coreml",
        Accelerator::Metal => "metal",
        Accelerator::WebGpu => "webgpu",
        Accelerator::Wasm => "wasm",
    }
}

fn strings(strings: &[String]) -> JsValue {
    strings
        .iter()
        .map(|s| JsValue::from(s.as_str()))
        .collect::<Array>()
        .into()
}

/// A plain object with the properties that have a value.
fn object(properties: &[(&str, Option<JsValue>)]) -> JsValue {
    let object = Object::new();
    for (key, value) in properties {
        if let Some(value) = value {
            js_sys::Reflect::set(&object, &(*key).into(), value).expect("a plain object");
        }
    }
    object.into()
}
