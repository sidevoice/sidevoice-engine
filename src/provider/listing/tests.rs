//! A provider's listing over a fake adapter: when it is asked again, what a failure keeps and what it drops, and the
//! voices on each text-to-speech model.

use std::sync::Mutex;
use std::time::Duration;

use web_time::Instant;

use super::{due, Listings, MODELS_AGE, VOICES_AGE};
use crate::backend::BackendModel;
use crate::provider::{Adapter, Api, ProviderModel, ProviderSpec};
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
    description: "A fake provider.",
};

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

    async fn models(&self, _api: &Api) -> Result<Vec<ProviderModel>> {
        self.answer("models")?;
        Ok(vec![
            listed("ear", Capability::Stt),
            listed("mouth", Capability::Tts),
        ])
    }

    async fn voices(&self, _api: &Api) -> Result<Vec<Voice>> {
        self.answer("voices")?;
        Ok(vec![Voice {
            id: "v".into(),
            name: Some("Vee".into()),
            languages: vec!["es".into()],
            gender: None,
        }])
    }

    fn open(&self, _api: Api, _model: &ProviderModel) -> Result<Box<dyn BackendModel>> {
        Err(Error::new("not-implemented"))
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

#[test]
fn without_a_key_nothing_is_asked_and_the_provider_has_no_models() {
    let (api, _) = remote_api("fake");
    let fake = Fake::default();
    let provider = block_on(Listings::default().provider(&fake, &api, false));
    assert_eq!(
        provider.status.map(|reason| reason.code),
        Some("credential-missing")
    );
    assert!(provider.models.is_empty() && !provider.stale);
    assert!(fake.listings().is_empty());
}

#[test]
fn a_listing_is_asked_once_until_forced_and_its_voices_go_on_each_speaker() {
    let (api, keys) = remote_api("fake");
    keys.key("fake", "k");
    let (fake, listings) = (Fake::default(), Listings::default());
    let provider = block_on(listings.provider(&fake, &api, false));
    assert_eq!(
        (provider.id, provider.status, provider.stale),
        ("fake", None, false)
    );
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
    let (api, keys) = remote_api("fake");
    keys.key("fake", "k");
    let (fake, listings) = (Fake::default(), Listings::default());
    block_on(listings.provider(&fake, &api, false));

    fake.fail(Some("provider-unreachable"));
    let provider = block_on(listings.provider(&fake, &api, true));
    assert_eq!(
        provider.status.map(|reason| reason.code),
        Some("provider-unreachable")
    );
    assert!(provider.stale && provider.models.len() == 2);

    fake.fail(Some("listing-not-permitted"));
    let provider = block_on(listings.provider(&fake, &api, true));
    assert_eq!(
        provider.status.map(|reason| reason.code),
        Some("listing-not-permitted")
    );
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
    let (api, keys) = remote_api("fake");
    keys.key("fake", "k");
    let (fake, listings) = (Fake::default(), Listings::default());
    block_on(listings.provider(&fake, &api, false));
    keys.forget("fake");
    let provider = block_on(listings.provider(&fake, &api, false));
    assert_eq!(
        provider.status.map(|reason| reason.code),
        Some("credential-missing")
    );
    assert!(provider.models.is_empty());
}
