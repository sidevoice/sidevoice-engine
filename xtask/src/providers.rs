//! What the remote providers' APIs do not say about their models, derived from each provider's official OpenAPI spec,
//! which nobody types. `pin-providers` fetches each spec, derives its facts and writes them to the provider's
//! `facts.json` (`src/provider/<provider>/facts.json`), with the spec's URL and SHA-256:
//!
//! - OpenAI (github.com/openai/openai-openapi, `openapi.json` at `main`'s commit, which the URL pins): the
//!   speech-to-text models are the ids `CreateTranscriptionRequest.model` enumerates, the text-to-speech ones those of
//!   `CreateSpeechRequest.model`; the voices, every id `CreateSpeechRequest.voice` enumerates; a model's language
//!   field is its request's `language`, when the request has one; the speed range is `CreateSpeechRequest.speed`'s.
//! - ElevenLabs (api.elevenlabs.io's `openapi.json`, which no URL pins: its SHA-256 says which one was read): the
//!   text-to-speech models are the `const` `model_id` of each per-model generation request (a schema with `text`,
//!   `voice` and a `const` `model_id`, `ElevenFlashV2_5Request`, ...), each with its `language_code` field if it has
//!   one and the speed range of its `voice_settings`; the speech-to-text models, the model ids the speech-to-text
//!   request gives as examples (the spec enumerates none), with its `language_code` field. Its voices are the
//!   account's, listed live: none here.
//!
//! Each provider's speech format must be one its spec lists (OpenAI's `pcm`, ElevenLabs' `pcm_24000`), or pinning
//! fails.
//!
//! `pin-providers --check` derives the facts again from the specs as they are now and fails if they differ from those
//! written, writing nothing: that is the drift check. A spec that changed without changing the facts passes, and is
//! said.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::{read, repo, run_in, sha256, write, Result};

#[cfg(test)]
mod tests;

/// One provider: its backend's directory, where its spec is, and how its facts are derived.
struct Provider {
    id: &'static str,
    spec: fn() -> Result<String>,
    derive: fn(&Value) -> Result<Value>,
}

const PROVIDERS: &[Provider] = &[
    Provider {
        id: "openai",
        spec: openai_spec,
        derive: openai,
    },
    Provider {
        id: "elevenlabs",
        spec: || Ok("https://api.elevenlabs.io/openapi.json".to_owned()),
        derive: elevenlabs,
    },
];

/// `cargo xtask pin-providers [--check]`.
pub(crate) fn pin(check: bool) -> Result<()> {
    let mut drifted = Vec::new();
    for provider in PROVIDERS {
        let url = (provider.spec)()?;
        let text = run_in(Path::new("."), "curl -fsSL --retry 3", &[&url])?;
        let spec: Value = serde_json::from_str(&text).map_err(|error| format!("{url}: {error}"))?;
        let facts = (provider.derive)(&spec).map_err(|error| format!("{url}: {error}"))?;
        let path = facts_path(provider.id);
        let mut doc = Map::new();
        doc.insert("source".into(), url.clone().into());
        doc.insert("sha256".into(), sha256(text.as_bytes()).into());
        doc.extend(facts.as_object().cloned().unwrap_or_default());
        let doc = Value::Object(doc);
        if check {
            let written: Value = serde_json::from_slice(&read(&path)?)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if without_source(&written) != without_source(&doc) {
                println!("drift: {}: {url} says other facts", provider.id);
                drifted.push(provider.id);
            } else if written["sha256"] != doc["sha256"] {
                println!(
                    "unchanged: {}: the spec changed, its facts did not",
                    provider.id
                );
            }
        } else {
            write(&path, format!("{doc:#}\n").as_bytes())?;
            println!("pinned: {} from {url}", provider.id);
        }
    }
    if !drifted.is_empty() {
        let drifted = drifted.join(", ");
        return Err(format!(
            "the providers' specs drifted: {drifted}; run `cargo xtask pin-providers` and review the facts"
        ));
    }
    Ok(())
}

fn facts_path(provider: &str) -> PathBuf {
    repo().join(format!("src/provider/{provider}/facts.json"))
}

/// A facts file without where it was read from: what the drift check compares.
fn without_source(doc: &Value) -> Value {
    let mut doc = doc.clone();
    if let Some(doc) = doc.as_object_mut() {
        doc.remove("source");
        doc.remove("sha256");
    }
    doc
}

/// OpenAI's spec at the commit `main` names now, by a URL that pins it. GitHub is asked with `GITHUB_TOKEN` when it is
/// set (CI's runners share the anonymous rate limit).
fn openai_spec() -> Result<String> {
    let repo = "openai/openai-openapi";
    let auth = std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|token| !token.is_empty());
    let auth = auth.map(|token| format!("Authorization: Bearer {token}"));
    let mut args: Vec<&str> = auth
        .as_deref()
        .into_iter()
        .flat_map(|header| ["-H", header])
        .collect();
    let url = format!("https://api.github.com/repos/{repo}/commits/main");
    args.push(&url);
    let commit = run_in(Path::new("."), "curl -fsSL --retry 3", &args)?;
    let commit: Value = serde_json::from_str(&commit).map_err(|error| format!("{url}: {error}"))?;
    let sha = commit["sha"].as_str().ok_or(format!("{url}: no commit"))?;
    Ok(format!(
        "https://raw.githubusercontent.com/{repo}/{sha}/openapi.json"
    ))
}

/// OpenAI's facts from its spec.
fn openai(spec: &Value) -> Result<Value> {
    let transcription = schema(spec, "CreateTranscriptionRequest")?;
    let speech = schema(spec, "CreateSpeechRequest")?;
    let formats = strings(spec, &speech["properties"]["response_format"]);
    if !formats.iter().any(|format| format == "pcm") {
        return Err("CreateSpeechRequest.response_format: no pcm".into());
    }
    let model = |request: &Value, name: &str, speed: bool| -> Result<Vec<Value>> {
        let ids = strings(spec, &request["properties"]["model"]);
        if ids.is_empty() {
            return Err(format!("{name}.model: no model ids"));
        }
        let language = request["properties"].get("language").map(|_| "language");
        let speed = if speed {
            Some(
                range(spec, &request["properties"]["speed"])
                    .ok_or(format!("{name}.speed: no range"))?,
            )
        } else {
            None
        };
        Ok(ids.iter().map(|id| fact(id, language, speed)).collect())
    };
    let voices = strings(spec, &speech["properties"]["voice"]);
    if voices.is_empty() {
        return Err("CreateSpeechRequest.voice: no voices".into());
    }
    Ok(json!({
        "speech_to_text": model(transcription, "CreateTranscriptionRequest", false)?,
        "text_to_speech": model(speech, "CreateSpeechRequest", true)?,
        "voices": voices,
    }))
}

/// ElevenLabs' facts from its spec.
fn elevenlabs(spec: &Value) -> Result<Value> {
    let path = &spec["paths"]["/v1/text-to-speech/{voice_id}"]["post"];
    let format = path["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|parameter| parameter["name"] == "output_format")
        .map(|parameter| strings(spec, &parameter["schema"]))
        .unwrap_or_default();
    if !format.iter().any(|format| format == "pcm_24000") {
        return Err("/v1/text-to-speech/{voice_id}: no pcm_24000 output".into());
    }
    let schemas = spec["components"]["schemas"]
        .as_object()
        .ok_or("no components.schemas")?;
    let mut speakers = Vec::new();
    for request in schemas.values() {
        let properties = &request["properties"];
        let (Some(id), true, true) = (
            properties["model_id"]["const"].as_str(),
            properties.get("text").is_some(),
            properties.get("voice").is_some(),
        ) else {
            continue;
        };
        let language = properties.get("language_code").map(|_| "language_code");
        let settings = object(spec, &properties["voice_settings"]);
        let speed = range(spec, &settings["properties"]["speed"]);
        speakers.push(fact(id, language, speed));
    }
    speakers.sort_by(|a, b| a["model"].as_str().cmp(&b["model"].as_str()));
    if speakers.is_empty() {
        return Err("no per-model text-to-speech requests".into());
    }
    let body = &spec["paths"]["/v1/speech-to-text"]["post"]["requestBody"]["content"];
    let body = body
        .as_object()
        .and_then(|content| content.values().next())
        .map(|content| resolve(spec, &content["schema"]))
        .ok_or("/v1/speech-to-text: no request body")?;
    let language = body["properties"]
        .get("language_code")
        .map(|_| "language_code");
    let scribes: Vec<_> = strings(spec, &body["properties"]["model_id"]["examples"])
        .iter()
        .map(|id| fact(id, language, None))
        .collect();
    if scribes.is_empty() {
        return Err("/v1/speech-to-text: no model ids".into());
    }
    Ok(json!({
        "speech_to_text": scribes,
        "text_to_speech": speakers,
        "voices": [],
    }))
}

/// One model's facts: its id, the field its language goes in, and its speed range.
fn fact(model: &str, language: Option<&str>, speed: Option<[f64; 2]>) -> Value {
    json!({ "model": model, "language": language, "speed": speed })
}

/// The schema `name` of `components.schemas`.
fn schema<'a>(spec: &'a Value, name: &str) -> Result<&'a Value> {
    spec["components"]["schemas"]
        .get(name)
        .ok_or(format!("no schema {name}"))
}

/// `value`, or what its `$ref` points at, followed to the end.
fn resolve<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
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
fn object<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
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
fn strings(spec: &Value, value: &Value) -> Vec<String> {
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
fn range(spec: &Value, value: &Value) -> Option<[f64; 2]> {
    let value = resolve(spec, value);
    if let (Some(min), Some(max)) = (value["minimum"].as_f64(), value["maximum"].as_f64()) {
        return Some([min, max]);
    }
    value["anyOf"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|branch| range(spec, branch))
}
