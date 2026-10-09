//! Each provider's spec facts and listing, kept in memory only (nothing is written anywhere): the facts its OpenAPI
//! spec gives, its models, its voices, when each was read, and how the last attempt went. Each is asked for when there
//! is none, when the facts or the models are a day old or the voices an hour old (an account's voices change more
//! often: cloned, designed, added), and whenever the app asks (a catalogue's `refresh`). Each start of the app begins
//! with none.
//!
//! A key that is missing, refused or not allowed to list drops the listing: the provider then has no models, and there
//! is no fallback to models it might have. A provider that could not be asked keeps the last listing, marked stale. A
//! spec that cannot be read keeps the last facts, and says so (`provider-spec-unreadable`); with no facts yet, the
//! provider has no models.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use crate::catalog::{Capability, Voice};
use crate::provider::{Adapter, Api, Facts, ProviderModel};
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// How long a listing of models, and the facts of a spec, stand.
pub(crate) const MODELS_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// How long a listing of voices stands.
pub(crate) const VOICES_AGE: Duration = Duration::from_secs(60 * 60);

/// Every provider's listing, by id.
#[derive(Default)]
pub(crate) struct Listings(async_lock::Mutex<BTreeMap<&'static str, Listing>>);

/// One provider's facts and listing.
#[derive(Default, Clone)]
struct Listing {
    facts: Option<Arc<Facts>>,
    facts_at: Option<Instant>,
    /// Why the spec was last not read, until it is.
    spec_status: Option<&'static str>,
    models: Vec<ProviderModel>,
    models_at: Option<Instant>,
    voices: Vec<Voice>,
    voices_at: Option<Instant>,
    status: Option<&'static str>,
    /// What the provider said when the listing last failed.
    detail: Option<String>,
}

/// A provider as it stands: how its listing went, its models with their voices, and the facts to make them with.
#[derive(Debug, Clone)]
pub(crate) struct Listed {
    /// Why it is not current, as a stable code; `None` when it is.
    pub(crate) status: Option<&'static str>,
    /// What the provider said when it last refused, for a developer to read.
    pub(crate) detail: Option<String>,
    /// Whether `models` is the last listing kept after a refresh failed.
    pub(crate) stale: bool,
    pub(crate) models: Vec<ProviderModel>,
    pub(crate) facts: Option<Arc<Facts>>,
}

impl Listings {
    /// `adapter`'s provider as it stands now: its spec read and its models and voices listed again where they are
    /// missing or old, or everything with `force`. It waits for any other listing in progress.
    pub(crate) async fn provider(&self, adapter: &dyn Adapter, api: &Api, force: bool) -> Listed {
        let mut listings = self.0.lock().await;
        let listing = listings.entry(adapter.spec().id).or_default();
        let now = Instant::now();
        if let Err(error) = api.key().await {
            listing.drop_for(error.code, None);
            return listing.describe();
        }
        if listing.status.is_some_and(unusable) {
            listing.status = None;
        }
        if force || due(listing.facts_at, now, MODELS_AGE) {
            let read = match api.spec(adapter.spec().spec).await {
                Ok(spec) => adapter.facts(&spec),
                Err(error) => Err(error),
            };
            listing.facts_at = Some(now);
            match read {
                Ok(facts) => {
                    listing.facts = Some(Arc::new(facts));
                    listing.spec_status = None;
                }
                Err(error) => listing.spec_status = Some(error.code),
            }
        }
        let Some(facts) = listing.facts.clone() else {
            listing.models.clear();
            listing.models_at = None;
            return listing.describe();
        };
        let mut listed = true;
        if force || due(listing.models_at, now, MODELS_AGE) {
            let models = adapter.models(api, &facts).await;
            listed = models.is_ok();
            listing.update(models, api, |listing, models| {
                listing.models = models;
                listing.models_at = Some(now);
            });
        }
        if listed && (force || due(listing.voices_at, now, VOICES_AGE)) {
            let voices = adapter.voices(api, &facts).await;
            listing.update(voices, api, |listing, voices| {
                listing.voices = voices;
                listing.voices_at = Some(now);
            });
        }
        listing.describe()
    }
}

impl Listing {
    /// Keeps `answer` with `keep` when it is one; otherwise records why not (and what the provider said), and drops
    /// the listing for a key that cannot list.
    fn update<T>(&mut self, answer: Result<T>, api: &Api, keep: impl FnOnce(&mut Self, T)) {
        match answer {
            Ok(answer) => {
                keep(self, answer);
                self.status = None;
                self.detail = None;
            }
            Err(Error { code }) if unusable(code) => self.drop_for(code, api.detail()),
            Err(Error { code }) => {
                self.status = Some(code);
                self.detail = api.detail();
            }
        }
    }

    /// Nothing listed, because of `code`; the spec's facts, which no key decides, stay.
    fn drop_for(&mut self, code: &'static str, detail: Option<String>) {
        *self = Self {
            facts: self.facts.take(),
            facts_at: self.facts_at,
            spec_status: self.spec_status,
            status: Some(code),
            detail,
            ..Self::default()
        };
    }

    /// The provider as listed: its models with its voices on each text-to-speech one. A listing's own status comes
    /// first; a spec that was not read is said when the listing is otherwise current.
    fn describe(&self) -> Listed {
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
        Listed {
            status: self.status.or(self.spec_status),
            detail: self.detail.clone(),
            stale: self.status.is_some() && self.models_at.is_some(),
            models,
            facts: self.facts.clone(),
        }
    }
}

/// Whether something read at `at` (never, with `None`) is to be read again at `now`, once `age` old.
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
