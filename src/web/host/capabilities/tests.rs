use js_sys::Reflect;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

use super::{accelerator, read};
use crate::{Accelerator, Capabilities};

/// The capabilities a JavaScript host resolving to `json` gives, or the code they are rejected with.
fn read_json(json: &str) -> Result<Capabilities, String> {
    let value = js_sys::JSON::parse(json).unwrap();
    read(&value).map_err(|error| {
        Reflect::get(&JsValue::from(error), &"message".into())
            .unwrap()
            .as_string()
            .unwrap()
    })
}

#[wasm_bindgen_test]
fn every_accelerator_has_its_id_and_other_ids_name_none() {
    let named = [
        ("cpu", Accelerator::Cpu),
        ("cuda", Accelerator::Cuda),
        ("coreml", Accelerator::CoreMl),
        ("metal", Accelerator::Metal),
        ("webgpu", Accelerator::WebGpu),
        ("wasm", Accelerator::Wasm),
        ("remote", Accelerator::Remote),
    ];
    for (id, expected) in named {
        assert_eq!(accelerator(id), Some(expected));
    }
    // A new variant fails to compile here: give it an id, and list it above.
    for (_, expected) in named {
        match expected {
            Accelerator::Cpu
            | Accelerator::Cuda
            | Accelerator::CoreMl
            | Accelerator::Metal
            | Accelerator::WebGpu
            | Accelerator::Wasm
            | Accelerator::Remote => {}
        }
    }
    assert_eq!(accelerator("npu"), None);
    assert_eq!(accelerator("CPU"), None);
}

#[wasm_bindgen_test]
fn a_js_host_names_accelerators_by_id_and_may_leave_numbers_out() {
    let caps =
        read_json(r#"{"os":"web","arch":"wasm32","accelerators":["webgpu","wasm"]}"#).unwrap();
    assert_eq!(caps.accelerators, [Accelerator::WebGpu, Accelerator::Wasm]);
    assert_eq!((caps.memory_mb, caps.cores), (None, None));

    let caps =
        read_json(r#"{"os":"web","arch":"wasm32","accelerators":[],"memoryMb":4096,"cores":null}"#)
            .unwrap();
    assert_eq!((caps.memory_mb, caps.cores), (Some(4_096), None));
}

#[wasm_bindgen_test]
fn a_js_host_with_malformed_capabilities_is_rejected_with_the_field() {
    for (json, code) in [
        (
            r#"{"arch":"wasm32","accelerators":[]}"#,
            "host-capabilities-os",
        ),
        (
            r#"{"os":"web","arch":"wasm32"}"#,
            "host-capabilities-accelerators",
        ),
        (
            r#"{"os":"web","arch":"wasm32","accelerators":["npu"]}"#,
            "host-capabilities-accelerators",
        ),
        (
            r#"{"os":"web","arch":"wasm32","accelerators":[7]}"#,
            "host-capabilities-accelerators",
        ),
        (
            r#"{"os":"web","arch":"wasm32","accelerators":[],"memoryMb":-1}"#,
            "host-capabilities-memoryMb",
        ),
        (
            r#"{"os":"web","arch":"wasm32","accelerators":[],"memoryMb":"4096"}"#,
            "host-capabilities-memoryMb",
        ),
        (
            r#"{"os":"web","arch":"wasm32","accelerators":[],"cores":0}"#,
            "host-capabilities-cores",
        ),
        (
            r#"{"os":"web","arch":"wasm32","accelerators":[],"cores":2.5}"#,
            "host-capabilities-cores",
        ),
    ] {
        assert_eq!(read_json(json).map(|_| ()), Err(code.to_owned()), "{json}");
    }
}
