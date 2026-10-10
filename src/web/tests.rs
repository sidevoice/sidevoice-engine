use js_sys::{Array, Function, Object, Promise, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;

use super::{JsHost, WebEngine};

async fn engine() -> WebEngine {
    let caps =
        js_sys::JSON::parse(r#"{"os":"web","arch":"wasm32","accelerators":["wasm"]}"#).unwrap();
    let host = Object::new();
    let capabilities = Function::new_with_args("", "return this.caps");
    Reflect::set(&host, &"caps".into(), &Promise::resolve(&caps)).unwrap();
    Reflect::set(&host, &"capabilities".into(), &capabilities).unwrap();
    WebEngine::create(JsValue::from(host).unchecked_into::<JsHost>())
        .await
        .map_err(|_| "no engine")
        .unwrap()
}

fn get(value: &JsValue, key: &str) -> JsValue {
    Reflect::get(value, &key.into()).unwrap()
}

/// The code a promise rejected with.
async fn rejection(promise: Promise) -> String {
    let error = JsFuture::from(promise).await.expect_err("rejected");
    assert!(error.is_instance_of::<js_sys::Error>(), "an Error");
    assert!(get(&error, "params").is_object(), "with its params");
    get(&error, "code").as_string().expect("a code")
}

#[wasm_bindgen_test]
async fn the_local_catalogue_lists_every_model_with_its_web_builds_first() {
    let engine = engine().await;
    assert_eq!(engine.backends(), ["transformers-js"]);
    // Whether a model is installed is asked of storage: without OPFS (Node), there is no answer.
    if crate::web::opfs::root().await.is_err() {
        assert_eq!(
            rejection(engine.local_catalog().models(None)).await,
            "storage-failed"
        );
        return;
    }
    let models: Array = JsFuture::from(engine.local_catalog().models(None))
        .await
        .unwrap()
        .into();
    let whisper = models
        .iter()
        .find(|model| get(model, "id").as_string().as_deref() == Some("whisper-tiny"))
        .expect("whisper-tiny");
    assert_eq!(get(&whisper, "installed"), JsValue::FALSE);
    assert_eq!(get(&whisper, "parametersM").as_f64(), Some(39.0));
    assert_eq!(
        get(&whisper, "family").as_string().as_deref(),
        Some("whisper")
    );
    let builds: Array = get(&whisper, "builds").into();
    let first = builds.get(0);
    assert_eq!(
        get(&first, "backend").as_string().as_deref(),
        Some("transformers-js")
    );
    assert_eq!(
        get(&first, "accelerator").as_string().as_deref(),
        Some("wasm")
    );
    assert_eq!(get(&first, "available"), JsValue::TRUE);
    assert_eq!(get(&whisper, "recommendedBuild"), get(&first, "id"));
    // The others say why not, by code.
    let last = builds.get(builds.length() - 1);
    assert_eq!(get(&last, "available"), JsValue::FALSE);
    let reason = Array::from(&get(&last, "reasons")).get(0);
    assert!(get(&reason, "code")
        .as_string()
        .is_some_and(|code| !code.is_empty()));
}

#[wasm_bindgen_test]
async fn what_the_engine_refuses_rejects_with_its_code() {
    let engine = engine().await;
    let unknown = engine.install("no-such-model".into(), None, None, None);
    assert_eq!(rejection(unknown).await, "model-not-found");
    let unknown = engine.load(
        "whisper-tiny".into(),
        Some("no-such-build".into()),
        None,
        None,
    );
    assert_eq!(rejection(unknown).await, "build-not-found");
    assert_eq!(
        rejection(engine.uninstall("no-such-model".into(), None)).await,
        "model-not-found"
    );
}

#[wasm_bindgen_test]
async fn an_aborted_signal_cancels_before_anything_starts() {
    let engine = engine().await;
    let controller = web_sys::AbortController::new().unwrap();
    controller.abort();
    let installing = engine.install("whisper-tiny".into(), None, None, Some(controller.signal()));
    assert_eq!(rejection(installing).await, "cancelled");
}

#[wasm_bindgen_test]
async fn a_host_with_malformed_capabilities_rejects_with_the_field() {
    let host = Object::new();
    let capabilities =
        Function::new_with_args("", "return Promise.resolve({os: 'web', accelerators: []})");
    Reflect::set(&host, &"capabilities".into(), &capabilities).unwrap();
    let created = WebEngine::create(JsValue::from(host).unchecked_into::<JsHost>()).await;
    let error = created.err().expect("rejected");
    assert_eq!(
        get(&error, "code").as_string().as_deref(),
        Some("host-capabilities-arch")
    );
}
