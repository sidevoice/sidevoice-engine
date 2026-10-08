//! What sherpa-onnx's backend checks before the library is asked anything: which family a build's files make, which
//! config fields its keys fill (and that the bundled catalogue names only those), which accelerators it takes, how it
//! fails without its files, and the ONNX metadata reader. Running real models is
//! `inference_tests.rs`.

use std::collections::BTreeSet;
use std::io::{BufReader, Cursor};

use sherpa_onnx::{OfflineModelConfig, OfflineTtsModelConfig};

use super::synthesizer::espeak_voice;
use super::{config, model_metadata, provider, text, Kind, SherpaOnnx, SPEC};
use crate::backend::{Backend, LoadedModel};
use crate::catalog::{BundledCatalog, Capability, CatalogSource};
use crate::host::Accelerator;
use crate::install::Installed;
use crate::test_support::{build, ready};
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
fn what_a_build_is_follows_from_its_files_keys() {
    assert_eq!(kind(&[("whisper.encoder", "e.onnx")]), Ok(Kind::Stt));
    assert_eq!(kind(&[("kokoro.voices", "v.bin")]), Ok(Kind::Tts));
    assert_eq!(kind(&[]).unwrap_err().code, "unsupported-model");
    assert_eq!(
        kind(&[("tokens", "t.txt"), ("kokoro.model", "m.onnx")])
            .unwrap_err()
            .code,
        "unsupported-model",
        "a recognizer's field and a TTS's"
    );
}

/// Every file key of every sherpa-onnx build in the bundled catalogue names a field of its capability's config: a key
/// sherpa-onnx does not take fails here, when the catalogue is checked, and never reaches a load.
#[test]
fn every_sherpa_onnx_build_in_the_catalogue_names_config_fields_its_family_takes() {
    let catalogue = BundledCatalog.load().expect("the bundled catalogue");
    let mut checked = 0;
    for model in catalogue.families.iter().flat_map(|family| &family.models) {
        for build in model.builds.iter().filter(|b| b.backend == "sherpa-onnx") {
            for file in &build.files {
                let known = if model.capabilities.contains(&Capability::Stt) {
                    config::stt_field(&mut OfflineModelConfig::default(), &file.key).is_some()
                } else {
                    config::tts_field(&mut OfflineTtsModelConfig::default(), &file.key).is_some()
                };
                assert!(known, "{}: no sherpa-onnx field {}", build.id, file.key);
            }
            let files = Installed {
                files: build
                    .files
                    .iter()
                    .map(|file| (file.key.clone(), String::new()))
                    .collect(),
            };
            assert!(Kind::of(&files).is_ok(), "{}: no family", build.id);
            checked += 1;
        }
    }
    assert!(checked > 0, "no sherpa-onnx build");
}

#[test]
fn a_key_names_the_config_field_it_fills_and_an_unknown_one_is_refused() {
    let files = installed(&[
        ("whisper.encoder", "e.onnx"),
        ("whisper.decoder", "d.onnx"),
        ("tokens", "t.txt"),
    ]);
    let stt = config::recognizer(&files, "cpu").expect("a recognizer config");
    assert_eq!(stt.model_config.whisper.encoder.as_deref(), Some("e.onnx"));
    assert_eq!(stt.model_config.whisper.decoder.as_deref(), Some("d.onnx"));
    assert_eq!(stt.model_config.tokens.as_deref(), Some("t.txt"));
    assert_eq!(stt.model_config.provider.as_deref(), Some("cpu"));
    // Everything else is sherpa-onnx's default.
    assert_eq!(stt.decoding_method, None);
    assert_eq!(stt.feat_config.feature_dim, 80);

    let files = installed(&[("kokoro.model", "m.onnx"), ("kokoro.data_dir", "espeak")]);
    let tts = config::tts(&files, "cpu").expect("a TTS config");
    assert_eq!(tts.model.kokoro.model.as_deref(), Some("m.onnx"));
    assert_eq!(tts.model.kokoro.data_dir.as_deref(), Some("espeak"));

    let code = |result: Result<()>| result.unwrap_err().code;
    let tokens = installed(&[("tokens", "t.txt")]);
    assert_eq!(
        code(config::tts(&tokens, "cpu").map(drop)),
        "unsupported-model",
        "an STT path is not a TTS one"
    );
    let typo = installed(&[("whisper.encodr", "e.onnx")]);
    assert_eq!(
        code(config::recognizer(&typo, "cpu").map(drop)),
        "unsupported-model"
    );
}

#[test]
fn a_language_reaches_espeak_ng_as_its_voice_or_its_primary_subtag() {
    let voices: BTreeSet<String> = ["en", "en-us", "en-gb", "es", "es-419", "pt-br"]
        .map(str::to_owned)
        .into();
    assert_eq!(espeak_voice("en-US", &voices), "en-us");
    assert_eq!(espeak_voice("en_GB", &voices), "en-gb");
    assert_eq!(espeak_voice("pt-BR", &voices), "pt-br");
    assert_eq!(espeak_voice("es-ES", &voices), "es", "no es-es voice");
    assert_eq!(espeak_voice("es", &voices), "es");
    assert_eq!(espeak_voice("fr-CA", &BTreeSet::new()), "fr");
}

fn load(accelerator: Accelerator, files: &[(&str, &str)]) -> Result<Box<dyn LoadedModel>> {
    let files = installed(files);
    let library = ready(SherpaOnnx.open(&files))?;
    ready(library.load(&build("test", "sherpa-onnx", 0), accelerator, &files))
}

#[test]
fn a_model_missing_a_file_does_not_load() {
    let code = |result: Result<Box<dyn LoadedModel>>| result.map(|_| ()).unwrap_err().code;
    // sherpa-onnx checks its config before it creates anything, and refuses one whose files are not there.
    assert_eq!(
        code(load(Accelerator::Cpu, &[("whisper.encoder", "e.onnx")])),
        "model-load-failed"
    );
    assert_eq!(
        code(load(Accelerator::Cpu, &[("kokoro.voices", "v.bin")])),
        "model-load-failed"
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
