//! What a JavaScript host's `capabilities()` resolved to, read into [`Capabilities`], strictly: a field that is
//! missing or malformed, or an accelerator id this engine does not know, fails with `host-capabilities-<field>`.
//! `memoryMb` and `cores` may be absent (`undefined` or `null`), which is unknown.

use js_sys::{Array, Reflect};
use wasm_bindgen::prelude::*;

use crate::{Accelerator, Capabilities, Runs};

#[cfg(test)]
mod tests;

/// The capabilities in `value`, or the field that is wrong.
pub(super) fn read(value: &JsValue) -> Result<Capabilities, JsError> {
    let text = |key| get(value, key).as_string().ok_or_else(|| invalid(key));
    let accelerators = get(value, "accelerators");
    if !Array::is_array(&accelerators) {
        return Err(invalid("accelerators"));
    }
    let accelerators = Array::from(&accelerators)
        .iter()
        .map(|id| {
            id.as_string()
                .and_then(|id| accelerator(&id))
                .ok_or_else(|| invalid("accelerators"))
        })
        .collect::<Result<_, _>>()?;
    Ok(Capabilities {
        runs: Runs::Page,
        os: text("os")?,
        arch: text("arch")?,
        accelerators,
        memory_mb: optional_count(value, "memoryMb")?,
        cores: optional_count(value, "cores")?,
    })
}

/// The accelerator a JavaScript host names `id`; `None` for any other id.
fn accelerator(id: &str) -> Option<Accelerator> {
    Some(match id {
        "cpu" => Accelerator::Cpu,
        "cuda" => Accelerator::Cuda,
        "coreml" => Accelerator::CoreMl,
        "metal" => Accelerator::Metal,
        "webgpu" => Accelerator::WebGpu,
        "wasm" => Accelerator::Wasm,
        _ => return None,
    })
}

/// A positive whole number, or `None` when the field is absent; anything else is malformed.
fn optional_count(object: &JsValue, key: &str) -> Result<Option<u32>, JsError> {
    let value = get(object, key);
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    match value.as_f64() {
        Some(number) if (1.0..=f64::from(u32::MAX)).contains(&number) && number.fract() == 0.0 => {
            Ok(Some(number as u32))
        }
        _ => Err(invalid(key)),
    }
}

fn invalid(key: &str) -> JsError {
    JsError::new(&format!("host-capabilities-{key}"))
}

fn get(object: &JsValue, key: &str) -> JsValue {
    Reflect::get(object, &key.into()).unwrap_or(JsValue::UNDEFINED)
}
