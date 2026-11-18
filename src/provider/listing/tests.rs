//! A provider's listing over a fake adapter: when its spec and its listing are read again, what a failure keeps and
//! what it drops, the voices on each text-to-speech model, and that nobody waits for a provider that does not answer.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use web_time::Instant;

use super::{due, lock, Listing, Listings, MODELS_AGE, RETRY, VOICES_AGE};
use crate::backend::BackendModel;
use crate::provider::{Adapter, Api, Facts, ProviderSpec, RemoteModelInfo};
use crate::test_support::{block_on, remote_api, FakeProvider};
use crate::{async_trait, Capability, Error, Result, Voice};

#[cfg(native)]
use crate::test_support::ready;
#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// A provider that lists one model of each kind and one voice, or fails with `failure`; it counts its listings, and its
/// models wait for as long as a test holds `gate`.
struct Fake {
    spec: ProviderSpec,
    failure: Mutex<Option<&'static str>>,
    listings: Mutex<Vec<&'static str>>,
    gate: Arc<async_lock::Mutex<()>>,
}

/// Where the fake providers' spec is: a host's provider side answers it with `{"ok": true}`, which the fake reads.
const SPEC_URL: &str = "https://spec.example/openapi.json";

fn listed(id: &str, capability: Capability) -> RemoteModelInfo {
    RemoteModelInfo {
        id: id.into(),
        capabilities: vec![capability],
        languages: Vec::new(),
        voices: Vec::new(),
        speed: None,
    }
}

impl Fake {
    fn named(id: &'static str) -> Arc<Self> {
        Arc::new(Self {
            spec: ProviderSpec {
                id,
                name: "Fake",
                spec: SPEC_URL,
            },
            failure: Mutex::default(),
            listings: Mutex::default(),
            gate: Arc::default(),
        })
    }

    fn answer(&self, what: &'static str) -> Result<()> {
        self.listings.lock().unwrap().push(what);
        match *self.failure.lock().unwrap() {
            Some(code) => Err(Error::new(code)),
            None => Ok(()),
        }
    }

    fn fail(&self, code: Option<&'static str>) {
        *self.failure.lock().unwrap() = code;
    }

    fn listings(&self) -> Vec<&'static str> {
        self.listings.lock().unwrap().clone()
    }

    /// The provider as the engine holds it.
    fn adapter(self: &Arc<Self>) -> Arc<dyn Adapter> {
        Arc::clone(self) as Arc<dyn Adapter>
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Adapter for Fake {
    fn spec(&self) -> &ProviderSpec {
        &self.spec
    }

    fn facts(&self, spec: &serde_json::Value) -> Result<Facts> {
        if spec["ok"] != true {
            return Err(Error::new("provider-spec-unreadable"));
        }
        Ok(facts())
    }

    async fn models(&self, _api: &Api, _facts: &Facts) -> Result<Vec<RemoteModelInfo>> {
        self.answer("models")?;
        drop(self.gate.lock().await);
        Ok(vec![
            listed("ear", Capability::Stt),
            listed("mouth", Capability::Tts),
        ])
    }

    async fn voices(&self, _api: &Api, _facts: &Facts) -> Result<Vec<Voice>> {
        self.answer("voices")?;
        Ok(vec![Voice {
            id: "v".into(),
            name: Some("Vee".into()),
            languages: vec!["es".into()],
            gender: None,
        }])
    }

    fn open(
        &self,
        _api: Api,
        _model: &RemoteModelInfo,
        _facts: &Facts,
    ) -> Result<Box<dyn BackendModel>> {
        Err(Error::new("not-implemented"))
    }
}

fn facts() -> Facts {
    Facts {
        speech_to_text: Vec::new(),
        text_to_speech: Vec::new(),
        transcription: crate::provider::facts::ModelFacts::new("", None, None),
        speech: crate::provider::facts::ModelFacts::new("", None, None),
        voices: Vec::new(),
    }
}

/// Provider `id`'s API with a key, its spec served, and that provider side.
fn keyed_as(id: &'static str) -> (Api, Arc<FakeProvider>) {
    let (api, keys) = remote_api(id);
    keys.key(id, "k");
    keys.answer(SPEC_URL, 200, br#"{"ok": true}"#);
    (api, keys)
}

/// The fake provider's API with a key, its spec served, and that provider side.
fn keyed() -> (Api, Arc<FakeProvider>) {
    keyed_as("fake")
}

/// How many times the spec was read.
fn reads(keys: &FakeProvider) -> usize {
    keys.requests()
        .iter()
        .filter(|request| request.url == SPEC_URL)
        .count()
}

/// As if `RETRY` had passed since the spec, or a listing, last failed.
fn let_retry_pass(listings: &Listings, id: &'static str) {
    let provider = listings.of(id);
    let mut guard = lock(&provider.listing);
    let listing: &mut Listing = &mut guard;
    for failed in [&mut listing.spec_failed_at, &mut listing.failed_at] {
        *failed = failed.and_then(|at| at.checked_sub(RETRY));
    }
}

#[test]
fn a_listing_is_due_when_missing_or_once_its_age_has_passed() {
    let now = Instant::now() + Duration::from_secs(48 * 60 * 60);
    assert!(due(None, now, MODELS_AGE));
    assert!(!due(Some(now - Duration::from_secs(60)), now, VOICES_AGE));
    assert!(due(Some(now - VOICES_AGE), now, VOICES_AGE));
    assert!(!due(Some(now - VOICES_AGE), now, MODELS_AGE));
    assert!(due(Some(now - MODELS_AGE), now, MODELS_AGE));
}

/// The attempt is recorded when it fails, so a listing that does not work is not asked again on every call.
#[test]
fn a_listing_that_failed_is_due_again_only_once_the_retry_has_passed() {
    let mut listing = Listing {
        facts: Some(Arc::new(facts())),
        facts_at: Some(Instant::now()),
        ..Listing::default()
    };
    assert!(listing.models_due(Instant::now()), "never listed");
    listing.update(
        Result::<()>::Err(Error::new("provider-unreachable")),
        |_, ()| {},
    );
    assert!(!listing.due(Instant::now()), "just failed");
    assert!(!listing.models_due(Instant::now()) && !listing.voices_due(Instant::now()));
    assert!(listing.due(Instant::now() + RETRY), "tried again after");
    listing.update(Ok(()), |listing, ()| {
        listing.models_at = Some(Instant::now());
    });
    assert!(!listing.models_due(Instant::now() + RETRY), "listed");

    // A kept listing a day old that then fails is not asked again at once either.
    let listed_at = Instant::now();
    listing.models_at = Some(listed_at);
    listing.failed_at = Some(listed_at + MODELS_AGE);
    assert!(!listing.models_due(listed_at + MODELS_AGE + Duration::from_secs(1)));
    assert!(listing.models_due(listed_at + MODELS_AGE + RETRY));
}

/// A key the provider refuses is not asked about again on every call either.
#[test]
fn a_key_refused_is_asked_about_again_only_once_the_retry_has_passed() {
    let mut listing = Listing {
        facts: Some(Arc::new(facts())),
        facts_at: Some(Instant::now()),
        ..Listing::default()
    };
    listing.update(
        Result::<()>::Err(Error::new("credential-rejected")),
        |_, ()| {},
    );
    assert_eq!(listing.describe().status, Some("credential-rejected"));
    assert!(!listing.due(Instant::now()));
    assert!(listing.due(Instant::now() + RETRY));
}

/// What is served before a provider's first listing: nothing, from a provider that reads as not reached.
#[test]
fn a_provider_never_listed_has_no_models_and_reads_as_not_reached() {
    let mut listing = Listing::default();
    let never = listing.describe();
    assert_eq!(never.status, Some("provider-unreachable"));
    assert!(never.models.is_empty() && !never.stale);

    listing.update(Ok(()), |listing, ()| {
        listing.models_at = Some(Instant::now());
    });
    let current = listing.describe();
    assert_eq!((current.status, current.stale), (None, false));

    listing.update(
        Result::<()>::Err(Error::new("provider-unreachable")),
        |_, ()| {},
    );
    let stale = listing.describe();
    assert_eq!(stale.status, Some("provider-unreachable"));
    assert!(stale.stale, "its last listing, kept");
}

#[test]
fn without_a_key_nothing_is_asked_and_the_provider_has_no_models() {
    let (api, _) = remote_api("fake");
    let fake = Fake::named("fake");
    let provider = block_on(Listings::default().provider(&fake.adapter(), &api));
    assert_eq!(provider.status, Some("credential-missing"));
    assert!(provider.models.is_empty() && !provider.stale);
    assert!(fake.listings().is_empty());
}

#[test]
fn a_listing_is_asked_once_until_forced_and_its_voices_go_on_each_speaker() {
    let (api, _keys) = keyed();
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    let provider = block_on(listings.settled(&fake.adapter(), &api, false));
    assert_eq!((provider.status, provider.stale), (None, false));
    let voices: Vec<_> = provider
        .models
        .iter()
        .map(|model| model.voices.len())
        .collect();
    assert_eq!(voices, [0, 1], "voices on the text-to-speech model only");
    block_on(listings.settled(&fake.adapter(), &api, false));
    assert_eq!(
        fake.listings(),
        ["models", "voices"],
        "in memory: not asked again"
    );
    block_on(listings.settled(&fake.adapter(), &api, true));
    assert_eq!(fake.listings().len(), 4, "forced: both again");
}

#[test]
fn a_provider_not_reached_keeps_its_last_listing_stale_and_a_key_that_cannot_list_drops_it() {
    let (api, _keys) = keyed();
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    let adapter = fake.adapter();
    block_on(listings.settled(&adapter, &api, false));

    fake.fail(Some("provider-unreachable"));
    let provider = block_on(listings.settled(&adapter, &api, true));
    assert_eq!(provider.status, Some("provider-unreachable"));
    assert!(provider.stale && provider.models.len() == 2);

    fake.fail(Some("listing-not-permitted"));
    let provider = block_on(listings.settled(&adapter, &api, true));
    assert_eq!(provider.status, Some("listing-not-permitted"));
    assert!(!provider.stale && provider.models.is_empty(), "no fallback");

    fake.fail(None);
    let asked = fake.listings().len();
    let provider = block_on(listings.settled(&adapter, &api, false));
    assert_eq!(
        provider.status,
        Some("listing-not-permitted"),
        "not asked again until the retry has passed"
    );
    assert_eq!(fake.listings().len(), asked);
    let_retry_pass(&listings, "fake");
    let provider = block_on(listings.settled(&adapter, &api, false));
    assert_eq!(
        (provider.status, provider.models.len()),
        (None, 2),
        "listed again"
    );
}

#[test]
fn a_key_taken_away_drops_the_listing() {
    let (api, keys) = keyed();
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    block_on(listings.settled(&fake.adapter(), &api, false));
    keys.forget("fake");
    let provider = block_on(listings.provider(&fake.adapter(), &api));
    assert_eq!(provider.status, Some("credential-missing"));
    assert!(provider.models.is_empty());
}

#[test]
fn a_key_given_after_none_is_listed_with_at_once() {
    let (api, keys) = keyed();
    keys.forget("fake");
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    let adapter = fake.adapter();
    let provider = block_on(listings.settled(&adapter, &api, false));
    assert_eq!(provider.status, Some("credential-missing"));

    keys.key("fake", "k");
    let provider = block_on(listings.settled(&adapter, &api, false));
    assert_eq!(
        (provider.status, provider.models.len()),
        (None, 2),
        "no retry to wait for: nothing was asked before"
    );
}

#[test]
fn the_spec_is_read_with_the_listing_and_once_until_forced() {
    let (api, keys) = keyed();
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    let adapter = fake.adapter();
    block_on(listings.settled(&adapter, &api, false));
    block_on(listings.settled(&adapter, &api, false));
    assert_eq!(reads(&keys), 1, "in memory: not read again");
    block_on(listings.settled(&adapter, &api, true));
    assert_eq!(reads(&keys), 2, "forced: read again");
}

#[test]
fn a_spec_never_read_leaves_no_models_and_one_no_longer_read_keeps_its_facts() {
    let (api, keys) = remote_api("fake");
    keys.key("fake", "k");
    keys.answer(SPEC_URL, 404, b"");
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    let provider = block_on(listings.settled(&fake.adapter(), &api, false));
    assert_eq!(provider.status, Some("provider-spec-unreadable"));
    assert!(provider.models.is_empty() && provider.facts.is_none());
    assert!(fake.listings().is_empty(), "no facts, no listing");

    let (api, keys) = keyed();
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    let adapter = fake.adapter();
    block_on(listings.settled(&adapter, &api, false));
    keys.answer(SPEC_URL, 200, br#"{"ok": false}"#);
    let provider = block_on(listings.settled(&adapter, &api, true));
    assert_eq!(provider.status, Some("provider-spec-unreadable"));
    assert_eq!(provider.models.len(), 2, "listed with the last facts");
    assert!(provider.facts.is_some() && !provider.stale);
}

#[test]
fn a_listing_refused_keeps_what_the_provider_said() {
    let (api, keys) = keyed();
    keys.answer(
        "https://provider.example/",
        401,
        br#"{"detail": {"status": "missing_permissions", "message": "The API key you used is missing the permission models_read."}}"#,
    );
    let request = crate::host::HttpRequest {
        method: "GET",
        url: "https://provider.example/v1/models".into(),
        headers: Vec::new(),
        body: Vec::new(),
    };
    let refused = block_on(api.list(request)).map(drop).unwrap_err();
    assert_eq!(
        refused.code, "listing-not-permitted",
        "a scoped key, not a bad one"
    );
    assert_eq!(
        refused.detail.as_deref(),
        Some("missing_permissions: The API key you used is missing the permission models_read.")
    );
}

/// ENG82-01: a spec that could not be read on the first call is read again by an ordinary call once `RETRY` has
/// passed, not only a day later or when forced; within it, it is not read on every call.
#[test]
fn a_spec_not_read_at_first_is_read_again_by_an_ordinary_call_after_a_while() {
    let (api, keys) = remote_api("fake");
    keys.key("fake", "k");
    keys.answer(SPEC_URL, 503, b"");
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    let adapter = fake.adapter();
    let provider = block_on(listings.settled(&adapter, &api, false));
    assert_eq!(provider.status, Some("provider-spec-unreadable"));

    keys.answer(SPEC_URL, 200, br#"{"ok": true}"#);
    let provider = block_on(listings.settled(&adapter, &api, false));
    assert!(provider.models.is_empty(), "not read again at once");
    assert_eq!(reads(&keys), 1);

    let_retry_pass(&listings, "fake");
    let provider = block_on(listings.settled(&adapter, &api, false));
    assert_eq!(
        (provider.status, provider.models.len()),
        (None, 2),
        "recovered"
    );
    assert_eq!(reads(&keys), 2);
}

/// A listing that fails is recorded as an attempt: the next calls do not ask the provider again until `RETRY` has
/// passed, whether it was never listed or its last listing is kept.
#[test]
fn a_failed_attempt_backs_off_and_the_next_one_after_the_retry_recovers() {
    let (api, _keys) = keyed();
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    let adapter = fake.adapter();
    fake.fail(Some("provider-unreachable"));
    let provider = block_on(listings.settled(&adapter, &api, false));
    assert_eq!(provider.status, Some("provider-unreachable"));
    assert!(provider.models.is_empty() && !provider.stale);
    for _ in 0..3 {
        block_on(listings.settled(&adapter, &api, false));
    }
    assert_eq!(fake.listings(), ["models"], "asked once, not on every call");

    fake.fail(None);
    let_retry_pass(&listings, "fake");
    let provider = block_on(listings.settled(&adapter, &api, false));
    assert_eq!(provider.status, None, "recovered");
    assert_eq!(provider.models.len(), 2);

    // With a listing kept: it is served stale while the provider is not reached.
    fake.fail(Some("provider-unreachable"));
    let provider = block_on(listings.settled(&adapter, &api, true));
    assert!(provider.stale && provider.models.len() == 2);
    assert_eq!(provider.status, Some("provider-unreachable"));
}

/// What a provider that never answers is served: nothing, at once, from a provider that reads as not reached. The
/// listing it was asked for is the only request it has, however many calls ask meanwhile.
///
/// Native only: the read runs on the runtime, which the web's single-threaded test harness cannot drive.
#[cfg(native)]
#[test]
fn a_provider_that_does_not_answer_is_served_unreachable_at_once_and_asked_once() {
    let (api, _keys) = keyed();
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    let adapter = fake.adapter();
    let held = block_on(fake.gate.lock_arc());
    let provider = block_on(async {
        // Ready on its first poll: nothing waits.
        let first = ready(listings.provider(&adapter, &api));
        for _ in 0..5 {
            ready(listings.provider(&adapter, &api));
        }
        first
    });
    assert_eq!(provider.status, Some("provider-unreachable"));
    assert!(provider.models.is_empty() && !provider.stale);
    wait_until(|| fake.listings() == ["models"]);
    block_on(async {
        for _ in 0..5 {
            ready(listings.provider(&adapter, &api));
        }
    });
    assert_eq!(
        fake.listings(),
        ["models"],
        "one read in flight for the provider"
    );
    drop(held);
}

/// What the background read finds is served on a later call, and replaces what was served.
#[cfg(native)]
#[test]
fn what_the_read_in_the_background_finds_is_served_on_a_later_call() {
    let (api, _keys) = keyed();
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    let adapter = fake.adapter();
    let held = block_on(fake.gate.lock_arc());
    let served = block_on(async { ready(listings.provider(&adapter, &api)) });
    assert!(served.models.is_empty());
    wait_until(|| fake.listings() == ["models"]);
    drop(held);

    // Waits for the read under way, which lists: it asks nothing more.
    let settled = block_on(listings.settled(&adapter, &api, false));
    assert_eq!(settled.models.len(), 2);
    let again = block_on(async { ready(listings.provider(&adapter, &api)) });
    assert_eq!((again.status, again.models.len()), (None, 2));
    assert_eq!(fake.listings(), ["models", "voices"], "read once");
}

/// A call that waits for a listing (a refresh, after a new key) does not take the read already under way for its own:
/// that one may have used the key before, so it waits for it and then reads again.
#[cfg(native)]
#[test]
fn a_forced_read_waits_for_the_read_under_way_and_then_reads_again() {
    let (api, _keys) = keyed();
    let (fake, listings) = (Fake::named("fake"), Arc::new(Listings::default()));
    let adapter = fake.adapter();
    let held = block_on(fake.gate.lock_arc());
    block_on(async { ready(listings.provider(&adapter, &api)) });
    wait_until(|| fake.listings() == ["models"]);
    let (waiting_listings, waiting_adapter, waiting_api) =
        (Arc::clone(&listings), Arc::clone(&adapter), api.clone());
    let settled = block_on(async move {
        let waiting = tokio::spawn(async move {
            waiting_listings
                .settled(&waiting_adapter, &waiting_api, true)
                .await
        });
        drop(held);
        waiting.await.expect("the forced read")
    });
    assert_eq!(settled.models.len(), 2);
    assert_eq!(
        fake.listings(),
        ["models", "voices", "models", "voices"],
        "the one under way, then its own"
    );
}

/// A provider that does not answer holds up no other: not its catalogue read, not its refresh.
#[cfg(native)]
#[test]
fn providers_do_not_wait_for_one_another() {
    let (stuck_api, _) = keyed_as("stuck");
    let (fine_api, _) = keyed_as("fine");
    let (stuck, fine) = (Fake::named("stuck"), Fake::named("fine"));
    let listings = Listings::default();
    let held = block_on(stuck.gate.lock_arc());
    block_on(async {
        ready(listings.provider(&stuck.adapter(), &stuck_api));
    });
    wait_until(|| stuck.listings() == ["models"]);

    // Ready on their first poll, with the other provider's read in flight: a refresh, then a plain read.
    let (refreshed, read) = block_on(async {
        let refreshed = ready(listings.settled(&fine.adapter(), &fine_api, true));
        let read = ready(listings.provider(&fine.adapter(), &fine_api));
        (refreshed, read)
    });
    assert_eq!(
        (refreshed.status, refreshed.models.len()),
        (None, 2),
        "listed while the other one waits"
    );
    assert_eq!((read.status, read.models.len()), (None, 2));
    drop(held);
}

/// With no runtime to read on, nothing is read in the background and nobody is left waiting: the provider's turn is
/// free for a call that waits.
#[cfg(native)]
#[test]
fn without_a_runtime_nothing_is_read_in_the_background_and_a_call_that_waits_still_reads() {
    let (api, _keys) = keyed();
    let (fake, listings) = (Fake::named("fake"), Listings::default());
    let adapter = fake.adapter();
    let served = ready(listings.provider(&adapter, &api));
    assert_eq!(served.status, Some("provider-unreachable"));
    assert!(fake.listings().is_empty());
    let settled = block_on(listings.settled(&adapter, &api, false));
    assert_eq!(settled.models.len(), 2);
}

/// Waits until `condition` holds, for what runs on the runtime to get there.
#[cfg(native)]
fn wait_until(condition: impl Fn() -> bool) {
    for _ in 0..1_000 {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("what the background read does did not happen in ten seconds");
}
