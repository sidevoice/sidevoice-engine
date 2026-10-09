use js_sys::Reflect;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

use super::{audio, progress, reason, vad_options, vad_output, voice};
use crate::{Audio, Gender, Progress, Reason, VadEvent, VadFrame, VadOptions, VadOutput, Voice};

fn get(value: &JsValue, key: &str) -> JsValue {
    Reflect::get(value, &key.into()).unwrap()
}

#[wasm_bindgen_test]
fn a_reason_has_its_code_and_only_the_numbers_it_has() {
    let memory = reason(&Reason::with_numbers("memory", 4096, 2048));
    assert_eq!(get(&memory, "code").as_string().as_deref(), Some("memory"));
    assert_eq!(get(&get(&memory, "params"), "needs").as_f64(), Some(4096.0));
    assert_eq!(get(&get(&memory, "params"), "has").as_f64(), Some(2048.0));
    let plain = reason(&Reason::new("no-accelerator"));
    let params = get(&plain, "params");
    assert!(!Reflect::has(&params, &"needs".into()).unwrap());
}

#[wasm_bindgen_test]
fn a_voice_has_a_gender_only_when_the_catalogue_states_one() {
    let dora = Voice {
        id: "ef_dora".into(),
        languages: vec!["es".into()],
        gender: Some(Gender::Female),
    };
    assert_eq!(
        get(&voice(&dora), "gender").as_string().as_deref(),
        Some("female")
    );
    let plain = Voice {
        id: "F1".into(),
        languages: vec!["es".into()],
        gender: None,
    };
    assert!(!Reflect::has(&voice(&plain), &"gender".into()).unwrap());
}

#[wasm_bindgen_test]
fn progress_and_audio_take_javascripts_names() {
    let report = progress(Progress {
        files: 3,
        done: 1,
        received: 10,
        size: None,
    });
    assert_eq!(get(&report, "files").as_f64(), Some(3.0));
    assert!(!Reflect::has(&report, &"size".into()).unwrap());
    let spoken = audio(&Audio {
        samples: vec![0.5, -0.5],
        sample_rate: 24_000,
    });
    assert_eq!(get(&spoken, "sampleRate").as_f64(), Some(24_000.0));
    let samples: js_sys::Float32Array = get(&spoken, "samples").into();
    assert_eq!(samples.to_vec(), [0.5, -0.5]);
}

/// `{ ... }` from JSON.
fn parsed(json: &str) -> js_sys::Object {
    js_sys::JSON::parse(json).unwrap().into()
}

#[wasm_bindgen_test]
fn vad_options_left_out_take_their_defaults_and_malformed_ones_are_refused() {
    assert_eq!(vad_options(None), Ok(VadOptions::default()));
    assert_eq!(vad_options(Some(&parsed("{}"))), Ok(VadOptions::default()));
    let set = vad_options(Some(&parsed(
        r#"{"threshold": 0.6, "minSilenceMs": 300, "minSpeechMs": 100}"#,
    )));
    assert_eq!(
        set,
        Ok(VadOptions {
            threshold: 0.6,
            min_silence_ms: 300,
            min_speech_ms: 100
        })
    );
    for bad in [
        r#"{"threshold": "0.5"}"#,
        r#"{"minSilenceMs": 2.5}"#,
        r#"{"minSpeechMs": -1}"#,
    ] {
        assert_eq!(
            vad_options(Some(&parsed(bad))).map_err(|e| e.code),
            Err("invalid-vad-options"),
            "{bad}"
        );
    }
}

#[wasm_bindgen_test]
fn a_vad_output_has_its_frames_and_typed_events() {
    let output = vad_output(&VadOutput {
        frames: vec![
            VadFrame {
                end: 512,
                speech: true,
                probability: Some(0.75),
            },
            VadFrame {
                end: 1024,
                speech: false,
                probability: None,
            },
        ],
        events: vec![
            VadEvent::SpeechStart { at: 512 },
            VadEvent::SpeechEnd { start: 0, end: 900 },
        ],
    });
    let frames: js_sys::Array = get(&output, "frames").into();
    assert_eq!(get(&frames.get(0), "end").as_f64(), Some(512.0));
    assert_eq!(get(&frames.get(0), "speech").as_bool(), Some(true));
    assert_eq!(get(&frames.get(0), "probability").as_f64(), Some(0.75));
    assert!(!Reflect::has(&frames.get(1), &"probability".into()).unwrap());
    let events: js_sys::Array = get(&output, "events").into();
    let start = events.get(0);
    assert_eq!(
        get(&start, "type").as_string().as_deref(),
        Some("speech-start")
    );
    assert_eq!(get(&start, "at").as_f64(), Some(512.0));
    let end = events.get(1);
    assert_eq!(get(&end, "type").as_string().as_deref(), Some("speech-end"));
    assert_eq!(
        (get(&end, "start").as_f64(), get(&end, "end").as_f64()),
        (Some(0.0), Some(900.0))
    );
}
