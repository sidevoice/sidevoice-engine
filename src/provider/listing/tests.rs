//! A provider's listing over a fake adapter: when its spec and its listing are read again, what a failure keeps and
//! what it drops, and the voices on each text-to-speech model.

use std::sync::Mutex;
use std::time::Duration;

use web_time::Instant;

use super::{due, Listings, MODELS_AGE, VOICES_AGE};
use crate::backend::BackendModel;
use crate::provider::{Adapter, Api, Facts, ProviderModel, ProviderSpec};
use crate::test_support::{block_on, remote_api};
use crate::{async_trait, Capability, Error, Result, Voice};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// A provider that lists one model of each kind and one voice, or fails with `failure`; it counts its listings.
#[derive(Default)]
struct Fake {
    failure: Mutex<Option<&'static str>>,
    listings: Mutex<Vec<&'static str>>,
}

const SPEC: ProviderSpec = ProviderSpec {
    id: "fake",
    name: "Fake",
    spec: SPEC_URL,
};

/// Where the fake provider's spec is: a host's provider side answers it with `{"ok": true}`, which the fake reads.
const SPEC_URL: &str = "https://spec.example/openapi.json";

fn listed(id: &str, capability: Capability) -> ProviderModel {
    ProviderModel {
        id: id.into(),
        capabilities: vec![capability],
        languages: Vec::new(),
        voices: Vec::new(),
        speed: None,
    }
}

impl Fake {
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
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Adapter for Fake {
    fn spec(&self) -> &ProviderSpec {
        &SPEC
    }

    fn facts(&self, spec: &serde_json::Value) -> Result<Facts> {
        if spec["ok"] != true {
            return Err(Error::new("provider-spec-unreadable"));
        }
        Ok(Facts {
            speech_to_text: Vec::new(),
            text_to_speech: Vec::new(),
            transcription: crate::provider::facts::ModelFacts::new("", None, None),
            speech: crate::provider::facts::ModelFacts::new("", None, None),
            voices: Vec::new(),
        })
    }

    async fn models(&self, _api: &Api, _facts: &Facts) -> Result<Vec<ProviderModel>> {
        self.answer("models")?;
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
        _model: &ProviderModel,
        _facts: &Facts,
    ) -> Result<Box<dyn BackendModel>> {
        Err(Error::new("not-implemented"))
    }
}

/// The fake provider's API with a key, its spec served, and that provider side.
fn keyed() -> (Api, std::sync::Arc<crate::test_support::FakeProvider>) {
    let (api, keys) = remote_api("fake");
    keys.key("fake", "k");
    keys.answer(SPEC_URL, 200, br#"{"ok": true}"#);
    (api, keys)
}

/// How many times the spec was read.
fn reads(keys: &crate::test_support::FakeProvider) -> usize {
    keys.requests()
        .iter()
        .filter(|request| request.url == SPEC_URL)
        .count()
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

#[test]
fn without_a_key_nothing_is_asked_and_the_provider_has_no_models() {
    let (api, _) = remote_api("fake");
    let fake = Fake::default();
    let provider = block_on(Listings::default().provider(&fake, &api, false));
    assert_eq!(provider.status, Some("credential-missing"));
    assert!(provider.models.is_empty() && !provider.stale);
    assert!(fake.listings().is_empty());
}

#[test]
fn a_listing_is_asked_once_until_forced_and_its_voices_go_on_each_speaker() {
    let (api, _keys) = keyed();
    let (fake, listings) = (Fake::default(), Listings::default());
    let provider = block_on(listings.provider(&fake, &api, false));
    assert_eq!((provider.status, provider.stale), (None, false));
    let voices: Vec<_> = provider
        .models
        .iter()
        .map(|model| model.voices.len())
        .collect();
    assert_eq!(voices, [0, 1], "voices on the text-to-speech model only");
    block_on(listings.provider(&fake, &api, false));
    assert_eq!(
        fake.listings(),
        ["models", "voices"],
        "in memory: not asked again"
    );
    block_on(listings.provider(&fake, &api, true));
    assert_eq!(fake.listings().len(), 4, "forced: both again");
}

#[test]
fn a_provider_not_reached_keeps_its_last_listing_stale_and_a_key_that_cannot_list_drops_it() {
    let (api, _keys) = keyed();
    let (fake, listings) = (Fake::default(), Listings::default());
    block_on(listings.provider(&fake, &api, false));

    fake.fail(Some("provider-unreachable"));
    let provider = block_on(listings.provider(&fake, &api, true));
    assert_eq!(provider.status, Some("provider-unreachable"));
    assert!(provider.stale && provider.models.len() == 2);

    fake.fail(Some("listing-not-permitted"));
    let provider = block_on(listings.provider(&fake, &api, true));
    assert_eq!(provider.status, Some("listing-not-permitted"));
    assert!(!provider.stale && provider.models.is_empty(), "no fallback");

    fake.fail(None);
    let provider = block_on(listings.provider(&fake, &api, false));
    assert_eq!(
        (provider.status, provider.models.len()),
        (None, 2),
        "listed again"
    );
}

#[test]
fn a_key_taken_away_drops_the_listing() {
    let (api, keys) = keyed();
    let (fake, listings) = (Fake::default(), Listings::default());
    block_on(listings.provider(&fake, &api, false));
    keys.forget("fake");
    let provider = block_on(listings.provider(&fake, &api, false));
    assert_eq!(provider.status, Some("credential-missing"));
    assert!(provider.models.is_empty());
}

#[test]
fn the_spec_is_read_with_the_listing_and_once_until_forced() {
    let (api, keys) = keyed();
    let (fake, listings) = (Fake::default(), Listings::default());
    block_on(listings.provider(&fake, &api, false));
    block_on(listings.provider(&fake, &api, false));
    assert_eq!(reads(&keys), 1, "in memory: not read again");
    block_on(listings.provider(&fake, &api, true));
    assert_eq!(reads(&keys), 2, "forced: read again");
}

#[test]
fn a_spec_never_read_leaves_no_models_and_one_no_longer_read_keeps_its_facts() {
    let (api, keys) = remote_api("fake");
    keys.key("fake", "k");
    keys.answer(SPEC_URL, 404, b"");
    let (fake, listings) = (Fake::default(), Listings::default());
    let provider = block_on(listings.provider(&fake, &api, false));
    assert_eq!(provider.status, Some("provider-spec-unreadable"));
    assert!(provider.models.is_empty() && provider.facts.is_none());
    assert!(fake.listings().is_empty(), "no facts, no listing");

    let (api, keys) = keyed();
    let (fake, listings) = (Fake::default(), Listings::default());
    block_on(listings.provider(&fake, &api, false));
    keys.answer(SPEC_URL, 200, br#"{"ok": false}"#);
    let provider = block_on(listings.provider(&fake, &api, true));
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
    let listed = block_on(api.list(request)).map_err(|e| e.code);
    assert_eq!(
        listed,
        Err("listing-not-permitted"),
        "a scoped key, not a bad one"
    );
    assert_eq!(
        api.detail().as_deref(),
        Some("missing_permissions: The API key you used is missing the permission models_read.")
    );
}

/// ENG82-01: a spec that could not be read on the first call is read again by an ordinary call once `SPEC_RETRY` has
/// passed, not only a day later or when forced; within it, it is not read on every call.
#[test]
fn a_spec_not_read_at_first_is_read_again_by_an_ordinary_call_after_a_while() {
    let (api, keys) = remote_api("fake");
    keys.key("fake", "k");
    keys.answer(SPEC_URL, 503, b"");
    let (fake, listings) = (Fake::default(), Listings::default());
    let provider = block_on(listings.provider(&fake, &api, false));
    assert_eq!(provider.status, Some("provider-spec-unreadable"));

    keys.answer(SPEC_URL, 200, br#"{"ok": true}"#);
    let provider = block_on(listings.provider(&fake, &api, false));
    assert!(provider.models.is_empty(), "not read again at once");
    assert_eq!(reads(&keys), 1);

    // As if `SPEC_RETRY` had passed since the failure.
    block_on(async {
        let mut listings = listings.0.lock().await;
        let listing = listings.get_mut("fake").expect("listed");
        let failed = listing.spec_failed_at.expect("a failure");
        listing.spec_failed_at = failed.checked_sub(super::SPEC_RETRY);
    });
    let provider = block_on(listings.provider(&fake, &api, false));
    assert_eq!(
        (provider.status, provider.models.len()),
        (None, 2),
        "recovered"
    );
    assert_eq!(reads(&keys), 2);
}
