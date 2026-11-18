//! Asking a provider: the read of its spec, its models and its voices, and running it in the background. The provider's
//! record is locked only to look at it and to put an answer in, never while the provider is being asked.

use std::future::Future;
use std::sync::Arc;

use web_time::Instant;

use super::{lock, Provider};
use crate::provider::{Adapter, Api};

impl Provider {
    /// Starts reading what is due of `adapter`'s provider, on its own, unless a read is under way (one request in flight
    /// for each provider). Nothing waits for it.
    pub(super) fn read_in_background(self: &Arc<Self>, adapter: &Arc<dyn Adapter>, api: &Api) {
        let Some(turn) = self.turn.try_lock_arc() else {
            return;
        };
        let (provider, adapter, api) = (Arc::clone(self), Arc::clone(adapter), api.clone());
        let id = adapter.spec().id;
        spawn(id, async move {
            provider.read(adapter.as_ref(), &api, false).await;
            drop(turn);
        });
    }

    /// Reads what is due of `adapter`'s provider, everything with `force`, and keeps what it says. The caller holds the
    /// provider's turn. A failure is kept as the listing's status, with when it happened, which is what the next call
    /// waits [`RETRY`](super::RETRY) from.
    pub(super) async fn read(&self, adapter: &dyn Adapter, api: &Api, force: bool) {
        let (spec_due, mut facts) = {
            let listing = lock(&self.listing);
            (
                force || listing.spec_due(Instant::now()),
                listing.facts.clone(),
            )
        };
        if spec_due {
            let read = match api.spec(adapter.spec().spec).await {
                Ok(spec) => adapter.facts(&spec),
                Err(error) => Err(error),
            };
            let mut listing = lock(&self.listing);
            let now = Instant::now();
            match read {
                Ok(read) => {
                    let read = Arc::new(read);
                    listing.facts = Some(Arc::clone(&read));
                    listing.facts_at = Some(now);
                    listing.spec_failed_at = None;
                    listing.spec_status = None;
                    facts = Some(read);
                }
                Err(error) => {
                    listing.spec_failed_at = Some(now);
                    listing.spec_status = Some(error.code);
                }
            }
        }
        let Some(facts) = facts else {
            return;
        };
        let models_due = force || lock(&self.listing).models_due(Instant::now());
        let mut listed = true;
        if models_due {
            let models = adapter.models(api, &facts).await;
            listed = models.is_ok();
            lock(&self.listing).update(models, |listing, models| {
                listing.models = models;
                listing.models_at = Some(Instant::now());
            });
        }
        let voices_due = force || lock(&self.listing).voices_due(Instant::now());
        if listed && voices_due {
            let voices = adapter.voices(api, &facts).await;
            lock(&self.listing).update(voices, |listing, voices| {
                listing.voices = voices;
                listing.voices_at = Some(Instant::now());
            });
        }
    }
}

/// Runs `task` on its own, so that nobody waits for it: on the Tokio runtime the native engine's futures expect, or on
/// the page. With no runtime the task is dropped, and so is the provider's turn it holds: nothing is read in the
/// background, and a call that waits (`refresh`) still reads.
#[cfg(native)]
fn spawn(provider: &'static str, task: impl Future<Output = ()> + Send + 'static) {
    match tokio::runtime::Handle::try_current() {
        Ok(runtime) => {
            runtime.spawn(task);
        }
        Err(_) => tracing::warn!(
            provider,
            error.type = "no-runtime",
            "no async runtime to list in the background, so the provider keeps what it has"
        ),
    }
}

/// Runs `task` on its own, so that nobody waits for it: on the page, between its events.
#[cfg(web)]
fn spawn(_provider: &'static str, task: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}
