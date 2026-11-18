//! The engine's remote side, behind its providers' catalogues: their listings and the remote models it makes. Apart
//! from the local catalogue's: a provider has no builds, nothing is installed, and no accelerator is chosen.

use std::sync::Arc;

use crate::capability::Resident;
use crate::install::Cancel;
use crate::provider::listing::Listed;
use crate::provider::{Adapter, Api, RemoteModel};
use crate::{Engine, Error, Result};

impl Engine {
    /// The model `model` of `adapter`'s provider, which its listing must have. It makes no call: each call on it goes to
    /// the provider, with the key the host hands over for that call. The listing is the one kept, whatever its age (it
    /// is read again in the background); only a provider with none yet is listed first, which waits for its own provider
    /// alone. `model-not-found` for a model its listing does not have, and its status when it has no listing;
    /// `cancelled` once `cancel` is, whether before or while its listing is read.
    pub(super) async fn remote(
        &self,
        adapter: &Arc<dyn Adapter>,
        model: &str,
        cancel: &Cancel,
    ) -> Result<RemoteModel> {
        cancel.check()?;
        let api = self.api(adapter);
        let listed = self.listings.for_model(adapter, &api).await;
        cancel.check()?;
        let found = listed.models.iter().find(|listed| listed.id == model);
        let (Some(found), Some(facts)) = (found, &listed.facts) else {
            return Err(match listed.status {
                Some(status) if listed.models.is_empty() => Error::new(status),
                _ => Error::new("model-not-found"),
            });
        };
        let spec = adapter.spec();
        let opened = adapter.open(api, found, facts)?;
        let resident = Resident::new(
            opened,
            None,
            found.languages.clone(),
            found.voices.clone(),
            None,
        );
        Ok(RemoteModel::new(spec.id, &found.id, resident))
    }

    /// `adapter`'s provider as listed, from what is kept: nothing waits on the provider. What is missing or old is read
    /// in the background and shows on a later call.
    pub(super) async fn listed(&self, adapter: &Arc<dyn Adapter>) -> Listed {
        self.listings.provider(adapter, &self.api(adapter)).await
    }

    /// `adapter`'s provider as listed once it has been read: this waits for its own provider (for a read under way, then
    /// for what is still due, everything with `force`).
    pub(super) async fn settled(&self, adapter: &Arc<dyn Adapter>, force: bool) -> Listed {
        self.listings
            .settled(adapter, &self.api(adapter), force)
            .await
    }

    /// `adapter`'s provider's API, through the host.
    fn api(&self, adapter: &Arc<dyn Adapter>) -> Api {
        Api::new(&self.host, adapter.spec().id)
    }
}
