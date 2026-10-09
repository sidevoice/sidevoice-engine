//! Each provider's listing, kept in memory only (nothing is written anywhere): its models, its voices, when each was
//! listed, and how the last attempt went. A listing is asked for when there is none, when its models are a day old or
//! its voices an hour old (an account's voices change more often: cloned, designed, added), and whenever the app asks
//! ([`Engine::refresh`](crate::Engine::refresh)). Each start of the app begins with none.
//!
//! A key that is missing, refused or not allowed to list drops the listing: the provider then has no models, and there
//! is no fallback to models it might have. A provider that could not be asked keeps the last listing, marked stale.

use std::collections::BTreeMap;
use std::time::Duration;

use web_time::Instant;

use crate::catalog::{Capability, Voice};
use crate::provider::{Adapter, Api, Provider, ProviderModel};
use crate::resolver::Reason;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// How long a listing of models stands.
pub(crate) const MODELS_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// How long a listing of voices stands.
pub(crate) const VOICES_AGE: Duration = Duration::from_secs(60 * 60);

/// Every provider's listing, by id.
#[derive(Default)]
pub(crate) struct Listings(async_lock::Mutex<BTreeMap<&'static str, Listing>>);

/// One provider's listing.
#[derive(Default, Clone)]
struct Listing {
    models: Vec<ProviderModel>,
    models_at: Option<Instant>,
    voices: Vec<Voice>,
    voices_at: Option<Instant>,
    status: Option<&'static str>,
}

impl Listings {
    /// `adapter`'s provider as it stands now: listed again where its listing is missing or old, or everything with
    /// `force`. It waits for any other listing in progress.
    pub(crate) async fn provider(&self, adapter: &dyn Adapter, api: &Api, force: bool) -> Provider {
        let mut listings = self.0.lock().await;
        let listing = listings.entry(adapter.spec().id).or_default();
        let now = Instant::now();
        match api.key().await {
            Ok(_) => {
                if listing.status.is_some_and(unusable) {
                    listing.status = None;
                }
                let mut listed = true;
                if force || due(listing.models_at, now, MODELS_AGE) {
                    let models = adapter.models(api).await;
                    listed = models.is_ok();
                    listing.update(models, |listing, models| {
                        listing.models = models;
                        listing.models_at = Some(now);
                    });
                }
                if listed && (force || due(listing.voices_at, now, VOICES_AGE)) {
                    let voices = adapter.voices(api).await;
                    listing.update(voices, |listing, voices| {
                        listing.voices = voices;
                        listing.voices_at = Some(now);
                    });
                }
            }
            Err(error) => listing.drop_for(error.code),
        }
        listing.describe(adapter)
    }
}

impl Listing {
    /// Keeps `answer` with `keep` when it is one; otherwise records why not, and drops everything for a key that cannot
    /// list.
    fn update<T>(&mut self, answer: Result<T>, keep: impl FnOnce(&mut Self, T)) {
        match answer {
            Ok(answer) => {
                keep(self, answer);
                self.status = None;
            }
            Err(Error { code }) if unusable(code) => self.drop_for(code),
            Err(Error { code }) => self.status = Some(code),
        }
    }

    /// Nothing listed, because of `code`.
    fn drop_for(&mut self, code: &'static str) {
        *self = Self {
            status: Some(code),
            ..Self::default()
        };
    }

    /// The provider as listed: its models with its voices on each text-to-speech one.
    fn describe(&self, adapter: &dyn Adapter) -> Provider {
        let spec = adapter.spec();
        let models = self
            .models
            .iter()
            .cloned()
            .map(|mut model| {
                if model.capabilities.contains(&Capability::Tts) {
                    model.voices.clone_from(&self.voices);
                }
                model
            })
            .collect();
        Provider {
            id: spec.id,
            name: spec.name,
            description: spec.description,
            status: self.status.map(Reason::new),
            stale: self.status.is_some() && self.models_at.is_some(),
            models,
        }
    }
}

/// Whether something listed at `at` (never, with `None`) is to be listed again at `now`, once `age` old.
fn due(at: Option<Instant>, now: Instant, age: Duration) -> bool {
    at.is_none_or(|at| now.saturating_duration_since(at) >= age)
}

/// Whether `code` says the key cannot list at all, so that nothing listed stands.
fn unusable(code: &str) -> bool {
    matches!(
        code,
        "credential-missing" | "credential-rejected" | "listing-not-permitted"
    )
}
