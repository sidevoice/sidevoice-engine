use super::{repository_path, Models};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn a_files_repository_path_is_what_follows_its_revision() {
    let url = "https://huggingface.co/onnx-community/whisper-tiny/resolve/ff41770/onnx/encoder_model_quantized.onnx";
    assert_eq!(
        repository_path(url),
        Some("onnx/encoder_model_quantized.onnx")
    );
    let url = "https://huggingface.co/onnx-community/whisper-tiny/resolve/ff41770/config.json";
    assert_eq!(repository_path(url), Some("config.json"));
    for url in [
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro.tar.bz2",
        "https://huggingface.co/onnx-community/whisper-tiny/resolve/ff41770/",
    ] {
        assert_eq!(repository_path(url), None, "{url}");
    }
}

#[wasm_bindgen_test]
fn a_served_model_finds_its_files_by_local_path_and_by_url_and_nothing_else() {
    let mut models = Models::default();
    let files = [
        ("config.json", "sidevoice-engine/aa"),
        ("onnx/model.onnx", "sidevoice-engine/bb"),
    ];
    let files = files
        .iter()
        .map(|(path, at)| ((*path).to_owned(), (*at).to_owned()));
    models.files.insert("m1".to_owned(), files.collect());

    // transformers.js asks by its local path, then by the URL it would download.
    assert_eq!(
        models.location("m1/config.json").as_deref(),
        Some("sidevoice-engine/aa")
    );
    assert_eq!(
        models
            .location("m1/resolve/main/onnx/model.onnx")
            .as_deref(),
        Some("sidevoice-engine/bb")
    );
    // A file at the root, asked for with an empty subfolder.
    assert_eq!(
        models.location("m1//config.json").as_deref(),
        Some("sidevoice-engine/aa")
    );
    assert_eq!(
        models.location("m1/resolve/main//config.json").as_deref(),
        Some("sidevoice-engine/aa")
    );
    // A file the build does not have, another model's name, or no path: nothing.
    for path in ["m1/generation_config.json", "m2/config.json", "m1", ""] {
        assert_eq!(models.location(path), None, "{path:?}");
    }
}
