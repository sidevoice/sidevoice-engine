//! What the remote backends share, without a provider: forms, audio in and out, language tags, JSON answers.

use super::{field, pcm16, primary_subtag, text, wav, Form};
use crate::test_support::build;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn a_form_is_its_parts_between_boundaries() {
    let (content_type, body) = Form::new()
        .text("model", "m")
        .file("file", "a.wav", "audio/wav", b"RIFF")
        .finish();
    let boundary = content_type
        .strip_prefix("multipart/form-data; boundary=")
        .expect("a multipart type");
    let expected = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nm\r\n--{boundary}\r\n\
         Content-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\nContent-Type: audio/wav\r\n\r\n\
         RIFF\r\n--{boundary}--\r\n"
    );
    assert_eq!(String::from_utf8(body).expect("text"), expected);
}

#[test]
fn a_turn_is_a_16_bit_mono_wav_and_speech_comes_back_from_16_bit_pcm() {
    let file = wav(&[0.0, 1.0, -1.0, 2.0], 16_000);
    assert_eq!(&file[..4], b"RIFF");
    assert_eq!(&file[8..16], b"WAVEfmt ");
    assert_eq!(u32::from_le_bytes(file[24..28].try_into().unwrap()), 16_000);
    assert_eq!(u16::from_le_bytes([file[34], file[35]]), 16, "bits");
    assert_eq!(
        u32::from_le_bytes(file[40..44].try_into().unwrap()),
        8,
        "data bytes"
    );
    let samples = pcm16(&file[44..]);
    assert_eq!(samples.len(), 4);
    assert!(
        (samples[1] - 1.0).abs() < 1e-3 && (samples[3] - 1.0).abs() < 1e-3,
        "clamped"
    );
    assert_eq!(
        pcm16(&[0x00, 0x40, 0x00, 0xC0, 0x7F]),
        [0.5, -0.5],
        "an odd byte is dropped"
    );
}

#[test]
fn a_language_is_sent_as_its_primary_subtag_where_the_build_maps_it() {
    assert_eq!(primary_subtag("es-ES"), "es");
    assert_eq!(primary_subtag("PT_br"), "pt");
    let mut build = build("m/openai", "openai", 0);
    assert_eq!(field(&build, "language"), None);
    build
        .call_params
        .insert("language".into(), vec!["language_code".into()]);
    assert_eq!(field(&build, "language"), Some("language_code"));
}

#[test]
fn an_answers_text_is_trimmed_and_its_absence_fails() {
    assert_eq!(
        text(br#"{"text": " hola "}"#, "text", "x").as_deref(),
        Ok("hola")
    );
    assert_eq!(text(b"{}", "text", "x").map_err(|e| e.code), Err("x"));
    assert_eq!(text(b"nope", "text", "x").map_err(|e| e.code), Err("x"));
}
