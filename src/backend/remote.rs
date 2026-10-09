//! What the remote backends share (`implementations/openai.rs`, `implementations/elevenlabs.rs`): a provider reached
//! through the host, with the key the host hands over for each call; the statuses every provider answers alike; the
//! request bodies they take (multipart forms, JSON); and audio in and out (a WAV file of a turn, 16-bit PCM back).

use std::sync::Arc;

use crate::catalog::BuildEntry;
use crate::host::{Host, HttpRequest};
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// The sample rate a turn is sent at: what the engine hands every speech-to-text model (`SttModel::transcribe`).
pub(crate) const STT_RATE: u32 = 16_000;

/// One provider, through the host: its HTTP and its key.
#[derive(Clone)]
pub(crate) struct Provider {
    host: Arc<dyn Host>,
    id: &'static str,
}

impl Provider {
    /// `id` (`"openai"`) through `host`.
    pub(crate) fn new(host: &Arc<dyn Host>, id: &'static str) -> Self {
        Self {
            host: Arc::clone(host),
            id,
        }
    }

    /// The provider's key, asked of the host for this call alone: `credential-missing` when it has none.
    pub(crate) async fn key(&self) -> Result<String> {
        self.host
            .credentials()
            .credential(self.id)
            .await?
            .ok_or(Error::new("credential-missing"))
    }

    /// Sends `request` and returns the body of a success. A refused key is `credential-rejected`, a request to slow
    /// down `rate-limited`, any other failure `failure` (`transcription-failed`, `speech-failed`, ...); no answer is
    /// the host's `request-failed`.
    pub(crate) async fn call(
        &self,
        request: HttpRequest,
        failure: &'static str,
    ) -> Result<Vec<u8>> {
        let response = self.host.http().send(request).await?;
        match response.status {
            200..=299 => Ok(response.body),
            status => Err(failed(self.id, status, &response.body, failure)),
        }
    }
}

/// The code for an answer of `status`, with what the provider said in the console on the web.
fn failed(provider: &str, status: u16, body: &[u8], failure: &'static str) -> Error {
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
    Error::new(match status {
        401 | 403 => "credential-rejected",
        429 => "rate-limited",
        _ => failure,
    })
}

/// The request field `build` maps `argument` to (its `call_params`), if any.
pub(crate) fn field<'a>(build: &'a BuildEntry, argument: &str) -> Option<&'a str> {
    build
        .call_params
        .get(argument)
        .and_then(|paths| paths.first())
        .map(String::as_str)
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
        .chunks_exact(2)
        .map(|sample| f32::from(i16::from_le_bytes([sample[0], sample[1]])) / 32_768.0)
        .collect()
}
