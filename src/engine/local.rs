//! A local model in memory, as the app holds it: [`LocalModel`], the build of a catalogue model that
//! [`Engine::load`](crate::Engine::load) loaded, which hands out the capability interfaces ([`Stt`], [`Tts`], [`Vad`],
//! [`EndOfTurn`]). Every `LocalModel` of one build shares one model in memory, which is unloaded when the last of them
//! (or a `VadStream` of it) is dropped.

use std::sync::Arc;

use crate::capability::{EndOfTurn, Resident, Stt, Tts, Vad};
use crate::catalog::Capability;

/// A catalogue model's build, loaded by [`Engine::load`](crate::Engine::load). It stays in memory while any
/// `LocalModel` of its build lives: dropping the last one unloads it. Calls on it wait for one another.
#[derive(Clone)]
pub struct LocalModel {
    id: String,
    build: String,
    resident: Arc<Resident>,
}

impl std::fmt::Debug for LocalModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalModel")
            .field("id", &self.id)
            .field("build", &self.build)
            .finish_non_exhaustive()
    }
}

impl LocalModel {
    pub(crate) fn new(id: &str, build: &str, resident: Arc<Resident>) -> Self {
        Self {
            id: id.to_owned(),
            build: build.to_owned(),
            resident,
        }
    }

    /// The model's id, in the catalogue.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The id of the build that was loaded.
    #[must_use]
    pub fn build(&self) -> &str {
        &self.build
    }

    /// The model in memory behind it.
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

    /// The model as a voice activity detector, if it is one.
    #[must_use]
    pub fn as_vad(&self) -> Option<Vad<'_>> {
        self.resident.as_vad()
    }

    /// The model as an end-of-turn classifier, if it is one.
    #[must_use]
    pub fn as_end_of_turn(&self) -> Option<EndOfTurn<'_>> {
        self.resident.as_end_of_turn()
    }
}
