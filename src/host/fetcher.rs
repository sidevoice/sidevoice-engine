//! How a host downloads: a URL in, its bytes out, a part at a time. The fetcher only moves bytes: checking them and
//! storing them is the installer's, so every host gets digests, progress and cancellation alike.

use async_trait::async_trait;

use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

/// Where files are downloaded from. Implemented with [`async_trait`](crate::async_trait), as [`Host`](crate::Host)
/// shows. Errors are stable codes: `download-failed` when the file cannot be had (no network, an HTTP error status).
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Fetcher: MaybeSend + MaybeSync {
    /// Starts downloading `url`, following redirects; resolves once the server has answered.
    async fn fetch(&self, url: &str) -> Result<Box<dyn Download>>;
}

/// A download under way. Dropping it stops it.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Download: MaybeSend {
    /// Its size in bytes, when the server says.
    fn size(&self) -> Option<u64>;

    /// The next bytes, never empty, or `None` once all have arrived.
    async fn chunk(&mut self) -> Result<Option<Vec<u8>>>;
}
