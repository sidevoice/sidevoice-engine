//! The web build's downloads: the page's `fetch`, its body read as a stream a part at a time, stopped through an
//! `AbortController` when the download is dropped. And what reads any such stream, a download or a stored file
//! ([`StreamDownload`]).

use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, Blob, ReadableStream, ReadableStreamDefaultReader, Response};

use crate::{async_trait, Download, Error, Fetcher, Result};

#[cfg(test)]
mod tests;

/// Downloads through the global `fetch` (a window's, a worker's or Node's), following redirects. Fails with
/// `download-failed` when the request fails or the server answers with anything but a success; the status or the
/// cause goes to the console.
pub(super) struct WebFetcher;

#[async_trait(?Send)]
impl Fetcher for WebFetcher {
    async fn fetch(&self, url: &str) -> Result<Box<dyn Download>> {
        let controller = AbortController::new().map_err(|error| failed(url, &error))?;
        let init = js_sys::Object::new();
        Reflect::set(&init, &"signal".into(), &controller.signal()).expect("a plain object");
        let fetch = Reflect::get(&js_sys::global(), &"fetch".into())
            .ok()
            .and_then(|fetch| fetch.dyn_into::<js_sys::Function>().ok())
            .ok_or_else(|| failed(url, &"no fetch here".into()))?;
        let promise = fetch
            .call2(&JsValue::UNDEFINED, &url.into(), &init)
            .map_err(|error| failed(url, &error))?;
        let response: Response = JsFuture::from(js_sys::Promise::from(promise))
            .await
            .map_err(|error| failed(url, &error))?
            .unchecked_into();
        if !response.ok() {
            let status = JsValue::from(response.status());
            return Err(failed(url, &status));
        }
        let size = response
            .headers()
            .get("content-length")
            .ok()
            .flatten()
            .and_then(|length| length.parse().ok());
        let mut download = match response.body() {
            Some(body) => StreamDownload::new(&body, size, "download-failed"),
            None => StreamDownload::empty(),
        };
        download.controller = Some(controller);
        Ok(Box::new(download))
    }
}

/// Bytes read from a `ReadableStream`, a part at a time: a response's body, or a stored file's. Dropping it cancels
/// the stream, and aborts the request it came from, if any.
pub(super) struct StreamDownload {
    reader: Option<ReadableStreamDefaultReader>,
    size: Option<u64>,
    controller: Option<AbortController>,
    /// The code a failed read fails with: `download-failed`, or `storage-failed` for a stored file.
    failure: &'static str,
}

impl StreamDownload {
    fn new(stream: &ReadableStream, size: Option<u64>, failure: &'static str) -> Self {
        Self {
            reader: Some(stream.get_reader().unchecked_into()),
            size,
            controller: None,
            failure,
        }
    }

    fn empty() -> Self {
        Self {
            reader: None,
            size: Some(0),
            controller: None,
            failure: "download-failed",
        }
    }

    /// This stream failed: its code, with the cause in the console.
    fn failed(&self, cause: &JsValue) -> Error {
        web_sys::console::warn_2(&"sidevoice-engine: reading a stream failed:".into(), cause);
        Error::new(self.failure)
    }

    /// The bytes of `blob` (a stored `File`), whose size is known.
    pub(super) fn of_blob(blob: &Blob) -> Self {
        Self::new(&blob.stream(), Some(blob.size() as u64), "storage-failed")
    }
}

#[async_trait(?Send)]
impl Download for StreamDownload {
    fn size(&self) -> Option<u64> {
        self.size
    }

    async fn chunk(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            let Some(reader) = &self.reader else {
                return Ok(None);
            };
            let read = JsFuture::from(reader.read())
                .await
                .map_err(|error| self.failed(&error))?;
            let done = Reflect::get(&read, &"done".into()).is_ok_and(|done| done.is_truthy());
            if done {
                self.reader = None;
                return Ok(None);
            }
            let value =
                Reflect::get(&read, &"value".into()).map_err(|error| self.failed(&error))?;
            let bytes = value
                .dyn_into::<Uint8Array>()
                .map_err(|value| self.failed(&value))?;
            // A stream may hand over an empty part; the contract says a chunk is never empty.
            if bytes.length() > 0 {
                return Ok(Some(bytes.to_vec()));
            }
        }
    }
}

impl Drop for StreamDownload {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take() {
            // The promise settles on its own; nothing waits for it.
            let _ = reader.cancel();
        }
        if let Some(controller) = self.controller.take() {
            controller.abort();
        }
    }
}

/// `download-failed`, with what failed in the console.
fn failed(what: &str, cause: &JsValue) -> Error {
    web_sys::console::warn_3(
        &"sidevoice-engine: download failed:".into(),
        &what.into(),
        cause,
    );
    Error::new("download-failed")
}
