//! The funnel over a fake host and catalogue: what fits is offered, and every other build comes back with why not.

use crate::test_support::{FakeCatalog, FakeHost};
use crate::{Capabilities, Engine, Fetcher, Host, Offer, Reason, Rejection, Runs, Storage, Task};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn an_engine_with_a_fake_host_offers_what_fits_and_says_why_the_rest_does_not() {
    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(FakeCatalog)]).expect("engine");
    let offers = engine.offers(Task::Stt);

    let offered: Vec<_> = offers
        .iter()
        .filter_map(|offer| match offer {
            Offer::Offered { model, build, .. } => Some((model.id.as_str(), build.id.as_str())),
            Offer::Rejected { .. } => None,
        })
        .collect();
    let rejected: Vec<_> = offers
        .iter()
        .filter_map(|offer| match offer {
            Offer::Rejected { build, why, .. } => Some((build.id.as_str(), why.clone())),
            Offer::Offered { .. } => None,
        })
        .collect();

    if cfg!(target_arch = "wasm32") {
        assert_eq!(offered, [("whisper-small", "whisper-small-web")]);
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        assert_eq!(offered, [("whisper-small", "whisper-small-mlx")]);
    } else {
        assert_eq!(offered, [("whisper-small", "whisper-small-onnx")]);
    }
    assert!(rejected.contains(&("whisper-small-gguf", Rejection::BackendNotInThisBuild)));
    let large_rejection = if cfg!(target_arch = "wasm32") {
        // Its only build is native: on the web, its backend does not exist.
        Rejection::BackendNotInThisBuild
    } else {
        Rejection::DoesNotFit(Reason::with_numbers("memory", 16_384, 8_192))
    };
    assert!(rejected.contains(&("whisper-large-onnx", large_rejection)));
    assert!(offers
        .iter()
        .all(|offer| !matches!(offer, Offer::Offered { model, .. } if model.task != Task::Stt)));
}

#[cfg(native)]
#[test]
fn a_native_engine_and_its_futures_can_cross_threads() {
    fn shared<T: Send + Sync>(_: &T) {}
    fn sent<T: Send>(_: T) {}

    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(FakeCatalog)]).expect("engine");
    shared(&engine);
    let selection = engine
        .select(Task::Stt, &crate::Preferences::default())
        .expect("a selection");
    sent(engine.prepare(&selection));
}

/// A native host on a platform backends.json has no entry for.
struct Elsewhere;

impl Host for Elsewhere {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            runs: Runs::Native,
            os: "plan9".to_owned(),
            ..FakeHost.capabilities()
        }
    }

    fn storage(&self) -> &dyn Storage {
        &FakeHost
    }

    fn fetcher(&self) -> &dyn Fetcher {
        &FakeHost
    }
}

#[test]
fn a_platform_with_no_runtime_rejects_every_build_of_this_engine_in_the_funnel() {
    let engine = Engine::new(Box::new(Elsewhere), vec![Box::new(FakeCatalog)]).expect("engine");
    let offers = engine.offers(Task::Stt);
    assert!(!offers.is_empty());
    for offer in offers {
        let Offer::Rejected { build, why, .. } = offer else {
            panic!("nothing runs on plan9: {offer:?}");
        };
        if engine.backends().contains(&build.backend.as_str()) {
            let no_runtime = Reason::new("no-runtime-for-platform");
            assert_eq!(
                why,
                Rejection::BackendUnavailable(no_runtime),
                "{}",
                build.id
            );
        } else {
            assert_eq!(why, Rejection::BackendNotInThisBuild, "{}", build.id);
        }
    }
}
