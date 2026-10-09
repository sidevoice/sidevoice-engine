//! A provider's API as its adapter calls it: through the host's HTTP, with the key the host hands over for each call
//! ([`Api`]); the statuses every provider answers alike; and the request and answer bodies they share (multipart forms,
//! JSON, a WAV file of a turn, 16-bit PCM back); and the provider's OpenAPI spec, read as any page is.

use std::sync::{Arc, Mutex, PoisonError};

use crate::host::{Host, HttpRequest};
use crate::{Error, Result};

use Kind::{Call, Listing};

#[cfg(test)]
mod tests;

/// One provider's API, through the host: its HTTP and its key; and what the provider said when it last refused.
#[derive(Clone)]
pub(crate) struct Api {
    host: Arc<dyn Host>,
    id: &'static str,
    detail: Arc<Mutex<Option<String>>>,
}

impl Api {
    /// `id` (`"openai"`) through `host`.
    pub(crate) fn new(host: &Arc<dyn Host>, id: &'static str) -> Self {
        Self {
            host: Arc::clone(host),
            id,
            detail: Arc::default(),
        }
    }

    /// What the provider said when it last refused a request (its own status or code, and its message), for a
    /// developer to read: never UI, never parsed further.
    pub(crate) fn detail(&self) -> Option<String> {
        self.detail
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The provider's OpenAPI spec at `url`, read without the key: `provider-spec-unreadable` when it cannot be
    /// fetched or is not JSON.
    pub(crate) async fn spec(&self, url: &str) -> Result<serde_json::Value> {
        let request = HttpRequest {
            method: "GET",
            url: url.to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
        };
        let unreadable = || Error::new("provider-spec-unreadable");
        let response = self
            .host
            .http()
            .send(request)
            .await
            .map_err(|_| unreadable())?;
        if !(200..=299).contains(&response.status) {
            return Err(unreadable());
        }
        serde_json::from_slice(&response.body).map_err(|_| unreadable())
    }

    /// The provider's key, asked of the host for this call alone: `credential-missing` when it has none.
    pub(crate) async fn key(&self) -> Result<String> {
        self.host
            .credentials()
            .credential(self.id)
            .await?
            .ok_or(Error::new("credential-missing"))
    }

    /// Sends `request` and returns the body of a success. A refused key (401, 403) is `credential-rejected`, a request
    /// to slow down `rate-limited`, an account out of credit `provider-quota`, any other failure `failure` (`transcription-failed`, `speech-failed`, ...); no
    /// answer is the host's `request-failed`.
    pub(crate) async fn call(
        &self,
        request: HttpRequest,
        failure: &'static str,
    ) -> Result<Vec<u8>> {
        self.send(request, Call(failure)).await
    }

    /// Sends `request`, a listing, and returns the body of a success: as [`Api::call`], but a key the provider
    /// knows and does not let list (403, or a 401 that says a permission is missing: ElevenLabs answers a scoped key
    /// that lacks one so) is `listing-not-permitted`, an account out of credit or asking to slow down
    /// (402, 429) `provider-quota`, a server failing (5xx) or no answer at all `provider-unreachable`, and any other
    /// failure `listing-failed`.
    pub(crate) async fn list(&self, request: HttpRequest) -> Result<Vec<u8>> {
        match self.send(request, Listing).await {
            Err(error) if error.code == "request-failed" => Err(Error::new("provider-unreachable")),
            answer => answer,
        }
    }

    async fn send(&self, request: HttpRequest, kind: Kind) -> Result<Vec<u8>> {
        let response = self.host.http().send(request).await?;
        match response.status {
            200..=299 => Ok(response.body),
            status => {
                let said = said(&response.body);
                let error = failed(self.id, status, &response.body, said.as_ref(), kind);
                *self.detail.lock().unwrap_or_else(PoisonError::into_inner) =
                    said.map(|said| said.text());
                Err(error)
            }
        }
    }
}

/// What a request is, for what its failures are called: a call to a model, which fails with its own code, or a listing.
#[derive(Clone, Copy)]
enum Kind {
    Call(&'static str),
    Listing,
}

/// What a provider says when it refuses: its own status or code, and its message. ElevenLabs answers
/// `{"detail": {"status" or "code", "message"}}`, OpenAI `{"error": {"code" or "type", "message"}}`.
struct Said {
    status: Option<String>,
    message: Option<String>,
}

impl Said {
    fn text(&self) -> String {
        match (&self.status, &self.message) {
            (Some(status), Some(message)) => format!("{status}: {message}"),
            (Some(said), None) | (None, Some(said)) => said.clone(),
            (None, None) => String::new(),
        }
    }

    /// Whether it says the key lacks a permission.
    fn lacks_permission(&self) -> bool {
        matches!(
            self.status.as_deref(),
            Some("missing_permissions" | "insufficient_permissions")
        )
    }
}

fn said(body: &[u8]) -> Option<Said> {
    let body: serde_json::Value = serde_json::from_slice(body).ok()?;
    let detail = [&body["detail"], &body["error"]]
        .into_iter()
        .find(|detail| detail.is_object())?;
    let text = |field: &str| detail[field].as_str().map(str::to_owned);
    let said = Said {
        status: text("status")
            .or_else(|| text("code"))
            .or_else(|| text("type")),
        message: text("message"),
    };
    (said.status.is_some() || said.message.is_some()).then_some(said)
}

/// The code for an answer of `status` to a request of `kind`, with what the provider said in the console on the web.
fn failed(provider: &str, status: u16, body: &[u8], said: Option<&Said>, kind: Kind) -> Error {
    #[cfg(web)]
    web_sys::console::warn_4(
        &"sidevoice-engine: the provider answered".into(),
        &provider.into(),
        &status.into(),
        &String::from_utf8_lossy(&body[..body.len().min(500)])
            .as_ref()
            .into(),
    );
    #[cfg(not(web))]
    let _ = (provider, body);
    let lacks_permission = said.is_some_and(Said::lacks_permission);
    Error::new(match (status, kind) {
        (401 | 403, Listing) if lacks_permission => "listing-not-permitted",
        (401, _) => "credential-rejected",
        (402, _) | (429, Listing) => "provider-quota",
        (403, Call(_)) => "credential-rejected",
        (403, Listing) => "listing-not-permitted",
        (429, Call(_)) => "rate-limited",
        (500..=599, Listing) => "provider-unreachable",
        (_, Listing) => "listing-failed",
        (_, Call(failure)) => failure,
    })
}

/// The JSON of a listing's answer, `listing-failed` when it is not JSON.
pub(crate) fn listed(body: &[u8]) -> Result<serde_json::Value> {
    serde_json::from_slice(body).map_err(|_| Error::new("listing-failed"))
}

/// The primary subtag of the BCP 47 tag `tag`, lowercased: what providers name a language by (`es`).
pub(crate) fn primary_subtag(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// The text field `field` of the JSON object `body`, `failure` when there is none.
pub(crate) fn text(body: &[u8], field: &str, failure: &'static str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| Error::new(failure))?;
    value[field]
        .as_str()
        .map(|text| text.trim().to_owned())
        .ok_or(Error::new(failure))
}

/// A `multipart/form-data` body, built a part at a time.
pub(crate) struct Form {
    boundary: String,
    body: Vec<u8>,
}

impl Form {
    pub(crate) fn new() -> Self {
        Self {
            // Any string the parts cannot hold: they are text fields and a WAV file, which never has this line.
            boundary: "sidevoice-engine-form-7d1b4e9a3f2c".to_owned(),
            body: Vec::new(),
        }
    }

    /// A text field.
    pub(crate) fn text(mut self, name: &str, value: &str) -> Self {
        self.head(&format!(
            "Content-Disposition: form-data; name=\"{name}\"\r\n"
        ));
        self.body.extend_from_slice(value.as_bytes());
        self.body.extend_from_slice(b"\r\n");
        self
    }

    /// A file field.
    pub(crate) fn file(
        mut self,
        name: &str,
        file_name: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Self {
        self.head(&format!(
            "Content-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\nContent-Type: \
             {content_type}\r\n"
        ));
        self.body.extend_from_slice(bytes);
        self.body.extend_from_slice(b"\r\n");
        self
    }

    /// Its `Content-Type`, and the body.
    pub(crate) fn finish(mut self) -> (String, Vec<u8>) {
        self.body
            .extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        (
            format!("multipart/form-data; boundary={}", self.boundary),
            self.body,
        )
    }

    fn head(&mut self, headers: &str) {
        self.body
            .extend_from_slice(format!("--{}\r\n{headers}\r\n", self.boundary).as_bytes());
    }
}

/// `pcm` (mono, in [-1, 1]) at `rate`, as a 16-bit WAV file: what a turn is sent as.
pub(crate) fn wav(pcm: &[f32], rate: u32) -> Vec<u8> {
    let data = u32::try_from(pcm.len() * 2).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for sample in pcm {
        let sample = (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// Little-endian 16-bit PCM as samples in [-1, 1]; an odd last byte is dropped.
pub(crate) fn pcm16(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|sample| f32::from(i16::from_le_bytes(*sample)) / 32_768.0)
        .collect()
}
