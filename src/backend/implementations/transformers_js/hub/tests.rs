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

/// ENG-03: two engines' libraries on one transformers.js module share its hub, so they never answer each other's
/// models, and `env` is put back only when the last of them is closed.
#[wasm_bindgen_test]
fn libraries_on_one_module_share_its_hub_and_env_comes_back_after_the_last() {
    use std::rc::Rc;

    use js_sys::{Function, Object, Reflect};
    use wasm_bindgen::JsValue;

    use super::Hub;

    let env = Object::new();
    let fetch = Function::new_no_args("return null;");
    Reflect::set(&env, &"fetch".into(), &fetch).unwrap();
    let module = Object::new();
    Reflect::set(&module, &"env".into(), &env).unwrap();
    let module: JsValue = module.into();
    let fetch_now = || Reflect::get(&env, &"fetch".into()).unwrap();

    let first = Hub::open(&module).expect("opened");
    let second = Hub::open(&module).expect("opened");
    assert!(Rc::ptr_eq(&first, &second), "one hub per module");
    assert!(!Object::is(&fetch_now(), &fetch), "env points at the hub");
    first.models.borrow_mut().next += 1;
    assert_eq!(second.models.borrow().next, 1, "one numbering for both");

    drop(first);
    assert!(!Object::is(&fetch_now(), &fetch), "still used by the other");
    drop(second);
    assert!(Object::is(&fetch_now(), &fetch), "put back after the last");

    let again = Hub::open(&module).expect("opened");
    assert!(
        !Object::is(&fetch_now(), &fetch),
        "a new hub points it again"
    );
    drop(again);
    assert!(Object::is(&fetch_now(), &fetch));
}
