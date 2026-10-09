//! The engine's remote side, behind its providers' catalogues: their listings and the remote models it makes. Apart
//! from the local catalogue's: a provider has no builds, nothing is installed, and no accelerator is chosen.

use crate::capability::Resident;
use crate::provider::listing::Listed;
use crate::provider::{Adapter, Api, RemoteModel};
use crate::{Engine, Error, Result};

impl Engine {
    /// The model `model` of `adapter`'s provider, which its listing must have (listed first if it is missing or old).
    /// It makes no call: each call on it goes to the provider, with the key the host hands over for that call.
    /// `model-not-found` for a model its listing does not have, and its status when it has no listing.
    pub(super) async fn remote(&self, adapter: &dyn Adapter, model: &str) -> Result<RemoteModel> {
        let listed = self.listed(adapter, false).await;
        let found = listed.models.iter().find(|listed| listed.id == model);
        let (Some(found), Some(facts)) = (found, &listed.facts) else {
            return Err(match listed.status {
                Some(status) if listed.models.is_empty() => Error::new(status),
                _ => Error::new("model-not-found"),
            });
        };
        let spec = adapter.spec();
        let opened = adapter.open(Api::new(&self.host, spec.id), found, facts)?;
        let resident = Resident::new(opened, None, found.languages.clone(), found.voices.clone());
        Ok(RemoteModel::new(spec.id, &found.id, resident))
    }

    /// `adapter`'s provider as listed now: again if its listing is missing or old, or with `force`.
    pub(super) async fn listed(&self, adapter: &dyn Adapter, force: bool) -> Listed {
        let api = Api::new(&self.host, adapter.spec().id);
        self.listings.provider(adapter, &api, force).await
    }
}
