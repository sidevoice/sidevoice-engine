//! The engine's remote side: the providers compiled into it, their listings, and the remote models it makes. Apart from
//! the catalogue's: a provider has no builds, nothing is installed, and no accelerator is chosen.

use crate::capability::Resident;
use crate::provider::{Adapter, Api, Provider, RemoteModel};
use crate::{Engine, Error, Result};

#[cfg(test)]
mod tests;

impl Engine {
    /// Every remote provider compiled into this build, by id, each with the status of its listing and its models the
    /// app's key may use. A listing missing or old (models a day, voices an hour) is asked for first, through the host:
    /// the first call after the app starts lists every provider with a key, and a provider without one is listed with
    /// no call (`credential-missing`). A provider that fails says so in its own status: this never fails.
    pub async fn providers(&self) -> Vec<Provider> {
        let mut providers = Vec::new();
        for adapter in &self.providers {
            providers.push(self.listed(adapter.as_ref(), false).await);
        }
        providers
    }

    /// `provider` listed again now, models and voices, whatever their age: what the app does when the person asks, or
    /// saves a new key.
    ///
    /// # Errors
    ///
    /// `provider-not-found` for a provider not compiled into this build. Otherwise it is listed, and how that went is
    /// its [`Provider::status`].
    pub async fn refresh(&self, provider: &str) -> Result<Provider> {
        let adapter = self.adapter(provider)?;
        Ok(self.listed(adapter, true).await)
    }

    /// The model `model` of `provider`, which the provider's listing must have (listed first if it is missing or old).
    /// It makes no call: each call on it goes to the provider, with the key the host hands over for that call.
    ///
    /// # Errors
    ///
    /// `provider-not-found`; the provider's status when it has no listing (`credential-missing`, ...); and
    /// `model-not-found` for a model its listing does not have.
    pub async fn remote(&self, provider: &str, model: &str) -> Result<RemoteModel> {
        let adapter = self.adapter(provider)?;
        let listed = self.listed(adapter, false).await;
        let found = listed.models.iter().find(|listed| listed.id == model);
        let Some(found) = found else {
            return Err(match listed.status {
                Some(status) if listed.models.is_empty() => Error::new(status.code),
                _ => Error::new("model-not-found"),
            });
        };
        let spec = adapter.spec();
        let opened = adapter.open(Api::new(&self.host, spec.id), found)?;
        let resident = Resident::new(opened, None, found.languages.clone(), found.voices.clone());
        Ok(RemoteModel::new(spec.id, &found.id, resident))
    }

    /// The provider `id` (`provider-not-found` otherwise).
    fn adapter(&self, id: &str) -> Result<&dyn Adapter> {
        self.providers
            .iter()
            .map(Box::as_ref)
            .find(|adapter| adapter.spec().id == id)
            .ok_or(Error::new("provider-not-found"))
    }

    /// `adapter`'s provider as listed now: again if its listing is missing or old, or with `force`.
    async fn listed(&self, adapter: &dyn Adapter, force: bool) -> Provider {
        let api = Api::new(&self.host, adapter.spec().id);
        self.listings.provider(adapter, &api, force).await
    }
}
