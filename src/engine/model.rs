//! What a catalogue hands out, as interfaces: [`ModelInfo`], a model as a catalogue lists it, and [`Model`], a model
//! ready to use, which hands out the capability interfaces. The local catalogue's and each provider's own types
//! implement them ([`LocalModelInfo`] and [`RemoteModelInfo`]; [`LocalModel`] and [`RemoteModel`]). The traits hold
//! only what is common, and know none of them: code that needs a type's specifics (a local model's builds, say) gets
//! it from its concrete catalogue ([`LocalCatalog`](crate::LocalCatalog), [`RemoteCatalog`](crate::RemoteCatalog)),
//! which hands out its own types.

use std::fmt;

use crate::catalog::{Capability, SpeedRange, Voice};
use crate::engine::{LocalModel, LocalModelInfo};
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::provider::{RemoteModel, RemoteModelInfo};
use crate::{EndOfTurn, Stt, Tts, Vad};

/// A model as a catalogue lists it: what every model has, wherever it comes from.
pub trait ModelInfo: fmt::Debug + MaybeSend + MaybeSync {
    /// Its id in its catalogue, which the catalogue's `load` takes.
    fn id(&self) -> &str;

    /// What it can do.
    fn capabilities(&self) -> &[Capability];

    /// The languages it handles, BCP 47 tags; empty when its source lists none.
    fn languages(&self) -> &[String];

    /// Its voices, for a text-to-speech model whose source describes them.
    fn voices(&self) -> &[Voice];

    /// The speeds it takes; `None` means only that it takes no speed.
    fn speed(&self) -> Option<SpeedRange>;
}

/// A model ready to use, which a catalogue loaded: it hands out the capability interfaces, the same whatever runs it.
pub trait Model: fmt::Debug + MaybeSend + MaybeSync {
    /// Its id in its catalogue.
    fn id(&self) -> &str;

    /// What it can do.
    fn capabilities(&self) -> &[Capability];

    /// The model as speech to text, if it is one.
    fn as_stt(&self) -> Option<Stt<'_>>;

    /// The model as text to speech, if it is one.
    fn as_tts(&self) -> Option<Tts<'_>>;

    /// The model as a voice activity detector, if it is one.
    fn as_vad(&self) -> Option<Vad<'_>>;

    /// The model as an end-of-turn classifier, if it is one.
    fn as_end_of_turn(&self) -> Option<EndOfTurn<'_>>;
}

impl ModelInfo for LocalModelInfo {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    fn languages(&self) -> &[String] {
        &self.languages
    }

    fn voices(&self) -> &[Voice] {
        &self.voices
    }

    fn speed(&self) -> Option<SpeedRange> {
        self.speed
    }
}

impl ModelInfo for RemoteModelInfo {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    fn languages(&self) -> &[String] {
        &self.languages
    }

    fn voices(&self) -> &[Voice] {
        &self.voices
    }

    fn speed(&self) -> Option<SpeedRange> {
        self.speed
    }
}

impl Model for LocalModel {
    fn id(&self) -> &str {
        LocalModel::id(self)
    }

    fn capabilities(&self) -> &[Capability] {
        LocalModel::capabilities(self)
    }

    fn as_stt(&self) -> Option<Stt<'_>> {
        LocalModel::as_stt(self)
    }

    fn as_tts(&self) -> Option<Tts<'_>> {
        LocalModel::as_tts(self)
    }

    fn as_vad(&self) -> Option<Vad<'_>> {
        LocalModel::as_vad(self)
    }

    fn as_end_of_turn(&self) -> Option<EndOfTurn<'_>> {
        LocalModel::as_end_of_turn(self)
    }
}

impl Model for RemoteModel {
    fn id(&self) -> &str {
        RemoteModel::id(self)
    }

    fn capabilities(&self) -> &[Capability] {
        RemoteModel::capabilities(self)
    }

    fn as_stt(&self) -> Option<Stt<'_>> {
        RemoteModel::as_stt(self)
    }

    fn as_tts(&self) -> Option<Tts<'_>> {
        RemoteModel::as_tts(self)
    }

    fn as_vad(&self) -> Option<Vad<'_>> {
        RemoteModel::as_vad(self)
    }

    fn as_end_of_turn(&self) -> Option<EndOfTurn<'_>> {
        RemoteModel::as_end_of_turn(self)
    }
}
