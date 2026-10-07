//! The JavaScript host as the engine sees it: any JavaScript object with the methods of [`JsHost`], wrapped as a
//! [`Host`]. Storage and downloads are not bridged yet.
//!
//! Inside: `capabilities`, reading what the JavaScript host reports.

use wasm_bindgen::prelude::*;

use crate::{async_trait, Capabilities, Error, Fetcher, Host, Result, Storage};

mod capabilities;

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

/// The JavaScript host as the engine sees it. Storage and downloads are not bridged yet.
pub(super) struct WebHost {
    capabilities: Capabilities,
}

impl WebHost {
    /// The host whose capabilities are `value`, what the JavaScript host's `capabilities()` resolved to.
    pub(super) fn from_capabilities(value: &JsValue) -> Result<Self, JsError> {
        Ok(Self {
            capabilities: capabilities::read(value)?,
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

/// The storage and downloads of a JavaScript host, not bridged yet: every call fails with `not-implemented`.
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
