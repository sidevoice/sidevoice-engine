//! What whisper.cpp's backend checks before a model runs: the accelerators it declares, what it asks whisper.cpp for on
//! each (the GPU and flash attention, or neither), how it fails without a model file or with one that is not a model,
//! and how a language reaches whisper.cpp. Running a real model is the voice loop of `cargo xtask e2e`.

use super::{context_params, language_id, WhisperCpp, SPEC};
use crate::backend::{Backend, BackendModel};
use crate::host::{Accelerator, AcceleratorClass};
use crate::install::Installed;
use crate::test_support::{block_on, build};
use crate::Result;

#[test]
fn it_declares_metal_first_on_apple_silicon_and_the_cpu_everywhere() {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        assert_eq!(SPEC.accelerators, [Accelerator::Metal, Accelerator::Cpu]);
    } else {
        assert_eq!(SPEC.accelerators, [Accelerator::Cpu]);
    }
}

fn load(accelerator: Accelerator, files: &[(&str, &str)]) -> Result<Box<dyn BackendModel>> {
    let files = Installed {
        files: files
            .iter()
            .map(|(key, path)| ((*key).to_owned(), (*path).to_owned()))
            .collect(),
    };
    let library = block_on(WhisperCpp.open(&Installed::default())).expect("the linked library");
    block_on(library.load(&build("test", "whisper-cpp", 0), accelerator, &files))
}

fn code(result: Result<Box<dyn BackendModel>>) -> &'static str {
    result.map(|_| ()).unwrap_err().code
}

#[test]
fn a_model_that_is_missing_or_is_not_a_ggml_file_does_not_load() {
    let cpu = Accelerator::Cpu;
    assert_eq!(code(load(cpu, &[])), "file-not-installed");
    assert_eq!(
        code(load(cpu, &[("model", "/no/such/model.bin")])),
        "model-load-failed"
    );
    let not_a_model = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    assert_eq!(
        code(load(cpu, &[("model", not_a_model)])),
        "model-load-failed"
    );
    assert_eq!(code(load(cpu, &[("model", "a\0b")])), "model-load-failed");
}

#[test]
fn it_refuses_an_accelerator_it_does_not_run_on_before_reading_the_model() {
    let model = [("model", "/no/such/model.bin")];
    for other in [
        Accelerator::Cuda,
        Accelerator::CoreMl,
        Accelerator::WebGpu,
        Accelerator::Wasm,
    ] {
        assert_eq!(
            code(load(other, &model)),
            "unsupported-accelerator",
            "{other:?}"
        );
    }
    let metal = code(load(Accelerator::Metal, &model));
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        assert_eq!(metal, "model-load-failed");
    } else {
        assert_eq!(metal, "unsupported-accelerator");
    }
}

#[test]
fn a_language_reaches_whisper_cpp_as_its_primary_subtag() {
    let spanish = whisper_rs::get_lang_id("es").expect("Whisper knows Spanish");
    assert_eq!(language_id("es"), Ok(spanish));
    assert_eq!(language_id("es-ES"), Ok(spanish));
    assert_eq!(language_id("ES_es"), Ok(spanish));
    for unknown in ["", "xx", "auto", "e\0s", "-ES", "1"] {
        assert_eq!(
            language_id(unknown).unwrap_err().code,
            "unsupported-language",
            "{unknown:?}"
        );
    }
}

/// On the GPU whisper.cpp runs with flash attention; on the CPU with neither the GPU nor flash attention. What it does
/// not run on is refused before any parameter is set.
#[test]
fn flash_attention_is_on_with_the_gpu_and_off_on_the_cpu() {
    let on =
        |accelerator| context_params(accelerator).map(|params| (params.use_gpu, params.flash_attn));
    assert_eq!(on(Accelerator::Cpu), Ok((false, false)));
    if cfg!(apple_silicon) {
        assert_eq!(on(Accelerator::Metal), Ok((true, true)));
    } else {
        assert_eq!(
            on(Accelerator::Metal).unwrap_err().code,
            "unsupported-accelerator"
        );
    }
    for other in [Accelerator::CoreMl, Accelerator::Wasm] {
        assert_eq!(
            on(other).unwrap_err().code,
            "unsupported-accelerator",
            "{other:?}"
        );
    }
}

/// Being a GPU is not enough: a GPU this build of the backend was not built for (CUDA and WebGPU, which whisper-rs is
/// not compiled with here) is refused, and is never asked for with the GPU or flash attention.
#[test]
fn a_gpu_the_backend_was_not_built_for_is_refused() {
    for gpu in [Accelerator::Cuda, Accelerator::WebGpu] {
        assert_eq!(gpu.class(), AcceleratorClass::Gpu, "{gpu:?}");
        assert!(!SPEC.accelerators.contains(&gpu), "{gpu:?}");
        assert_eq!(
            context_params(gpu).map(drop).unwrap_err().code,
            "unsupported-accelerator",
            "{gpu:?}"
        );
    }
}
