//! An app that uses `ort` itself, beside the engine: its `ort` works whether or not the engine's ONNX Runtime backend
//! has opened, before and after. This is its own process, so `ort` starts uninitialized here, as in an app that touches
//! `ort` before it ever loads an end-of-turn model. The engine installs no process-wide `ort` API: `ort` initializes
//! itself from the ONNX Runtime linked inside sherpa-onnx's libraries, as it would for any caller.

// The ONNX Runtime backend, and the runtime it uses, are native only.
#![cfg(native)]

use sidevoice_engine::{BundledCatalog, Engine, NativeHost};

#[test]
fn an_apps_own_ort_works_before_and_after_the_engine() {
    // Before anything of the engine runs.
    let before = ort::session::Session::builder();
    assert!(before.is_ok(), "ort before the engine: {:?}", before.err());

    let dir = std::env::temp_dir().join(format!("sidevoice-ort-alongside-{}", std::process::id()));
    let host = NativeHost::new(dir.clone()).expect("a host");
    let engine = Engine::new(Box::new(host), vec![Box::new(BundledCatalog)]).expect("an engine");
    let backends: Vec<_> = engine.backends().iter().map(|backend| backend.id).collect();
    assert!(backends.contains(&"onnxruntime"), "{backends:?}");

    let after = ort::session::Session::builder();
    assert!(after.is_ok(), "ort after the engine: {:?}", after.err());
    let _ = std::fs::remove_dir_all(dir);
}
