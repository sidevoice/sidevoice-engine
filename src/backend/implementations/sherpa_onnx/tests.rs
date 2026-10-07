//! What sherpa-onnx's backend checks without its library: the C structs' layout, which model a build's files make
//! and which accelerators it takes, how opening fails, and the ONNX metadata reader. Running real models is
//! `inference_tests.rs`.

use std::io::{BufReader, Cursor};
use std::mem::{offset_of, size_of};

use super::c_api::{
    GeneratedAudio, GenerationConfig, KokoroModelConfig, OfflineModelConfig,
    OfflineRecognizerConfig, OfflineRecognizerResult, OfflineTtsConfig, OfflineTtsModelConfig,
    WhisperModelConfig,
};
use super::{model_metadata, provider, Kind, SherpaOnnx};
use crate::backend::Backend;
use crate::host::Accelerator;
use crate::install::Installed;
use crate::test_support::block_on;
use crate::Result;

/// Sizes and offsets from c-api.h (v1.13.8), printed by a C compiler with `sizeof` and `offsetof` on a 64-bit
/// target. If the header changes, these are measured again, and c_api.rs follows.
#[test]
fn the_c_structs_have_the_headers_layout() {
    assert_eq!(size_of::<WhisperModelConfig>(), 48);
    assert_eq!(size_of::<OfflineModelConfig>(), 504);
    assert_eq!(offset_of!(OfflineModelConfig, whisper), 40);
    assert_eq!(offset_of!(OfflineModelConfig, tokens), 96);
    assert_eq!(offset_of!(OfflineModelConfig, num_threads), 104);
    assert_eq!(offset_of!(OfflineModelConfig, provider), 112);
    assert_eq!(size_of::<OfflineRecognizerConfig>(), 608);
    assert_eq!(offset_of!(OfflineRecognizerConfig, model_config), 8);
    assert_eq!(offset_of!(OfflineRecognizerConfig, decoding_method), 528);
    assert_eq!(size_of::<OfflineRecognizerResult>(), 128);
    assert_eq!(size_of::<KokoroModelConfig>(), 64);
    assert_eq!(offset_of!(KokoroModelConfig, dict_dir), 40);
    assert_eq!(size_of::<OfflineTtsModelConfig>(), 416);
    assert_eq!(offset_of!(OfflineTtsModelConfig, num_threads), 56);
    assert_eq!(offset_of!(OfflineTtsModelConfig, provider), 64);
    assert_eq!(offset_of!(OfflineTtsModelConfig, kokoro), 128);
    assert_eq!(size_of::<OfflineTtsConfig>(), 448);
    assert_eq!(size_of::<GenerationConfig>(), 56);
    assert_eq!(offset_of!(GenerationConfig, sid), 8);
    assert_eq!(offset_of!(GenerationConfig, extra), 48);
    assert_eq!(size_of::<GeneratedAudio>(), 16);
}

#[test]
fn it_runs_on_core_ml_and_the_cpu_only() {
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

fn open(files: &[(&str, &str)]) -> &'static str {
    block_on(SherpaOnnx.open(&installed(files)))
        .map(|_| ())
        .unwrap_err()
        .code
}

#[test]
fn without_the_library_installed_nothing_opens() {
    assert_eq!(open(&[("encoder", "e.onnx")]), "file-not-installed");
}

#[test]
fn a_library_that_is_not_there_does_not_open() {
    let nowhere = std::env::temp_dir().join("sidevoice-sherpa-onnx-no-library");
    let nowhere = nowhere.to_str().expect("a UTF-8 temporary directory");
    assert_eq!(open(&[("library", nowhere)]), "library-open-failed");
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
