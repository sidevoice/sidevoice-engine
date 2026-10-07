//! The JavaScript host as the engine sees it: any JavaScript object with the methods of [`JsHost`], wrapped as a
//! [`Host`]. Storage and downloads are not bridged yet.

use js_sys::{Array, Reflect};
use wasm_bindgen::prelude::*;

use crate::{async_trait, Accelerator, Capabilities, Error, Fetcher, Host, Result, Runs, Storage};

#[wasm_bindgen]
extern "C" {
    /// A JavaScript object that fulfils the engine's `Host` contract.
    pub type JsHost;

    /// `{ os, arch, accelerators: string[], memoryMb?: number, cores?: number }`
    #[wasm_bindgen(method, catch)]
    pub(super) async fn capabilities(this: &JsHost) -> Result<JsValue, JsValue>;
}

/// The JavaScript host as the engine sees it. Storage and downloads are not bridged yet.
pub(super) struct WebHost {
    capabilities: Capabilities,
}

impl WebHost {
    /// The host whose capabilities are `value`, what the JavaScript host's `capabilities()` resolved to.
    pub(super) fn from_capabilities(value: &JsValue) -> Result<Self, JsError> {
        Ok(Self {
            capabilities: capabilities(value)?,
        })
    }
}

impl Host for WebHost {
    fn capabilities(&self) -> Capabilities {
        self.capabilities.clone()
    }

    fn storage(&self) -> &dyn Storage {
        &Unimplemented
    }

    fn fetcher(&self) -> &dyn Fetcher {
        &Unimplemented
    }
}

struct Unimplemented;

#[async_trait(?Send)]
impl Storage for Unimplemented {
    async fn contains(&self, _key: &str) -> Result<bool> {
        Err(Error::new("not-implemented"))
    }
}

#[async_trait(?Send)]
impl Fetcher for Unimplemented {
    async fn fetch(&self, _url: &str, _sha256: &str, _key: &str) -> Result<()> {
        Err(Error::new("not-implemented"))
    }
}

fn capabilities(value: &JsValue) -> Result<Capabilities, JsError> {
    let text = |key| {
        get(value, key)
            .as_string()
            .ok_or_else(|| JsError::new(&format!("host-capabilities-{key}")))
    };
    let accelerators = Array::from(&get(value, "accelerators"))
        .iter()
        .filter_map(|name| match name.as_string()?.as_str() {
            "cpu" => Some(Accelerator::Cpu),
            "webgpu" => Some(Accelerator::WebGpu),
            "wasm" => Some(Accelerator::Wasm),
            _ => None,
        })
        .collect();
    Ok(Capabilities {
        runs: Runs::Page,
        os: text("os")?,
        arch: text("arch")?,
        accelerators,
        memory_mb: get(value, "memoryMb").as_f64().map(|mb| mb as u32),
        cores: get(value, "cores").as_f64().map(|cores| cores as u32),
    })
}

fn get(object: &JsValue, key: &str) -> JsValue {
    Reflect::get(object, &key.into()).unwrap_or(JsValue::UNDEFINED)
}
