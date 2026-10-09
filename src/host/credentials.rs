//! The keys a remote backend calls its provider with. The engine never stores one: the host hands it over when a remote
//! model is installed (to know it can be), loaded and called, from wherever the app keeps it (the OS keychain on
//! desktop, the browser's storage on the web).

use async_trait::async_trait;

use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

/// Where the host keeps the keys of remote providers. Implemented with [`async_trait`](crate::async_trait), as
/// [`Host`](crate::Host) shows.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Credentials: MaybeSend + MaybeSync {
    /// The key for `provider` (`"openai"`, `"elevenlabs"`: a remote backend's id), or `None` when the app has none.
    /// It is asked for every time it is needed, so a key the person changes takes effect at once; the engine keeps it
    /// only for the call it is made for.
    ///
    /// # Errors
    ///
    /// When the store cannot be read (`credentials-failed`): the call that needed the key fails with it.
    async fn credential(&self, provider: &str) -> Result<Option<String>>;
}

/// No keys at all: a host whose app uses local models only.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCredentials;

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Credentials for NoCredentials {
    async fn credential(&self, _provider: &str) -> Result<Option<String>> {
        Ok(None)
    }
}
