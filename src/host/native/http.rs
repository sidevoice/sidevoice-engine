//! The native host's downloads, over HTTP(S), with `reqwest`: the HTTP client sidevoice-core and the desktop app use.
//! The body is streamed as it arrives, so a slow disk or consumer slows the download instead of filling memory, and
//! dropping the download closes the connection. `reqwest` needs a Tokio runtime: so do these futures.

use std::fmt;
use std::pin::Pin;
use std::time::Duration;

use futures_core::Stream;

use crate::{async_trait, Download, Error, Fetcher, Result};

/// Downloads over HTTP(S), following redirects; an HTTP error status fails.
pub(super) struct Http {
    client: reqwest::Client,
}

impl fmt::Debug for Http {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Http").finish_non_exhaustive()
    }
}

impl Default for Http {
    fn default() -> Self {
        // Building fails only if TLS cannot be set up, which the build decides: a bug, not a condition.
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("an HTTP client");
        Self { client }
    }
}

#[async_trait]
impl Fetcher for Http {
    async fn fetch(&self, url: &str) -> Result<Box<dyn Download>> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| failed())?;
        Ok(Box::new(HttpDownload {
            size: response.content_length(),
            body: Box::pin(response.bytes_stream()),
        }))
    }
}

/// A download under way: its body, as it arrives.
struct HttpDownload<S> {
    size: Option<u64>,
    body: Pin<Box<S>>,
}

#[async_trait]
impl<S, B> Download for HttpDownload<S>
where
    S: Stream<Item = reqwest::Result<B>> + Send,
    B: AsRef<[u8]> + Into<Vec<u8>>,
{
    fn size(&self) -> Option<u64> {
        self.size
    }

    async fn chunk(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            match std::future::poll_fn(|cx| self.body.as_mut().poll_next(cx)).await {
                Some(Ok(bytes)) if bytes.as_ref().is_empty() => {}
                Some(Ok(bytes)) => return Ok(Some(bytes.into())),
                Some(Err(_)) => return Err(failed()),
                None => return Ok(None),
            }
        }
    }
}

fn failed() -> Error {
    Error::new("download-failed")
}
