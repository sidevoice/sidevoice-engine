use super::{JsHost, WebEngine};
use js_sys::{Function, Object, Promise, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
async fn a_web_engine_on_a_js_host_has_the_web_backends_and_the_bundled_catalogue() {
    let caps =
        js_sys::JSON::parse(r#"{"os":"web","arch":"wasm32","accelerators":["wasm"]}"#).unwrap();
    let host = Object::new();
    let capabilities = Function::new_with_args("", "return this.caps");
    Reflect::set(&host, &"caps".into(), &Promise::resolve(&caps)).unwrap();
    Reflect::set(&host, &"capabilities".into(), &capabilities).unwrap();

    let engine = WebEngine::create(JsValue::from(host).unchecked_into::<JsHost>())
        .await
        .expect("engine");
    assert_eq!(engine.backends(), ["transformers-js"]);
    // Each bundled Whisper model is offered once, on transformers.js, and every other build comes back with why not.
    // Large-v3 too, through its fp16 build: that build declares only WebGPU features, which the resolver does not check
    // yet, and no accelerator, so WebAssembly takes it (its q8 build is over its cap, `wasm-memory`).
    let offered = engine
        .offers("stt")
        .expect("offers")
        .iter()
        .filter(|entry| Reflect::get(entry, &"offered".into()).ok() == Some(JsValue::TRUE))
        .count();
    assert_eq!(offered, 5);
    assert!(engine.offers("llm").is_err());
}
