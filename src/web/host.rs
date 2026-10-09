//! The JavaScript host as the engine sees it: any JavaScript object with the methods of [`JsHost`], which reports the
//! page's capabilities and may hand over the keys of remote providers, wrapped as a [`Host`] with the web build's own
//! storage (OPFS), downloads and API calls (`fetch`).
//!
//! Inside: `capabilities` (reading what the JavaScript host reports), `storage` (`WebStorage`, files in OPFS) and
//! `fetcher` (`WebFetcher`, downloads and API calls through `fetch`).

use async_trait::async_trait;
use js_sys::{Function, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::{Capabilities, Credentials, Error, Fetcher, Host, HttpClient, Result, Storage};

mod capabilities;
mod fetcher;
mod storage;

use fetcher::WebFetcher;
use storage::WebStorage;

#[wasm_bindgen]
extern "C" {
    /// A JavaScript object that fulfils the engine's `Host` contract.
    pub type JsHost;

    /// `{ os, arch, accelerators: string[], memoryMb?: number, cores?: number }`: accelerators by their ids ("webgpu",
    /// "wasm", ...); `memoryMb` and `cores` left out when the page cannot tell. Checked strictly:
    /// anything malformed fails with `host-capabilities-<field>`.
    #[wasm_bindgen(method, catch)]
    pub(super) async fn capabilities(this: &JsHost) -> Result<JsValue, JsValue>;
}

/// The JavaScript host as the engine sees it: the capabilities it reported, files in OPFS, downloads and API calls
/// through `fetch`, and its keys.
pub(super) struct WebHost {
    capabilities: Capabilities,
    storage: WebStorage,
    credentials: JsCredentials,
}

impl WebHost {
    /// The host `host`, whose capabilities are `value`, what its `capabilities()` resolved to.
    pub(super) fn new(host: JsHost, value: &JsValue) -> Result<Self, JsError> {
        Ok(Self {
            capabilities: capabilities::read(value)?,
            storage: WebStorage::new(),
            credentials: JsCredentials(host),
        })
    }
}

impl Host for WebHost {
    fn capabilities(&self) -> Capabilities {
        self.capabilities.clone()
    }

    fn storage(&self) -> &dyn Storage {
        &self.storage
    }

    fn fetcher(&self) -> &dyn Fetcher {
        &WebFetcher
    }

    fn http(&self) -> &dyn HttpClient {
        &WebFetcher
    }

    fn credentials(&self) -> &dyn Credentials {
        &self.credentials
    }
}

/// The keys of the JavaScript host: its optional `credential(provider)`, which returns (or resolves to) the key as a
/// string, or `null` or `undefined` when the page has none. A host without the method has no keys.
struct JsCredentials(JsHost);

#[async_trait(?Send)]
impl Credentials for JsCredentials {
    /// Fails with `credentials-failed` when the method throws, rejects, or gives something that is not a string.
    async fn credential(&self, provider: &str) -> Result<Option<String>> {
        let failed = |cause: &JsValue| {
            web_sys::console::warn_2(
                &"sidevoice-engine: the host's credential failed:".into(),
                cause,
            );
            Error::new("credentials-failed")
        };
        let method = Reflect::get(&self.0, &"credential".into()).map_err(|e| failed(&e))?;
        let Some(method) = method.dyn_ref::<Function>() else {
            return Ok(None);
        };
        let mut value = method
            .call1(&self.0, &provider.into())
            .map_err(|e| failed(&e))?;
        if let Some(promise) = value.dyn_ref::<js_sys::Promise>() {
            value = JsFuture::from(promise.clone())
                .await
                .map_err(|e| failed(&e))?;
        }
        if value.is_null() || value.is_undefined() {
            return Ok(None);
        }
        let key = value
            .as_string()
            .ok_or_else(|| failed(&"not a string".into()))?;
        Ok(Some(key).filter(|key| !key.is_empty()))
    }
}
