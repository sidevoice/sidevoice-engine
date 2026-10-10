//! The engine's catalogues, one interface over every place models come from ([`Catalog`]): the local catalogue
//! ([`LocalCatalog`]: files, families, builds, backends, accelerators, install) and one per remote provider
//! ([`RemoteCatalog`]: a live listing with its own cache and status). Each has an id, a status, its models (by
//! capability), `refresh` and `load`, which hands back a [`Model`] with the shared capability interfaces. An engine on
//! the host would be one more. The trait holds what every catalogue has; each concrete catalogue also hands out its own
//! types, with their specifics (a local model's builds and install state; a provider's listing), for code that knows
//! which catalogue it asks.

use async_trait::async_trait;

use crate::catalog::Capability;
use crate::engine::{LocalModel, LocalModelInfo, Model, ModelInfo};
use crate::install::{Cancel, ProgressSink};
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::provider::listing::Listed;
use crate::provider::{Adapter, RemoteModel, RemoteModelInfo};
use crate::resolver::Reason;
use crate::{Engine, Error, Result};

#[cfg(test)]
mod tests;

/// The id of the local catalogue.
pub const LOCAL_CATALOG: &str = "local";

/// A place models come from: the local catalogue, or a remote provider.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Catalog: MaybeSend + MaybeSync {
    /// Its stable id: [`LOCAL_CATALOG`], or the provider's (`"openai"`, `"elevenlabs"`), which the host's
    /// [`Credentials`](crate::Credentials) know its key by.
    fn id(&self) -> &str;

    /// A remote provider's name, as a person reads it ("OpenAI"); `None` for the local catalogue, which the app names.
    fn name(&self) -> Option<&str>;

    /// How it stands. A provider whose listing is missing or old is listed first, through the host: the first call
    /// after the app starts lists it if the host has its key, and without one nothing is asked (`credential-missing`).
    async fn status(&self) -> CatalogStatus;

    /// Its models, those of `capability` alone when given: the local catalogue's in catalogue order, each with its
    /// builds ranked and what is installed; a provider's as listed (listed first if missing or old). A provider that
    /// cannot list has none, and says why in [`Catalog::status`].
    ///
    /// # Errors
    ///
    /// For the local catalogue, what the host's storage fails with, asked what is installed. A provider never fails
    /// here.
    async fn models(&self, capability: Option<Capability>) -> Result<Vec<Box<dyn ModelInfo>>>;

    /// Reads it again now, whatever the age of what it has: a provider's spec, models and voices (what the app does
    /// when the person asks, or saves a new key). The local catalogue is compiled in: nothing to read.
    async fn refresh(&self) -> CatalogStatus;

    /// The model `model`, ready to use. A local model is loaded as [`Engine::load`] does with no build named (an
    /// installed build that runs here, else the recommended one, installed first, telling `progress` and stopping once
    /// `cancel` is cancelled). A remote one must be in the provider's listing; it makes no call, and is `cancelled`
    /// once `cancel` is, while its listing is read too.
    ///
    /// # Errors
    ///
    /// For a local model, what [`Engine::load`] fails with. For a remote one, `model-not-found`, the provider's status
    /// when it has no listing (`credential-missing`, ...), and `cancelled`.
    async fn load(
        &self,
        model: &str,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<Box<dyn Model>>;
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

impl CatalogStatus {
    /// Current, with nothing to say.
    const CURRENT: Self = Self {
        reason: None,
        stale: false,
        detail: None,
    };

    /// A provider's listing, as its catalogue's status.
    fn of(listed: &Listed) -> Self {
        Self {
            reason: listed.status.map(Reason::new),
            stale: listed.stale,
            detail: listed.detail.clone(),
        }
    }
}

/// The local catalogue: the merged catalogue's models, their builds and what is installed. Its builds are also the
/// engine's own to install, uninstall and load by name ([`Engine::install`], [`Engine::load`]).
#[derive(Debug, Clone, Copy)]
pub struct LocalCatalog<'a> {
    engine: &'a Engine,
}

/// A remote provider, as a catalogue.
#[derive(Clone, Copy)]
pub struct RemoteCatalog<'a> {
    engine: &'a Engine,
    adapter: &'a dyn Adapter,
}

impl std::fmt::Debug for RemoteCatalog<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteCatalog")
            .field("id", &self.adapter.spec().id)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Every catalogue of this engine: the local one first ([`LOCAL_CATALOG`]), then each remote provider compiled
    /// into this build, by id.
    #[must_use]
    pub fn catalogs(&self) -> Vec<Box<dyn Catalog + '_>> {
        let local: Box<dyn Catalog + '_> = Box::new(self.local_catalog());
        let providers = self.providers.iter().map(|adapter| {
            Box::new(RemoteCatalog {
                engine: self,
                adapter: adapter.as_ref(),
            }) as Box<dyn Catalog + '_>
        });
        std::iter::once(local).chain(providers).collect()
    }

    /// The catalogue `id` (`"local"`, `"openai"`, `"elevenlabs"`).
    ///
    /// # Errors
    ///
    /// `catalog-not-found` for an id that is none of [`Engine::catalogs`].
    pub fn catalog(&self, id: &str) -> Result<Box<dyn Catalog + '_>> {
        self.catalogs()
            .into_iter()
            .find(|catalog| catalog.id() == id)
            .ok_or(Error::new("catalog-not-found"))
    }

    /// The local catalogue, as itself: its models are [`LocalModelInfo`]s and it loads [`LocalModel`]s.
    #[must_use]
    pub fn local_catalog(&self) -> LocalCatalog<'_> {
        LocalCatalog { engine: self }
    }

    /// The remote provider `id` (`"openai"`, `"elevenlabs"`), as itself: its models are [`RemoteModelInfo`]s and it
    /// loads [`RemoteModel`]s.
    ///
    /// # Errors
    ///
    /// `catalog-not-found` for an id that is no provider of this build.
    pub fn remote_catalog(&self, id: &str) -> Result<RemoteCatalog<'_>> {
        self.providers
            .iter()
            .find(|adapter| adapter.spec().id == id)
            .map(|adapter| RemoteCatalog {
                engine: self,
                adapter: adapter.as_ref(),
            })
            .ok_or(Error::new("catalog-not-found"))
    }
}

/// `models`, those of `capability` alone when given.
fn of_capability<M: ModelInfo>(models: Vec<M>, capability: Option<Capability>) -> Vec<M> {
    models
        .into_iter()
        .filter(|model| capability.is_none_or(|wanted| model.capabilities().contains(&wanted)))
        .collect()
}

/// `models`, as the trait hands them out.
fn boxed<M: ModelInfo + 'static>(models: Vec<M>) -> Vec<Box<dyn ModelInfo>> {
    models
        .into_iter()
        .map(|model| Box::new(model) as Box<dyn ModelInfo>)
        .collect()
}

impl LocalCatalog<'_> {
    /// Its models, as [`Catalog::models`] lists them, with their local specifics: each one's family, size, licence,
    /// builds ranked and what is installed.
    ///
    /// # Errors
    ///
    /// What the host's storage fails with, asked what is installed.
    pub async fn models(&self, capability: Option<Capability>) -> Result<Vec<LocalModelInfo>> {
        Ok(of_capability(self.engine.models().await?, capability))
    }

    /// The model `model`, loaded as [`Catalog::load`] does: as [`Engine::load`] with no build named.
    ///
    /// # Errors
    ///
    /// What [`Engine::load`] fails with.
    pub async fn load(
        &self,
        model: &str,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<LocalModel> {
        self.engine.load(model, None, progress, cancel).await
    }
}

impl RemoteCatalog<'_> {
    /// Its models, as [`Catalog::models`] lists them: the provider's listing.
    pub async fn models(&self, capability: Option<Capability>) -> Vec<RemoteModelInfo> {
        let listed = self.engine.listed(self.adapter, false).await;
        of_capability(listed.models, capability)
    }

    /// The model `model`, as [`Catalog::load`] makes it.
    ///
    /// # Errors
    ///
    /// `model-not-found`, the provider's status when it has no listing (`credential-missing`, ...), and `cancelled`.
    pub async fn load(&self, model: &str, cancel: &Cancel) -> Result<RemoteModel> {
        self.engine.remote(self.adapter, model, cancel).await
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Catalog for LocalCatalog<'_> {
    fn id(&self) -> &str {
        LOCAL_CATALOG
    }

    fn name(&self) -> Option<&str> {
        None
    }

    async fn status(&self) -> CatalogStatus {
        CatalogStatus::CURRENT
    }

    async fn models(&self, capability: Option<Capability>) -> Result<Vec<Box<dyn ModelInfo>>> {
        Ok(boxed(LocalCatalog::models(self, capability).await?))
    }

    async fn refresh(&self) -> CatalogStatus {
        CatalogStatus::CURRENT
    }

    async fn load(
        &self,
        model: &str,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<Box<dyn Model>> {
        Ok(Box::new(
            LocalCatalog::load(self, model, progress, cancel).await?,
        ))
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Catalog for RemoteCatalog<'_> {
    fn id(&self) -> &str {
        self.adapter.spec().id
    }

    fn name(&self) -> Option<&str> {
        Some(self.adapter.spec().name)
    }

    async fn status(&self) -> CatalogStatus {
        CatalogStatus::of(&self.engine.listed(self.adapter, false).await)
    }

    async fn models(&self, capability: Option<Capability>) -> Result<Vec<Box<dyn ModelInfo>>> {
        Ok(boxed(RemoteCatalog::models(self, capability).await))
    }

    async fn refresh(&self) -> CatalogStatus {
        CatalogStatus::of(&self.engine.listed(self.adapter, true).await)
    }

    async fn load(
        &self,
        model: &str,
        _progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<Box<dyn Model>> {
        Ok(Box::new(RemoteCatalog::load(self, model, cancel).await?))
    }
}
