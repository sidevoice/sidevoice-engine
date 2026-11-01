//! How a host downloads: a URL in, its bytes out, a part at a time. The fetcher only moves bytes: checking them and
//! storing them is the installer's, so every host gets digests, progress and cancellation alike.

use async_trait::async_trait;

use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

/// Where files are downloaded from. Implemented with [`async_trait`](crate::async_trait), as [`Host`](crate::Host)
/// shows. Errors are stable codes: `download-failed` when the file cannot be had (no network, an HTTP error status, no bytes for a minute).
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Fetcher: MaybeSend + MaybeSync {
    /// Starts downloading `url`, following redirects; resolves once the server has answered.
    async fn fetch(&self, url: &str) -> Result<Box<dyn Download>>;

    /// Starts downloading `url` from byte `offset` on, when the server can serve it (an HTTP range request), so that
    /// a download interrupted earlier goes on where it stopped. The [`Download`] says where it really starts
    /// ([`Download::offset`]): `offset`, or `0` when the server sends the whole file. The default never resumes.
    async fn fetch_from(&self, url: &str, _offset: u64) -> Result<Box<dyn Download>> {
        self.fetch(url).await
    }
}

/// Bytes arriving a part at a time: a download under way, or a stored file being read
/// ([`Storage::read`](crate::Storage::read)). Dropping it stops it.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Download: MaybeSend {
    /// The bytes it hands over, in all, when known (a server may not say): from [`Download::offset`] on.
    fn size(&self) -> Option<u64>;

    /// Where in the file its first byte is: `0`, unless it resumes ([`Fetcher::fetch_from`]).
    fn offset(&self) -> u64 {
        0
    }

    /// The next bytes, never empty, or `None` once all have arrived.
    async fn chunk(&mut self) -> Result<Option<Vec<u8>>>;
}
