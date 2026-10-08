use super::kokoro::stretches;
use super::{device, voices, Kind};
use crate::install::Installed;
use crate::Accelerator;
use wasm_bindgen_test::wasm_bindgen_test;

fn installed(keys: &[&str]) -> Installed {
    let mut installed = Installed::default();
    for key in keys {
        installed
            .files
            .insert((*key).to_owned(), format!("sidevoice-engine/{key}"));
    }
    installed
}

#[wasm_bindgen_test]
fn a_build_is_the_model_its_files_say() {
    let whisper = ["encoder", "decoder", "config", "tokenizer"];
    assert_eq!(Kind::of(&installed(&whisper)), Ok(Kind::Whisper));
    let kokoro = ["model", "config", "tokenizer", "voices/ef_dora"];
    assert_eq!(Kind::of(&installed(&kokoro)), Ok(Kind::Kokoro));
    let supertonic = [
        "text_encoder",
        "latent_denoiser",
        "voice_decoder",
        "voices/F1",
    ];
    assert_eq!(Kind::of(&installed(&supertonic)), Ok(Kind::Supertonic));
    for unknown in [&["model"][..], &["encoder"], &[]] {
        assert_eq!(
            Kind::of(&installed(unknown)).map_err(|e| e.code),
            Err("unsupported-model")
        );
    }
}

#[wasm_bindgen_test]
fn voices_are_the_keys_under_voices() {
    let files = installed(&["model", "voices/ef_dora", "voices/af_heart", "config"]);
    assert_eq!(voices(&files), ["af_heart", "ef_dora"]);
}

#[wasm_bindgen_test]
fn it_runs_on_webgpu_and_wasm_only() {
    assert_eq!(device(Accelerator::WebGpu), Ok("webgpu"));
    assert_eq!(device(Accelerator::Wasm), Ok("wasm"));
    assert_eq!(
        device(Accelerator::Cpu).map_err(|e| e.code),
        Err("unsupported-accelerator")
    );
}

#[wasm_bindgen_test]
fn long_phonemes_are_cut_after_punctuation_then_between_words() {
    assert_eq!(stretches("ola. kˈe tal", 100), ["ola. kˈe tal"]);
    assert_eq!(stretches("ola. kˈe tal", 8), ["ola.", "kˈe tal"]);
    assert_eq!(stretches("abc def ghi", 5), ["abc", "def", "ghi"]);
    assert_eq!(stretches("abcdefgh", 3), ["abc", "def", "gh"]);
    assert!(stretches("   ", 3).is_empty());
    for stretch in stretches(&"ˈa ".repeat(400), 509) {
        assert!(stretch.chars().count() <= 509);
    }
}
