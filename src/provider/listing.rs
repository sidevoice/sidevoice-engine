//! Each provider's spec facts and listing, kept in memory only (nothing is written anywhere): the facts its OpenAPI
//! spec gives, its models, its voices, when each was read, and how the last attempt went. Each is read when there is
//! none, when the facts or the models are a day old or the voices an hour (an account's voices change more often:
//! cloned, designed, added), and whenever the app asks (a catalogue's `refresh`). Each start of the app begins with
//! none.
//!
//! **Nobody waits for a provider to answer a read.** [`Listings::provider`] hands back what is kept at once, and what is
//! due is read in the background, one read at a time for each provider (the provider's turn): it shows on a later
//! call. A provider that has never been listed has no models and reads as one that could not be reached
//! (`provider-unreachable`) until its first listing lands. Only what asks to wait does, and only for its own provider:
//! a catalogue's `refresh`, and loading a model of a provider that has no listing yet ([`Listings::settled`]). No lock
//! is held while a provider is asked, so a provider that does not answer holds up nobody else.
//!
//! A key that is missing, refused or not allowed to list drops the listing: the provider then has no models, and there
//! is no fallback to models it might have. A provider that could not be asked keeps the last listing, marked stale. A
//! spec that cannot be read keeps the last facts, and says so (`provider-spec-unreadable`); with no facts yet, the
//! provider has no models. Either way, and for a listing that failed, it is tried again a minute later, on the next call
//! after that ([`RETRY`]), not only a day later, and not on every call while the provider does not answer.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use web_time::Instant;

use crate::catalog::{Capability, Voice};
use crate::provider::{Adapter, Api, Facts, RemoteModelInfo};
use crate::{Error, Result};

mod refresh;
#[cfg(test)]
mod tests;

/// How long a listing of models, and the facts of a spec, stand.
pub(crate) const MODELS_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// How long a listing of voices stands.
pub(crate) const VOICES_AGE: Duration = Duration::from_secs(60 * 60);
/// How long after a spec could not be read, or a listing could not be made, it is tried again: soon, so that facts never
/// read, or old, and a listing that failed, recover once the provider answers, without asking on every call while it
/// does not. Only the app's own `refresh` does not wait for it.
pub(crate) const RETRY: Duration = Duration::from_secs(60);

/// Every provider's listing, by id.
#[derive(Default)]
pub(crate) struct Listings(Mutex<BTreeMap<&'static str, Arc<Provider>>>);

/// One provider's listing, and its turn: who may ask the provider now.
#[derive(Default)]
struct Provider {
    /// What is known. Held only to read or change it, never while the provider is asked.
    listing: Mutex<Listing>,
    /// Held by whoever is asking the provider (a read in the background, or one the app waits for): one at a time, so a
    /// provider has one request in flight for its listing, and what waits for it waits for its own provider alone.
    turn: Arc<async_lock::Mutex<()>>,
}

/// One provider's facts and listing.
#[derive(Default, Clone)]
struct Listing {
    facts: Option<Arc<Facts>>,
    /// When the spec was last read.
    facts_at: Option<Instant>,
    /// When the spec last could not be read, until it is.
    spec_failed_at: Option<Instant>,
    /// Why the spec was last not read, until it is.
    spec_status: Option<&'static str>,
    models: Vec<RemoteModelInfo>,
    models_at: Option<Instant>,
    voices: Vec<Voice>,
    voices_at: Option<Instant>,
    /// When the models or the voices last could not be listed, until they are.
    failed_at: Option<Instant>,
    status: Option<&'static str>,
    /// What the provider said when the listing last failed.
    detail: Option<String>,
}

/// A provider as it stands: how its listing went, its models with their voices, and the facts to make them with.
#[derive(Clone)]
pub(crate) struct Listed {
    /// Why it is not current, as a stable code; `None` when it is.
    pub(crate) status: Option<&'static str>,
    /// What the provider said when it last refused, for a developer to read.
    pub(crate) detail: Option<String>,
    /// Whether `models` is the last listing kept after a refresh failed.
    pub(crate) stale: bool,
    pub(crate) models: Vec<RemoteModelInfo>,
    pub(crate) facts: Option<Arc<Facts>>,
}

impl std::fmt::Debug for Listed {
    /// Without `detail`, the provider's own words: its length only.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listed")
            .field("status", &self.status)
            .field(
                "detail",
                &self
                    .detail
                    .as_ref()
                    .map(|detail| format!("<{} chars>", detail.chars().count())),
            )
            .field("stale", &self.stale)
            .field("models", &self.models.len())
            .field("facts", &self.facts.is_some())
            .finish()
    }
}

impl Listings {
    /// `adapter`'s provider as it stands now, from what is kept: it makes no request and waits for none, not even for
    /// one under way. What is missing or old is read in the background, and shows on a later call. Without a key the
    /// provider has no models (`credential-missing`) and nothing is asked.
    pub(crate) async fn provider(&self, adapter: &Arc<dyn Adapter>, api: &Api) -> Listed {
        let provider = self.of(adapter.spec().id);
        if let Some(listed) = provider.keyless(api).await {
            return listed;
        }
        let (listed, due) = {
            let listing = lock(&provider.listing);
            (listing.describe(), listing.due(Instant::now()))
        };
        if due {
            provider.read_in_background(adapter, api);
        }
        listed
    }

    /// `adapter`'s provider as it stands once it has been read: this waits for the read under way, if any, and then
    /// reads what is still due, or everything with `force` (a read that starts after this call, which a new key needs).
    /// It waits for its own provider alone.
    pub(crate) async fn settled(
        &self,
        adapter: &Arc<dyn Adapter>,
        api: &Api,
        force: bool,
    ) -> Listed {
        let provider = self.of(adapter.spec().id);
        if let Some(listed) = provider.keyless(api).await {
            return listed;
        }
        let _turn = provider.turn.lock_arc().await;
        provider.read(adapter.as_ref(), api, force).await;
        let listing = lock(&provider.listing);
        listing.describe()
    }

    /// `adapter`'s provider as a model is made from it: as [`Listings::provider`] has it, except that a provider never
    /// listed is waited for ([`Listings::settled`]), since a model is made from a listing and there is nothing to make
    /// it from before. It waits for its own provider alone, and for no listing that has one.
    pub(crate) async fn for_model(&self, adapter: &Arc<dyn Adapter>, api: &Api) -> Listed {
        let never_listed = lock(&self.of(adapter.spec().id).listing)
            .models_at
            .is_none();
        if never_listed {
            self.settled(adapter, api, false).await
        } else {
            self.provider(adapter, api).await
        }
    }

    /// The record of the provider `id`, made when it is first asked for.
    fn of(&self, id: &'static str) -> Arc<Provider> {
        Arc::clone(lock(&self.0).entry(id).or_default())
    }
}

impl Provider {
    /// The provider as listed when the host has no key for it, which drops whatever was listed (the listing was of a key
    /// that is gone); `None` when it has one.
    async fn keyless(&self, api: &Api) -> Option<Listed> {
        let key = api.key().await;
        let mut listing = lock(&self.listing);
        if let Err(error) = key {
            listing.drop_for(error.code, None, None);
            return Some(listing.describe());
        }
        if listing.status == Some("credential-missing") {
            listing.status = None;
        }
        None
    }
}

impl Listing {
    /// Whether the spec is to be read again at `now`: it is a day old, or never read, and not tried a minute ago.
    fn spec_due(&self, now: Instant) -> bool {
        due(self.facts_at, now, MODELS_AGE) && due(self.spec_failed_at, now, RETRY)
    }

    /// Whether the models are to be listed again at `now`: there are facts to list them by, they are a day old or
    /// never listed, and no listing failed a minute ago.
    fn models_due(&self, now: Instant) -> bool {
        self.facts.is_some()
            && due(self.models_at, now, MODELS_AGE)
            && due(self.failed_at, now, RETRY)
    }

    /// Whether the voices are to be listed again at `now`, as the models are, at an hour.
    fn voices_due(&self, now: Instant) -> bool {
        self.facts.is_some()
            && due(self.voices_at, now, VOICES_AGE)
            && due(self.failed_at, now, RETRY)
    }

    /// Whether anything is to be read again at `now`.
    fn due(&self, now: Instant) -> bool {
        self.spec_due(now) || self.models_due(now) || self.voices_due(now)
    }

    /// Keeps `answer` with `keep` when it is one; otherwise records when and why not (and what the provider said, its
    /// error's `detail`), and drops the listing for a key that cannot list.
    fn update<T>(&mut self, answer: Result<T>, keep: impl FnOnce(&mut Self, T)) {
        match answer {
            Ok(answer) => {
                keep(self, answer);
                self.status = None;
                self.detail = None;
                self.failed_at = None;
            }
            // A key that went missing while the provider was being asked was not refused by it: nothing to back off from.
            Err(Error { code, detail }) if unusable(code) => {
                let refused = (code != "credential-missing").then(Instant::now);
                self.drop_for(code, detail, refused);
            }
            Err(Error { code, detail }) => {
                self.status = Some(code);
                self.detail = detail;
                self.failed_at = Some(Instant::now());
            }
        }
    }

    /// Nothing listed, because of `code`; the spec's facts, which no key decides, stay. `failed_at` is when the provider
    /// was asked and refused (`None` for a key the host does not have: nothing was asked, and the next call lists).
    fn drop_for(&mut self, code: &'static str, detail: Option<String>, failed_at: Option<Instant>) {
        *self = Self {
            facts: self.facts.take(),
            facts_at: self.facts_at,
            spec_failed_at: self.spec_failed_at,
            spec_status: self.spec_status,
            failed_at,
            status: Some(code),
            detail,
            ..Self::default()
        };
    }

    /// The provider as listed: its models with its voices on each text-to-speech one. A listing's own status comes
    /// first; a spec that was not read is said when the listing is otherwise current; and a provider that has never
    /// been listed, with nothing said why (its first listing is yet to land), reads as one not reached.
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
        let known = self.models_at.is_some();
        let status = self
            .status
            .or(self.spec_status)
            .or_else(|| (!known).then_some("provider-unreachable"));
        Listed {
            status,
            detail: self.detail.clone(),
            stale: self.status.is_some() && known,
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

/// `mutex`'s contents, whatever a panic elsewhere left: what it guards is plain data, set field by field.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
