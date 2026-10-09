//! A remote model, as the app holds it: [`RemoteModel`], a provider's model that
//! [`Engine::remote`](crate::Engine::remote) made, which hands out the same capability interfaces as a local one.

use std::sync::Arc;

use crate::capability::{EndOfTurn, Resident, Stt, Tts, Vad};
use crate::catalog::Capability;

/// A provider's model, made by [`Engine::remote`](crate::Engine::remote). Nothing of it is in memory but its
/// description: each call goes to the provider, through the host, with the key the host hands over for it. Calls on it
/// wait for one another.
#[derive(Clone)]
pub struct RemoteModel {
    provider: &'static str,
    id: String,
    resident: Arc<Resident>,
}

impl std::fmt::Debug for RemoteModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteModel")
            .field("provider", &self.provider)
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl RemoteModel {
    pub(crate) fn new(provider: &'static str, id: &str, resident: Arc<Resident>) -> Self {
        Self {
            provider,
            id: id.to_owned(),
            resident,
        }
    }

    /// The provider's id: `"openai"`, `"elevenlabs"`.
    #[must_use]
    pub fn provider(&self) -> &str {
        self.provider
    }

    /// The provider's id of the model.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The model in memory behind it.
    #[cfg_attr(
        not(web),
        allow(
            dead_code,
            reason = "the web bridge's: it hands the model to JavaScript's handles"
        )
    )]
    pub(crate) fn resident(&self) -> &Arc<Resident> {
        &self.resident
    }

    /// What it can do.
    #[must_use]
    pub fn capabilities(&self) -> &[Capability] {
        self.resident.capabilities()
    }

    /// The model as speech to text, if it is one.
    #[must_use]
    pub fn as_stt(&self) -> Option<Stt<'_>> {
        self.resident.as_stt()
    }

    /// The model as text to speech, if it is one.
    #[must_use]
    pub fn as_tts(&self) -> Option<Tts<'_>> {
        self.resident.as_tts()
    }

    /// The model as a voice activity detector, if it is one: no provider's is, today.
    #[must_use]
    pub fn as_vad(&self) -> Option<Vad<'_>> {
        self.resident.as_vad()
    }

    /// The model as an end-of-turn classifier, if it is one: no provider's is, today.
    #[must_use]
    pub fn as_end_of_turn(&self) -> Option<EndOfTurn<'_>> {
        self.resident.as_end_of_turn()
    }
}
