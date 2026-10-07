//! The funnel over a fake host and catalogue: what fits is offered, and every other build comes back with why not.

use crate::test_support::{FakeCatalog, FakeHost};
use crate::{
    BundledCatalog, Capabilities, Capability, Engine, Fetcher, Host, Offer, Reason, Rejection,
    Runs, Storage,
};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn an_engine_with_a_fake_host_offers_what_fits_and_says_why_the_rest_does_not() {
    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(FakeCatalog)]).expect("engine");
    let offers = engine.offers(Capability::Stt);

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
        .all(|offer| !matches!(offer, Offer::Offered { model, .. } if !model.capabilities.contains(&Capability::Stt))));
}

#[cfg(native)]
#[test]
fn a_native_engine_and_its_futures_can_cross_threads() {
    fn shared<T: Send + Sync>(_: &T) {}
    fn sent<T: Send>(_: T) {}

    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(FakeCatalog)]).expect("engine");
    shared(&engine);
    let selection = engine
        .select(Capability::Stt, &crate::Preferences::default())
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
    let offers = engine.offers(Capability::Stt);
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

/// What each CI platform offers with the catalogue this repository ships: every bundled model of a capability, each
/// on a backend this platform compiles. Which of a model's builds wins is the ranking's (sidevoice-engine#4).
#[test]
fn the_bundled_catalogue_offers_every_model_on_this_platforms_backends() {
    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(BundledCatalog)]).expect("engine");
    let offered = |capability| {
        let mut offered: Vec<_> = engine
            .offers(capability)
            .into_iter()
            .filter_map(|offer| match offer {
                Offer::Offered { model, build, .. } => Some((model.id, build.backend)),
                Offer::Rejected { .. } => None,
            })
            .collect();
        offered.sort();
        offered
    };
    let backends: &[&str] = if cfg!(target_arch = "wasm32") {
        &["transformers-js"]
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        &["mlx", "sherpa-onnx"]
    } else {
        &["sherpa-onnx"]
    };

    let stt = offered(Capability::Stt);
    let models: Vec<_> = stt.iter().map(|(model, _)| model.as_str()).collect();
    assert_eq!(models, ["whisper-base", "whisper-small", "whisper-tiny"]);
    for (model, backend) in &stt {
        assert!(backends.contains(&backend.as_str()), "{model} on {backend}");
    }

    let tts = offered(Capability::Tts);
    if cfg!(target_arch = "wasm32") {
        assert_eq!(tts, []);
    } else {
        let kokoro = ("kokoro-82m-v1.0".to_owned(), "sherpa-onnx".to_owned());
        assert_eq!(tts, [kokoro]);
    }
}
