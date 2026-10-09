//! How a host makes an API call: a request in, the whole response out. Remote backends call their providers through
//! it, so only a host touches the network (natively `reqwest`, on the web `fetch`), and tests answer for it. Streaming
//! is not part of it (sidevoice-engine#35).

use async_trait::async_trait;

use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

/// One HTTP request: its method, URL, headers and body.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// `"GET"`, `"POST"`.
    pub method: &'static str,
    /// The whole URL, query included.
    pub url: String,
    /// Each header's name and value, in order. They may hold a key: they are never logged.
    pub headers: Vec<(String, String)>,
    /// The body, empty for none.
    pub body: Vec<u8>,
}

impl std::fmt::Debug for HttpRequest {
    /// Without the headers' values, which may hold a key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let headers: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &headers)
            .field("body", &self.body.len())
            .finish()
    }
}

/// What the server answered: its status and its whole body. A status that is not a success is an answer, not an
/// error: what it means is the caller's to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// The HTTP status: 200, 401, ...
    pub status: u16,
    /// The body, whole.
    pub body: Vec<u8>,
}

/// Where API calls go out. Implemented with [`async_trait`](crate::async_trait), as [`Host`](crate::Host) shows.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait HttpClient: MaybeSend + MaybeSync {
    /// Sends `request`, following redirects, and resolves with the whole response, whatever its status.
    ///
    /// # Errors
    ///
    /// `request-failed` when there is no answer: no network, a refused connection, a timeout.
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse>;
}
