use serde_json::{json, Value};

use super::{elevenlabs, openai, without_source};

/// The parts of OpenAI's spec its facts come from, shaped as the spec shapes them.
fn openai_spec() -> Value {
    json!({ "components": { "schemas": {
        "CreateTranscriptionRequest": { "properties": {
            "model": { "anyOf": [{ "type": "string" }, { "type": "string", "enum": ["whisper-1", "gpt-4o-transcribe"] }] },
            "language": { "type": "string" },
        }},
        "CreateSpeechRequest": { "properties": {
            "model": { "anyOf": [{ "type": "string" }, { "type": "string", "enum": ["tts-1", "gpt-4o-mini-tts"] }] },
            "voice": { "anyOf": [
                { "anyOf": [{ "$ref": "#/components/schemas/VoiceIdsShared" }, { "type": "string", "enum": ["fable", "alloy"] }] },
                { "type": "object", "properties": { "id": { "type": "string" } } },
            ]},
            "speed": { "type": "number", "minimum": 0.25, "maximum": 4 },
            "response_format": { "type": "string", "enum": ["mp3", "pcm"] },
        }},
        "VoiceIdsShared": { "anyOf": [{ "type": "string" }, { "type": "string", "enum": ["alloy", "ash"] }] },
    }}})
}

#[test]
fn openais_models_voices_language_and_speed_come_from_its_requests() {
    assert_eq!(
        openai(&openai_spec()).expect("facts"),
        json!({
            "speech_to_text": [
                { "model": "whisper-1", "language": "language", "speed": null },
                { "model": "gpt-4o-transcribe", "language": "language", "speed": null },
            ],
            "text_to_speech": [
                { "model": "tts-1", "language": null, "speed": [0.25, 4.0] },
                { "model": "gpt-4o-mini-tts", "language": null, "speed": [0.25, 4.0] },
            ],
            "voices": ["alloy", "ash", "fable"],
        })
    );
}

#[test]
fn openais_facts_fail_without_pcm_speech() {
    let mut spec = openai_spec();
    spec["components"]["schemas"]["CreateSpeechRequest"]["properties"]["response_format"]["enum"] =
        json!(["mp3"]);
    assert!(openai(&spec).is_err());
}

/// The parts of ElevenLabs' spec its facts come from: two per-model requests, a request of another kind with a `const`
/// model, the speech-to-text body and the speech path's formats.
fn elevenlabs_spec() -> Value {
    let speed = json!({ "anyOf": [{ "type": "number", "minimum": 0.7, "maximum": 1.2 }, { "type": "null" }] });
    json!({
        "paths": {
            "/v1/text-to-speech/{voice_id}": { "post": { "parameters": [
                { "name": "output_format", "schema": { "type": "string", "enum": ["mp3_44100_128", "pcm_24000"] } },
            ]}},
            "/v1/speech-to-text": { "post": { "requestBody": { "content": { "multipart/form-data": {
                "schema": { "$ref": "#/components/schemas/Body" },
            }}}}},
        },
        "components": { "schemas": {
            "ElevenV3Request": { "properties": {
                "text": {}, "voice": {}, "model_id": { "const": "eleven_v3" }, "language_code": {},
                "voice_settings": { "anyOf": [{ "$ref": "#/components/schemas/V3Settings" }, { "type": "null" }] },
            }},
            "ElevenMultilingualV2Request": { "properties": {
                "text": {}, "voice": {}, "model_id": { "const": "eleven_multilingual_v2" },
                "voice_settings": { "anyOf": [{ "$ref": "#/components/schemas/Settings" }, { "type": "null" }] },
            }},
            "MusicRequest": { "properties": { "prompt": {}, "model_id": { "const": "music_v1" } } },
            "V3Settings": { "properties": { "stability": {} } },
            "Settings": { "properties": { "speed": speed } },
            "Body": { "properties": {
                "model_id": { "type": "string", "examples": ["scribe_v2", "scribe_v2_medical"] },
                "language_code": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
            }},
        }},
    })
}

#[test]
fn elevenlabs_models_come_from_its_per_model_requests_and_the_speech_to_text_body() {
    assert_eq!(
        elevenlabs(&elevenlabs_spec()).expect("facts"),
        json!({
            "speech_to_text": [
                { "model": "scribe_v2", "language": "language_code", "speed": null },
                { "model": "scribe_v2_medical", "language": "language_code", "speed": null },
            ],
            "text_to_speech": [
                { "model": "eleven_multilingual_v2", "language": null, "speed": [0.7, 1.2] },
                { "model": "eleven_v3", "language": "language_code", "speed": null },
            ],
            "voices": [],
        })
    );
}

#[test]
fn elevenlabs_facts_fail_without_pcm_24000_speech() {
    let mut spec = elevenlabs_spec();
    spec["paths"]["/v1/text-to-speech/{voice_id}"]["post"]["parameters"][0]["schema"]["enum"] =
        json!(["mp3_44100_128"]);
    assert!(elevenlabs(&spec).is_err());
}

#[test]
fn the_drift_check_ignores_where_the_facts_were_read() {
    let written = json!({ "source": "a", "sha256": "1", "voices": ["x"] });
    let now = json!({ "source": "b", "sha256": "2", "voices": ["x"] });
    assert_eq!(without_source(&written), without_source(&now));
    let drifted = json!({ "source": "b", "sha256": "2", "voices": ["y"] });
    assert_ne!(without_source(&written), without_source(&drifted));
}
