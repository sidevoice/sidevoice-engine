//! The native host's downloads and API calls, over HTTP(S), with `reqwest`: the HTTP client sidevoice-core and the
//! desktop app use.
//! The body is streamed as it arrives, so a slow disk or consumer slows the download instead of filling memory, and
//! dropping the download closes the connection. `reqwest` needs a Tokio runtime: so do these futures.

use std::fmt;
use std::pin::Pin;
use std::time::Duration;

use futures_core::Stream;

use crate::{async_trait, Download, Error, Fetcher, HttpClient, HttpRequest, HttpResponse, Result};

/// Downloads over HTTP(S), following redirects, where an HTTP error status fails; and API calls, whose status is the
/// caller's to read.
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
            offset: 0,
            body: Box::pin(response.bytes_stream()),
        }))
    }

    async fn fetch_from(&self, url: &str, offset: u64) -> Result<Box<dyn Download>> {
        if offset == 0 {
            return self.fetch(url).await;
        }
        let response = self
            .client
            .get(url)
            .header(reqwest::header::RANGE, format!("bytes={offset}-"))
            .send()
            .await
            .map_err(|_| failed())?;
        match response.status() {
            reqwest::StatusCode::PARTIAL_CONTENT => {
                // Only what was asked for: the range must start where the bytes kept end.
                let start = response
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(range_start);
                if start != Some(offset) {
                    return Err(failed());
                }
                Ok(Box::new(HttpDownload {
                    size: response.content_length(),
                    offset,
                    body: Box::pin(response.bytes_stream()),
                }))
            }
            // The server ignores ranges: the whole file, from the start.
            status if status.is_success() => Ok(Box::new(HttpDownload {
                size: response.content_length(),
                offset: 0,
                body: Box::pin(response.bytes_stream()),
            })),
            // The range is past the end of the file (what was kept is all of it, or more): the whole file.
            reqwest::StatusCode::RANGE_NOT_SATISFIABLE => self.fetch(url).await,
            _ => Err(failed()),
        }
    }
}

/// A download under way: its body, as it arrives.
struct HttpDownload<S> {
    size: Option<u64>,
    offset: u64,
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

    fn offset(&self) -> u64 {
        self.offset
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

/// The first byte of a `Content-Range` value (`bytes 100-199/200`).
fn range_start(value: &str) -> Option<u64> {
    let range = value.strip_prefix("bytes ")?;
    range.split('-').next()?.trim().parse().ok()
}

fn failed() -> Error {
    Error::new("download-failed")
}

#[async_trait]
impl HttpClient for Http {
    /// The whole body is read before it resolves: API answers are small (a transcript, an utterance's audio).
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse> {
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|_| Error::new("request-failed"))?;
        let mut builder = self.client.request(method, &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let response = builder
            .body(request.body)
            .send()
            .await
            .map_err(|_| Error::new("request-failed"))?;
        let status = response.status().as_u16();
        let body = response
            .bytes()
            .await
            .map_err(|_| Error::new("request-failed"))?;
        Ok(HttpResponse {
            status,
            body: body.to_vec(),
        })
    }
}
