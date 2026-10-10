//! Remote providers, the catalogue's sibling: where models that run on someone else's servers come from (OpenAI,
//! ElevenLabs). The catalogue says which local models exist and how to install them; a provider says, live, which of
//! its models the app's key may use. A remote model has no builds, no install and no accelerator, and is used through
//! the same capability interfaces as a local one ([`Stt`](crate::Stt), [`Tts`](crate::Tts), ...).
//!
//! Each provider is one file (`openai.rs`, `elevenlabs.rs`) with its [`Adapter`], which registers itself as backends
//! do (`registry`), with no central list. An adapter reads its provider's official OpenAPI spec and derives from it
//! what the API does not say (`facts`), lists its provider's models and voices through the host ([`Api`]: the host's
//! HTTP, with the key the host hands over for each call), keeps to what the spec describes, and makes the model that
//! calls it. The engine keeps each provider's spec facts and listing in memory only (`listing`); each provider is one
//! of the engine's catalogues ([`Engine::catalogs`](crate::Engine::catalogs)).
//!
//! Inside: `api` (the provider's API through the host, and the bodies it takes), `facts`, `listing` (the cache and its
//! ages), `registry`, `remote_model` ([`RemoteModel`]) and one file per provider.

use async_trait::async_trait;
use serde_json::Value;

use crate::backend::BackendModel;
use crate::catalog::{Capability, SpeedRange, Voice};
use crate::maybe_send::{MaybeSend, MaybeSync};
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
pub(crate) use facts::Facts;
pub(crate) use registry::built_in;
pub use remote_model::RemoteModel;

/// One model of a provider's, as listed.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteModelInfo {
    /// The provider's id of it, which its API is called with and its catalogue's `load` takes:
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
    pub speed: Option<SpeedRange>,
}

/// What a provider is, as data.
pub(crate) struct ProviderSpec {
    /// Its stable id: what its key is known by.
    pub(crate) id: &'static str,
    /// Its name, as a person reads it.
    pub(crate) name: &'static str,
    /// Where its official OpenAPI spec is, which its facts are read from.
    pub(crate) spec: &'static str,
}

/// A provider's adapter: its record, what it reads from its spec, its live listing, and the models it makes. A listing
/// fails with `credential-missing`, `credential-rejected`, `listing-not-permitted`, `provider-quota`,
/// `provider-unreachable` and `listing-failed` (an answer that does not parse); see [`Api::list`].
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait Adapter: MaybeSend + MaybeSync {
    /// What it is: its `const` record.
    fn spec(&self) -> &ProviderSpec;

    /// What its spec (the JSON at [`ProviderSpec::spec`]) says that its API does not: `provider-spec-unreadable` when
    /// the spec no longer says it where it used to.
    fn facts(&self, spec: &Value) -> Result<Facts>;

    /// Its models the key may use, listed now: those the provider lists and `facts` describe, and nothing else, with
    /// no voices (the engine adds them, [`Adapter::voices`]).
    async fn models(&self, api: &Api, facts: &Facts) -> Result<Vec<RemoteModelInfo>>;

    /// The voices of its text-to-speech models: the account's, listed now, or those `facts` fix.
    async fn voices(&self, api: &Api, facts: &Facts) -> Result<Vec<Voice>>;

    /// The model `model` (one it listed) calling the provider through `api`: makes no call. `unsupported-model` for a
    /// model `facts` do not describe.
    fn open(
        &self,
        api: Api,
        model: &RemoteModelInfo,
        facts: &Facts,
    ) -> Result<Box<dyn BackendModel>>;
}
