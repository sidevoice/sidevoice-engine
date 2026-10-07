//! The bridge to JavaScript: the engine as the web sees it, `WebEngine.create(host)`, where `host` is any object
//! with the methods of [`JsHost`]. The hosts themselves live with each platform, not here. Only in the wasm32 build
//! (the npm package).
//!
//! This file is the engine as JavaScript sees it ([`WebEngine`]); `host` is the JavaScript host as the engine sees it.

use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

use crate::{Engine, Offer, Rejection, Task};

mod host;
#[cfg(test)]
mod tests;

pub use host::JsHost;
use host::WebHost;

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
        let host = WebHost::from_capabilities(&caps)?;
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

fn set(object: &Object, key: &str, value: &JsValue) {
    Reflect::set(object, &key.into(), value).expect("setting a property of a plain object");
}
