//! The bridge to JavaScript: the engine as the web sees it, `WebEngine.create(host)`, where `host` is any object
//! with the methods of [`JsHost`]. The hosts themselves live with each platform, not here. Only in the wasm32 build
//! (the npm package).
#![cfg(web)]

use crate::{
    async_trait, Accelerator, Capabilities, Engine, Error, Fetcher, Host, Offer, Rejection, Result,
    Runs, Storage, Task,
};
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

#[cfg(test)]
mod tests;

#[wasm_bindgen]
extern "C" {
    /// A JavaScript object that fulfils the engine's `Host` contract.
    pub type JsHost;

    /// `{ os, arch, accelerators: string[], memoryMb?: number, cores?: number }`
    #[wasm_bindgen(method, catch)]
    async fn capabilities(this: &JsHost) -> Result<JsValue, JsValue>;
}

#[wasm_bindgen]
pub struct WebEngine {
    engine: Engine,
}

#[wasm_bindgen]
impl WebEngine {
    /// Asks the host for its capabilities once, and builds the engine.
    pub async fn create(host: JsHost) -> Result<WebEngine, JsError> {
        let caps = host
            .capabilities()
            .await
            .map_err(|_| JsError::new("host-capabilities"))?;
        let host = WebHost {
            capabilities: capabilities(&caps)?,
        };
        let engine = Engine::new(Box::new(host), vec![])
            .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(WebEngine { engine })
    }

    /// The ids of the backends in this build.
    pub fn backends(&self) -> Vec<String> {
        self.engine
            .backends()
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    /// `task`: "stt" or "tts". One `{ model, build, offered, why? }` per offer or rejected build; `why` is a stable
    /// code the page translates.
    pub fn offers(&self, task: &str) -> Result<Array, JsError> {
        let task = match task {
            "stt" => Task::Stt,
            "tts" => Task::Tts,
            _ => return Err(JsError::new("unknown-task")),
        };
        let out = Array::new();
        for offer in self.engine.offers(task) {
            let entry = Object::new();
            let (model, build, why) = match &offer {
                Offer::Offered { model, build, .. } => (model, build, None),
                Offer::Rejected { model, build, why } => (model, build, Some(why)),
            };
            set(&entry, "model", &model.id.as_str().into());
            set(&entry, "build", &build.id.as_str().into());
            set(&entry, "offered", &why.is_none().into());
            if let Some(why) = why {
                let code = match why {
                    Rejection::BackendNotInThisBuild => "backend-not-in-this-build",
                    Rejection::BackendUnavailable(reason) | Rejection::DoesNotFit(reason) => {
                        reason.code
                    }
                };
                set(&entry, "why", &code.into());
            }
            out.push(&entry);
        }
        Ok(out)
    }
}

/// The JavaScript host as the engine sees it. Storage and downloads are not bridged yet.
struct WebHost {
    capabilities: Capabilities,
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

fn set(object: &Object, key: &str, value: &JsValue) {
    Reflect::set(object, &key.into(), value).expect("setting a property of a plain object");
}
