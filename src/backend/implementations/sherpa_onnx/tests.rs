//! What sherpa-onnx's backend checks before the library is asked anything: which model a build's files make, which
//! accelerators it takes, how it fails without its files, and the ONNX metadata reader. Running real models is
//! `inference_tests.rs`.

use std::io::{BufReader, Cursor};

use super::{model_metadata, provider, text, Kind, SherpaOnnx, SPEC};
use crate::backend::{Backend, LoadedModel};
use crate::catalog::Build;
use crate::host::Accelerator;
use crate::install::Installed;
use crate::test_support::ready;
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
    assert_eq!(kind(&[("encoder", "e.onnx")]), Ok(Kind::Whisper));
    assert_eq!(kind(&[("voices", "v.bin")]), Ok(Kind::Kokoro));
    assert_eq!(kind(&[]).unwrap_err().code, "unsupported-model");
    assert_eq!(
        kind(&[("model", "m.onnx")]).unwrap_err().code,
        "unsupported-model"
    );
}

#[test]
fn kokoro_runs_on_the_cpu_only_and_whisper_on_core_ml_too() {
    assert_eq!(Kind::Whisper.provider(Accelerator::CoreMl), Ok("coreml"));
    assert_eq!(Kind::Kokoro.provider(Accelerator::Cpu), Ok("cpu"));
    assert_eq!(
        Kind::Kokoro.provider(Accelerator::CoreMl).unwrap_err().code,
        "unsupported-accelerator"
    );
}

fn load(accelerator: Accelerator, files: &[(&str, &str)]) -> Result<Box<dyn LoadedModel>> {
    let build = Build {
        id: "test".to_owned(),
        backend: "sherpa-onnx".to_owned(),
        format: "onnx".to_owned(),
        memory_mb: 0,
        accelerators: Vec::new(),
        files: Vec::new(),
    };
    ready(SherpaOnnx.load(&build, accelerator, &installed(files)))
}

#[test]
fn a_model_missing_a_file_does_not_load_and_kokoro_refuses_core_ml_first() {
    let code = |result: Result<Box<dyn LoadedModel>>| result.map(|_| ()).unwrap_err().code;
    assert_eq!(
        code(load(Accelerator::Cpu, &[("encoder", "e.onnx")])),
        "file-not-installed"
    );
    assert_eq!(
        code(load(Accelerator::Cpu, &[("voices", "v.bin")])),
        "file-not-installed"
    );
    let kokoro = [("model", "m.onnx"), ("voices", "v.bin")];
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
