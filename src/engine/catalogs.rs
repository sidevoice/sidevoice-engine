//! The engine's catalogues, one interface over every place models come from ([`Catalog`]): the local catalogue (files,
//! families, builds, backends, accelerators, install) and one per remote provider (a live listing with its own cache
//! and status). Each has an id, a status, its models (by capability), `refresh` and `load`, which hands back a model
//! with the shared capability interfaces ([`LoadedModel`]). An engine on the host would be one more.

use crate::catalog::{Capability, Voice};
use crate::engine::{LocalModel, Model};
use crate::install::{Cancel, ProgressSink};
use crate::provider::{Adapter, ProviderModel, RemoteModel};
use crate::resolver::Reason;
use crate::{EndOfTurn, Engine, Error, Result, Stt, Tts, Vad};

#[cfg(test)]
mod tests;

/// The id of the local catalogue.
pub const LOCAL_CATALOG: &str = "local";

/// One of the engine's catalogues ([`Engine::catalogs`]): the local one, or a remote provider's.
#[derive(Clone, Copy)]
pub struct Catalog<'a> {
    engine: &'a Engine,
    kind: Kind<'a>,
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    Local,
    Provider(&'a dyn Adapter),
}

impl std::fmt::Debug for Catalog<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("id", &self.id())
            .finish_non_exhaustive()
    }
}

/// How a catalogue stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogStatus {
    /// Why it is not current, as a stable code the app translates; `None` when it is. The local catalogue always is. A
    /// provider without a usable key (`credential-missing`, `credential-rejected`, `listing-not-permitted`: a key the
    /// provider does not let list, with no fallback) has no models; one that could not be asked
    /// (`provider-unreachable`, `provider-quota`, `listing-failed`) keeps its last listing, [`CatalogStatus::stale`];
    /// one whose spec could not be read (`provider-spec-unreadable`) keeps its last facts, and with none has no models.
    pub reason: Option<Reason>,
    /// Whether its models are the last listing kept after a refresh failed.
    pub stale: bool,
    /// What the provider said when it last refused (its own status, and its message): for a developer to read, never
    /// UI and never parsed.
    pub detail: Option<String>,
}

/// One model of a catalogue: a local one with its builds and install state, or a remote one as its provider lists it.
/// What every model has (id, capabilities, languages, voices, speed) is its methods'.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum CatalogModel {
    /// A model of the local catalogue.
    Local(Model),
    /// A model of a remote provider.
    Remote(ProviderModel),
}

impl CatalogModel {
    /// Its id in its catalogue, which [`Catalog::load`] takes.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Local(model) => &model.id,
            Self::Remote(model) => &model.id,
        }
    }

    /// What it can do.
    #[must_use]
    pub fn capabilities(&self) -> &[Capability] {
        match self {
            Self::Local(model) => &model.capabilities,
            Self::Remote(model) => &model.capabilities,
        }
    }

    /// The languages it handles, BCP 47 tags; empty when its source lists none.
    #[must_use]
    pub fn languages(&self) -> &[String] {
        match self {
            Self::Local(model) => &model.languages,
            Self::Remote(model) => &model.languages,
        }
    }

    /// Its voices, for a text-to-speech model whose source describes them.
    #[must_use]
    pub fn voices(&self) -> &[Voice] {
        match self {
            Self::Local(model) => &model.voices,
            Self::Remote(model) => &model.voices,
        }
    }

    /// The lowest and the highest speed it speaks at, where its source says (a remote model's spec); `None` otherwise.
    #[must_use]
    pub fn speed(&self) -> Option<[f32; 2]> {
        match self {
            Self::Local(_) => None,
            Self::Remote(model) => model.speed,
        }
    }
}

/// A model a catalogue loaded: a local one in memory, or a remote one that calls its provider. Either hands out the
/// same capability interfaces.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum LoadedModel {
    /// A local model's build, in memory.
    Local(LocalModel),
    /// A remote provider's model.
    Remote(RemoteModel),
}

impl LoadedModel {
    /// Its id in its catalogue.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Local(model) => model.id(),
            Self::Remote(model) => model.id(),
        }
    }

    /// What it can do.
    #[must_use]
    pub fn capabilities(&self) -> &[Capability] {
        match self {
            Self::Local(model) => model.capabilities(),
            Self::Remote(model) => model.capabilities(),
        }
    }

    /// The model as speech to text, if it is one.
    #[must_use]
    pub fn as_stt(&self) -> Option<Stt<'_>> {
        match self {
            Self::Local(model) => model.as_stt(),
            Self::Remote(model) => model.as_stt(),
        }
    }

    /// The model as text to speech, if it is one.
    #[must_use]
    pub fn as_tts(&self) -> Option<Tts<'_>> {
        match self {
            Self::Local(model) => model.as_tts(),
            Self::Remote(model) => model.as_tts(),
        }
    }

    /// The model as a voice activity detector, if it is one.
    #[must_use]
    pub fn as_vad(&self) -> Option<Vad<'_>> {
        match self {
            Self::Local(model) => model.as_vad(),
            Self::Remote(model) => model.as_vad(),
        }
    }

    /// The model as an end-of-turn classifier, if it is one.
    #[must_use]
    pub fn as_end_of_turn(&self) -> Option<EndOfTurn<'_>> {
        match self {
            Self::Local(model) => model.as_end_of_turn(),
            Self::Remote(model) => model.as_end_of_turn(),
        }
    }
}

impl Engine {
    /// Every catalogue of this engine: the local one first ([`LOCAL_CATALOG`]), then each remote provider compiled into
    /// this build, by id.
    #[must_use]
    pub fn catalogs(&self) -> Vec<Catalog<'_>> {
        let local = Catalog {
            engine: self,
            kind: Kind::Local,
        };
        let providers = self.providers.iter().map(|adapter| Catalog {
            engine: self,
            kind: Kind::Provider(adapter.as_ref()),
        });
        std::iter::once(local).chain(providers).collect()
    }

    /// The catalogue `id` (`"local"`, `"openai"`, `"elevenlabs"`).
    ///
    /// # Errors
    ///
    /// `catalog-not-found` for an id that is none of [`Engine::catalogs`].
    pub fn catalog(&self, id: &str) -> Result<Catalog<'_>> {
        self.catalogs()
            .into_iter()
            .find(|catalog| catalog.id() == id)
            .ok_or(Error::new("catalog-not-found"))
    }
}

impl<'a> Catalog<'a> {
    /// Its stable id: [`LOCAL_CATALOG`], or the provider's (`"openai"`, `"elevenlabs"`), which the host's
    /// [`Credentials`](crate::Credentials) know its key by.
    #[must_use]
    pub fn id(&self) -> &'a str {
        match self.kind {
            Kind::Local => LOCAL_CATALOG,
            Kind::Provider(adapter) => adapter.spec().id,
        }
    }

    /// A remote provider's name, as a person reads it ("OpenAI"); `None` for the local catalogue, which the app names.
    #[must_use]
    pub fn name(&self) -> Option<&'a str> {
        match self.kind {
            Kind::Local => None,
            Kind::Provider(adapter) => Some(adapter.spec().name),
        }
    }

    /// How it stands. A provider whose listing is missing or old is listed first, through the host: the first call
    /// after the app starts lists it if the host has its key, and without one nothing is asked
    /// (`credential-missing`).
    pub async fn status(&self) -> CatalogStatus {
        match self.kind {
            Kind::Local => CatalogStatus {
                reason: None,
                stale: false,
                detail: None,
            },
            Kind::Provider(adapter) => status(&self.engine.listed(adapter, false).await),
        }
    }

    /// Its models, those of `capability` alone when given: the local catalogue's in catalogue order, each with its
    /// builds ranked and what is installed; a provider's as listed (listed first if missing or old), each with its
    /// languages, voices and speed. A provider that cannot list has none, and says why in [`Catalog::status`].
    ///
    /// # Errors
    ///
    /// For the local catalogue, what the host's storage fails with, asked what is installed. A provider never fails
    /// here.
    pub async fn models(&self, capability: Option<Capability>) -> Result<Vec<CatalogModel>> {
        let models: Vec<CatalogModel> = match self.kind {
            Kind::Local => self
                .engine
                .models()
                .await?
                .into_iter()
                .map(CatalogModel::Local)
                .collect(),
            Kind::Provider(adapter) => self
                .engine
                .listed(adapter, false)
                .await
                .models
                .into_iter()
                .map(CatalogModel::Remote)
                .collect(),
        };
        Ok(models
            .into_iter()
            .filter(|model| capability.is_none_or(|wanted| model.capabilities().contains(&wanted)))
            .collect())
    }

    /// Reads it again now, whatever the age of what it has: a provider's spec, models and voices (what the app does
    /// when the person asks, or saves a new key). The local catalogue is compiled in: nothing to read.
    pub async fn refresh(&self) -> CatalogStatus {
        match self.kind {
            Kind::Local => self.status().await,
            Kind::Provider(adapter) => status(&self.engine.listed(adapter, true).await),
        }
    }

    /// The model `model`, ready to use. A local model is loaded as [`Engine::load`] does with no build named (an
    /// installed build that runs here, else the recommended one, installed first, telling `progress` and stopping once
    /// `cancel` is cancelled). A remote one must be in the provider's listing; it makes no call.
    ///
    /// # Errors
    ///
    /// For a local model, what [`Engine::load`] fails with. For a remote one, `model-not-found`, and the provider's
    /// status when it has no listing (`credential-missing`, ...).
    pub async fn load(
        &self,
        model: &str,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<LoadedModel> {
        match self.kind {
            Kind::Local => self
                .engine
                .load(model, None, progress, cancel)
                .await
                .map(LoadedModel::Local),
            Kind::Provider(adapter) => {
                cancel.check()?;
                self.engine
                    .remote(adapter, model, cancel)
                    .await
                    .map(LoadedModel::Remote)
            }
        }
    }
}

/// A provider's listing, as its catalogue's status.
fn status(listed: &crate::provider::listing::Listed) -> CatalogStatus {
    CatalogStatus {
        reason: listed.status.map(Reason::new),
        stale: listed.stale,
        detail: listed.detail.clone(),
    }
}
