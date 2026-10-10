//! What a provider's API does not say about its models, as its official OpenAPI spec says it: which models are speech
//! to text and which text to speech where the API does not say it, the request field a call's language goes in, the
//! speed range, and the voices when the spec fixes them. The spec enriches a model the provider lists; it does not
//! decide that it exists. A model the spec has no request of its own for follows its kind's general request: a
//! parameter is offered only where that request takes it. Each provider derives its facts from its spec at run time
//! (`Adapter::facts`), with the readers here; the engine keeps them with the provider's listing. Nothing about a remote
//! model is written by hand.

use serde_json::Value;

use crate::catalog::{Capability, SpeedRange};
use crate::{Error, Result};

/// One provider's facts, derived from its spec.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Facts {
    /// The speech-to-text models the spec names.
    pub(crate) speech_to_text: Vec<ModelFacts>,
    /// The text-to-speech models the spec has a request of their own for.
    pub(crate) text_to_speech: Vec<ModelFacts>,
    /// What the general speech-to-text request takes, for a model with no facts of its own (its `model` is empty).
    pub(crate) transcription: ModelFacts,
    /// What the general text-to-speech request takes, for a model with no facts of its own (its `model` is empty).
    pub(crate) speech: ModelFacts,
    /// The voices every text-to-speech model of the provider takes, when the spec fixes them (OpenAI's); empty for a
    /// provider whose voices are the account's, listed live (ElevenLabs').
    pub(crate) voices: Vec<String>,
}

/// One model's facts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelFacts {
    /// The provider's id of the model.
    pub(crate) model: String,
    /// The request field a call's language goes in; `None` when the model's request takes none.
    pub(crate) language: Option<String>,
    /// The lowest and the highest speed it takes; `None` when its request takes none.
    pub(crate) speed: Option<[f32; 2]>,
    /// The value its request's `chunking_strategy` takes to let the provider cut a long turn itself (OpenAI's `auto`),
    /// for a model the spec says takes that field; `None` for any other.
    pub(crate) chunking: Option<String>,
}

impl Facts {
    /// What `id`, a model of `capability`, takes: its own facts, or its kind's general request's.
    pub(crate) fn of(&self, capability: Capability, id: &str) -> ModelFacts {
        let general = match capability {
            Capability::Stt => &self.transcription,
            _ => &self.speech,
        };
        let own = self
            .models()
            .find(|(kind, facts)| *kind == capability && facts.model == id);
        let facts = own.map_or(general, |(_, facts)| facts);
        ModelFacts {
            model: id.to_owned(),
            ..facts.clone()
        }
    }

    /// Every model the spec describes, speech to text first.
    pub(crate) fn models(&self) -> impl Iterator<Item = (Capability, &ModelFacts)> {
        let stt = self
            .speech_to_text
            .iter()
            .map(|facts| (Capability::Stt, facts));
        let tts = self
            .text_to_speech
            .iter()
            .map(|facts| (Capability::Tts, facts));
        stt.chain(tts)
    }
}

impl ModelFacts {
    /// A model's facts.
    pub(crate) fn new(model: &str, language: Option<&str>, speed: Option<[f32; 2]>) -> Self {
        Self {
            model: model.to_owned(),
            language: language.map(str::to_owned),
            speed,
            chunking: None,
        }
    }

    /// Its speeds as the engine lists them, with the spec at `source` as where they come from; `None` for a model
    /// that takes no speed.
    pub(crate) fn range(&self, source: &str) -> Option<SpeedRange> {
        self.speed.map(|[min, max]| SpeedRange {
            min: Some(min),
            max: Some(max),
            source: source.to_owned(),
        })
    }

    /// `speed` within the model's range; `None` for a model that takes no speed.
    pub(crate) fn speed(&self, speed: f32) -> Option<f32> {
        self.speed.map(|[low, high]| speed.clamp(low, high))
    }
}

/// What a spec that does not say what the facts need fails with.
pub(crate) fn unreadable() -> Error {
    Error::new("provider-spec-unreadable")
}

/// The schema `name` of `components.schemas`.
pub(crate) fn schema<'a>(spec: &'a Value, name: &str) -> Result<&'a Value> {
    spec["components"]["schemas"]
        .get(name)
        .ok_or_else(unreadable)
}

/// `value`, or what its `$ref` points at, followed to the end.
pub(crate) fn resolve<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
    let mut value = value;
    for _ in 0..16 {
        let Some(path) = value["$ref"]
            .as_str()
            .and_then(|path| path.strip_prefix("#/"))
        else {
            break;
        };
        value = path.split('/').fold(spec, |value, key| &value[key]);
    }
    value
}

/// The object schema `value` is, through `$ref` and `anyOf` (a nullable object); `null` if none.
pub(crate) fn object<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
    let value = resolve(spec, value);
    if value.get("properties").is_some() {
        return value;
    }
    value["anyOf"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|branch| object(spec, branch))
        .find(|branch| branch.get("properties").is_some())
        .unwrap_or(&Value::Null)
}

/// Every string a schema allows by name (its `enum`, `const` or a list of them, through `$ref`, `anyOf` and `oneOf`),
/// each once, in order; or the strings of a plain list (`examples`).
pub(crate) fn strings(spec: &Value, value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    collect(spec, value, &mut found);
    found
}

fn collect(spec: &Value, value: &Value, found: &mut Vec<String>) {
    let value = resolve(spec, value);
    let mut add = |string: &str| {
        if !found.iter().any(|known| known == string) {
            found.push(string.to_owned());
        }
    };
    if let Some(list) = value.as_array() {
        list.iter().filter_map(Value::as_str).for_each(&mut add);
        return;
    }
    if let Some(constant) = value["const"].as_str() {
        add(constant);
    }
    for string in value["enum"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        add(string);
    }
    for key in ["anyOf", "oneOf"] {
        for branch in value[key].as_array().into_iter().flatten() {
            collect(spec, branch, found);
        }
    }
}

/// A number's `minimum` and `maximum`, through `$ref` and `anyOf` (a nullable number).
pub(crate) fn range(spec: &Value, value: &Value) -> Option<[f32; 2]> {
    let value = resolve(spec, value);
    if let (Some(min), Some(max)) = (value["minimum"].as_f64(), value["maximum"].as_f64()) {
        return Some([min as f32, max as f32]);
    }
    value["anyOf"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|branch| range(spec, branch))
}

/// Every description a schema gives, through `$ref`, `anyOf` and `oneOf`: where a spec says in words what its types do
/// not (which models a field is for).
pub(crate) fn descriptions(spec: &Value, value: &Value) -> Vec<String> {
    let value = resolve(spec, value);
    let mut found: Vec<String> = value["description"]
        .as_str()
        .map(str::to_owned)
        .into_iter()
        .collect();
    for key in ["anyOf", "oneOf"] {
        for branch in value[key].as_array().into_iter().flatten() {
            found.extend(descriptions(spec, branch));
        }
    }
    found
}
