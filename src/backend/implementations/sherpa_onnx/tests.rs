//! What sherpa-onnx's backend checks before the library is asked anything: which model a build's files make, which
//! accelerators it takes, how it fails without its files, how a language reaches a model, and the ONNX metadata
//! reader. Running real models is `inference_tests.rs`, and the voice loop of `cargo xtask e2e`.

use std::io::{BufReader, Cursor};

use super::kokoro::espeak_voice;
use super::supertonic::indexer_named_as_required;
use super::{model_metadata, primary_language, provider, text, Kind, SherpaOnnx, SPEC};
use crate::backend::{Backend, LoadedModel};
use crate::host::Accelerator;
use crate::install::Installed;
use crate::test_support::{block_on, build};
use crate::Result;

#[test]
fn it_declares_the_cpu_alone_since_the_linked_libraries_have_no_core_ml() {
    assert_eq!(SPEC.accelerators, [Accelerator::Cpu]);
}

#[test]
fn the_providers_it_names_are_the_cpu_and_core_ml() {
    assert_eq!(provider(Accelerator::Cpu), Ok("cpu"));
    assert_eq!(provider(Accelerator::CoreMl), Ok("coreml"));
    for other in [
        Accelerator::Cuda,
        Accelerator::Metal,
        Accelerator::WebGpu,
        Accelerator::Wasm,
    ] {
        assert_eq!(
            provider(other).map(|_| ()).unwrap_err().code,
            "unsupported-accelerator"
        );
    }
}

fn installed(files: &[(&str, &str)]) -> Installed {
    Installed {
        files: files
            .iter()
            .map(|(key, path)| ((*key).to_owned(), (*path).to_owned()))
            .collect(),
    }
}

fn kind(files: &[(&str, &str)]) -> Result<Kind> {
    Kind::of(&installed(files))
}

#[test]
fn the_model_follows_from_its_files() {
    let whisper = [("encoder", "e"), ("decoder", "d"), ("tokens", "t")];
    let transducer = [("encoder", "e"), ("decoder", "d"), ("joiner", "j")];
    let kokoro = [("model", "m"), ("voices", "v"), ("espeak-ng-data", "d")];
    let vits = [("model", "m"), ("tokens", "t"), ("espeak-ng-data", "d")];
    let supertonic = [("duration_predictor", "p"), ("text_encoder", "e")];
    assert_eq!(kind(&whisper), Ok(Kind::Whisper));
    assert_eq!(kind(&transducer), Ok(Kind::Transducer));
    assert_eq!(kind(&kokoro), Ok(Kind::Kokoro));
    assert_eq!(kind(&vits), Ok(Kind::Vits));
    assert_eq!(kind(&supertonic), Ok(Kind::Supertonic));
    assert_eq!(kind(&[]).unwrap_err().code, "unsupported-model");
    assert_eq!(
        kind(&[("tokens", "t")]).unwrap_err().code,
        "unsupported-model"
    );
}

#[test]
fn kokoro_refuses_core_ml_whatever_the_libraries_and_the_rest_would_take_it() {
    for other in [
        Kind::Whisper,
        Kind::Transducer,
        Kind::Vits,
        Kind::Supertonic,
    ] {
        assert_eq!(
            other.provider(Accelerator::CoreMl),
            Ok("coreml"),
            "{other:?}"
        );
    }
    assert_eq!(Kind::Kokoro.provider(Accelerator::Cpu), Ok("cpu"));
    assert_eq!(
        Kind::Kokoro.provider(Accelerator::CoreMl).unwrap_err().code,
        "unsupported-accelerator"
    );
}

fn load(accelerator: Accelerator, files: &[(&str, &str)]) -> Result<Box<dyn LoadedModel>> {
    let files = installed(files);
    let library = block_on(SherpaOnnx.open(&files))?;
    block_on(library.load(&build("test", "sherpa-onnx", 0), accelerator, &files))
}

#[test]
fn the_linked_library_opens_with_nothing_installed_and_a_model_missing_a_file_does_not_load() {
    let code = |result: Result<Box<dyn LoadedModel>>| result.map(|_| ()).unwrap_err().code;
    let cpu = Accelerator::Cpu;
    assert!(block_on(SherpaOnnx.open(&installed(&[]))).is_ok());
    assert_eq!(code(load(cpu, &[("encoder", "e")])), "file-not-installed");
    assert_eq!(code(load(cpu, &[("joiner", "j")])), "file-not-installed");
    assert_eq!(code(load(cpu, &[("voices", "v")])), "file-not-installed");
    assert_eq!(code(load(cpu, &[("model", "m")])), "file-not-installed");
    let kokoro = [("model", "m"), ("voices", "v")];
    assert_eq!(
        code(load(Accelerator::CoreMl, &kokoro)),
        "unsupported-accelerator"
    );
}

#[test]
fn text_with_a_nul_is_refused_before_the_crate_would_panic_on_it() {
    assert_eq!(text("hola").as_deref(), Ok("hola"));
    assert_eq!(text("a\0b").unwrap_err().code, "invalid-text");
}

#[test]
fn a_language_reaches_a_model_as_its_primary_subtag() {
    assert_eq!(primary_language("es-ES"), "es");
    assert_eq!(primary_language("EN_gb"), "en");
    assert_eq!(primary_language("es"), "es");
}

#[test]
fn a_multilingual_kokoro_reads_a_language_with_its_espeak_voice() {
    assert_eq!(espeak_voice("es-ES"), "es");
    assert_eq!(espeak_voice("es"), "es");
    assert_eq!(espeak_voice("en"), "en-us");
    assert_eq!(espeak_voice("en-US"), "en-us");
    assert_eq!(espeak_voice("en-GB"), "en-gb");
    assert_eq!(espeak_voice("pt-BR"), "pt-br");
    assert_eq!(espeak_voice("fr-FR"), "fr");
}

#[test]
fn supertonic_takes_an_indexer_named_bin_only_since_the_library_exits_otherwise() {
    assert!(indexer_named_as_required("/models/x/unicode_indexer.bin"));
    assert!(!indexer_named_as_required("/models/files/8402ca48e518"));
}

/// A protobuf length-delimited field: its tag, its length and its bytes.
fn field(number: u8, bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![(number << 3) | 2, u8::try_from(bytes.len()).expect("short")];
    out.extend_from_slice(bytes);
    out
}

fn entry(key: &str, value: &str) -> Vec<u8> {
    field(
        14,
        &[field(1, key.as_bytes()), field(2, value.as_bytes())].concat(),
    )
}

#[test]
fn metadata_is_read_past_the_other_fields_of_the_model() {
    let model = [
        vec![1 << 3, 8],        // ir_version = 8 (a varint)
        field(7, &[0xff; 100]), // the graph, skipped
        entry("model_type", "kokoro"),
        entry("speaker_names", "af,am_adam"),
    ]
    .concat();
    let find = |key| model_metadata::find(&mut BufReader::new(Cursor::new(&model)), key);
    assert_eq!(find("speaker_names").as_deref(), Some("af,am_adam"));
    assert_eq!(find("model_type").as_deref(), Some("kokoro"));
    assert_eq!(find("sample_rate"), None);
}
