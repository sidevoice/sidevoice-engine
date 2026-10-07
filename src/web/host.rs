//! The JavaScript host as the engine sees it: any JavaScript object with the methods of [`JsHost`], wrapped as a
//! [`Host`]. Storage and downloads are not bridged yet.
//!
//! Inside: `capabilities` (reading what the JavaScript host reports), `storage` and `fetcher` (its storage and
//! downloads, not bridged yet).

use wasm_bindgen::prelude::*;

use crate::{Capabilities, Fetcher, Host, Storage};

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
        &WebStorage
    }

    fn fetcher(&self) -> &dyn Fetcher {
        &WebFetcher
    }
}
