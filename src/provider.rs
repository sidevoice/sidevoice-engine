//! Remote providers, the catalogue's sibling: where models that run on someone else's servers come from (OpenAI,
//! ElevenLabs). The catalogue says which local models exist and how to install them; a provider says, live, which of
//! its models the app's key may use. A remote model has no builds, no install and no accelerator, and is used through
//! the same capability interfaces as a local one ([`Stt`](crate::Stt), [`Tts`](crate::Tts), ...).
//!
//! Each provider is one file (`openai.rs`, `elevenlabs.rs`) with its [`Adapter`], which registers itself as backends
//! do (`registry`), with no central list. An adapter lists its provider's models and voices through the host
//! ([`Api`]: the host's HTTP, with the key the host hands over for each call), keeps to what its provider's spec
//! describes (`facts`: what the API does not say, derived from the provider's OpenAPI spec by
//! `cargo xtask pin-providers`), and makes the model that calls it. The engine keeps each provider's listing in memory
//! only (`listing`), and lists the providers with their status ([`Engine::providers`](crate::Engine::providers)).
//!
//! Inside: `api` (the provider's API through the host, and the bodies it takes), `facts`, `listing` (the cache and its
//! ages), `registry`, `remote_model` ([`RemoteModel`]) and one file per provider.

use async_trait::async_trait;

use crate::backend::BackendModel;
use crate::catalog::{Capability, Voice};
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::resolver::Reason;
use crate::Result;

mod api;
mod elevenlabs;
mod facts;
pub(crate) mod listing;
mod openai;
mod registry;
mod remote_model;
#[cfg(test)]
mod tests;

pub(crate) use api::Api;
pub(crate) use registry::built_in;
pub use remote_model::RemoteModel;

/// A provider, as [`Engine::providers`](crate::Engine::providers) lists it: what it is, how its listing stands, and
/// the models the app's key may use.
#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    /// Its stable id, which the host's [`Credentials`](crate::Credentials) know its key by: `"openai"`,
    /// `"elevenlabs"`.
    pub id: &'static str,
    /// Its name, as a person reads it.
    pub name: &'static str,
    /// What it is, in one sentence, in English: a developer's description, not UI.
    pub description: &'static str,
    /// Why its listing is not current, as a stable code the app translates; `None` when it is. Without a usable key
    /// (`credential-missing`, `credential-rejected`, `listing-not-permitted`: a key the provider does not let list,
    /// with no fallback) it has no models; when the provider could not be asked (`provider-unreachable`,
    /// `provider-quota`, `listing-failed`) it keeps the last listing, [`Provider::stale`].
    pub status: Option<Reason>,
    /// Whether `models` is the last listing kept after a refresh failed, rather than the provider's answer now.
    pub stale: bool,
    /// Its models the key may use, as listed: those the provider lists and its spec describes.
    pub models: Vec<ProviderModel>,
}

/// One model of a provider's, as listed.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderModel {
    /// The provider's id of it, which its API is called with and [`Engine::remote`](crate::Engine::remote) takes:
    /// `"gpt-4o-transcribe"`, `"eleven_flash_v2_5"`.
    pub id: String,
    /// What it can do.
    pub capabilities: Vec<Capability>,
    /// The languages it handles, BCP 47 tags, as the provider lists them; empty when the provider lists none (OpenAI).
    pub languages: Vec<String>,
    /// Its voices, for a text-to-speech model: the account's, as listed (ElevenLabs), or those the provider's spec
    /// fixes (OpenAI).
    pub voices: Vec<Voice>,
    /// The lowest and the highest speed it speaks at, from the provider's spec; `None` when it takes no speed (a model
    /// that is not text to speech, or Eleven v3). [`Tts::speak`](crate::Tts::speak) keeps a speed within it.
    pub speed: Option<[f32; 2]>,
}

/// What a provider is, as data.
pub(crate) struct ProviderSpec {
    /// Its stable id: what its key is known by.
    pub(crate) id: &'static str,
    /// Its name, as a person reads it.
    pub(crate) name: &'static str,
    /// What it is, in one sentence, in English.
    pub(crate) description: &'static str,
}

/// A provider's adapter: its record, its live listing, and the models it makes. A listing fails with
/// `credential-missing`, `credential-rejected`, `listing-not-permitted`, `provider-quota`, `provider-unreachable` and
/// `listing-failed` (an answer that does not parse); see [`Api::list`].
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait Adapter: MaybeSend + MaybeSync {
    /// What it is: its `const` record.
    fn spec(&self) -> &ProviderSpec;

    /// Its models the key may use, listed now: those the provider lists and its spec describes, and nothing else,
    /// with no voices (the engine adds them, [`Adapter::voices`]).
    async fn models(&self, api: &Api) -> Result<Vec<ProviderModel>>;

    /// The voices of its text-to-speech models: the account's, listed now, or those its spec fixes.
    async fn voices(&self, api: &Api) -> Result<Vec<Voice>>;

    /// The model `model` (one it listed) calling the provider through `api`: makes no call. `unsupported-model` for a
    /// model its spec does not describe.
    fn open(&self, api: Api, model: &ProviderModel) -> Result<Box<dyn BackendModel>>;
}
