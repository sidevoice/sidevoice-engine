//! The engine's catalogues over a host whose provider side is fake: the local catalogue and each provider listed
//! alike, a provider's status before and after its key, a refresh, and a remote model loaded from its catalogue that
//! transcribes through the same interface as a local one.

use crate::test_support::{block_on, openai_spec, FakeCatalog, MemoryHost};
use crate::{Cancel, Capability, CatalogModel, Engine, LoadedModel, LOCAL_CATALOG};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

const OPENAI_SPEC: &str =
    "https://raw.githubusercontent.com/openai/openai-openapi/main/openapi.json";

#[test]
fn the_local_catalogue_comes_first_then_each_provider_and_any_other_id_is_not_found() {
    let engine =
        Engine::new(Box::new(MemoryHost::default()), vec![Box::new(FakeCatalog)]).expect("engine");
    let ids: Vec<_> = engine
        .catalogs()
        .iter()
        .map(|catalog| catalog.id())
        .collect();
    assert_eq!(ids, [LOCAL_CATALOG, "elevenlabs", "openai"]);
    let names: Vec<_> = engine
        .catalogs()
        .iter()
        .map(|catalog| catalog.name())
        .collect();
    assert_eq!(names, [None, Some("ElevenLabs"), Some("OpenAI")]);
    let missing = engine.catalog("nobody").map(drop).map_err(|e| e.code);
    assert_eq!(missing, Err("catalog-not-found"));

    let local = engine.catalog(LOCAL_CATALOG).expect("local");
    let status = block_on(local.status());
    assert_eq!((status.reason, status.stale), (None, false));
    let all = block_on(local.models(None)).expect("models");
    let speakers = block_on(local.models(Some(Capability::Tts))).expect("models");
    assert!(all
        .iter()
        .all(|model| matches!(model, CatalogModel::Local(_))));
    assert!(!speakers.is_empty() && speakers.len() < all.len());
    assert!(speakers
        .iter()
        .all(|model| model.capabilities().contains(&Capability::Tts)));
}

#[test]
fn a_provider_is_a_catalogue_whose_model_transcribes_like_a_local_one() {
    let host = MemoryHost::default();
    let remote = host.remote();
    let engine = Engine::new(Box::new(host), Vec::new()).expect("engine");
    let openai = engine.catalog("openai").expect("openai");

    let status = block_on(openai.status());
    assert_eq!(
        status.reason.map(|reason| reason.code),
        Some("credential-missing")
    );
    assert!(block_on(openai.models(None)).expect("models").is_empty());
    assert!(remote.requests().is_empty(), "no key, no call");
    let missing = block_on(openai.load("gpt-4o-transcribe", &|_| {}, &Cancel::new())).map(drop);
    assert_eq!(missing.map_err(|e| e.code), Err("credential-missing"));

    remote.key("openai", "sk-test");
    remote.answer(OPENAI_SPEC, 200, openai_spec().to_string().as_bytes());
    remote.answer(
        "https://api.openai.com/v1/models",
        200,
        br#"{"data": [{"id": "gpt-4o-transcribe"}]}"#,
    );
    remote.answer(
        "https://api.openai.com/v1/audio/transcriptions",
        200,
        br#"{"text": "hi"}"#,
    );
    let status = block_on(openai.refresh());
    assert_eq!(
        (status.reason, status.stale, status.detail),
        (None, false, None)
    );
    let models = block_on(openai.models(Some(Capability::Stt))).expect("models");
    let ids: Vec<_> = models.iter().map(CatalogModel::id).collect();
    assert_eq!(ids, ["gpt-4o-transcribe"]);
    assert!(block_on(openai.models(Some(Capability::Tts)))
        .expect("models")
        .is_empty());

    let model =
        block_on(openai.load("gpt-4o-transcribe", &|_| {}, &Cancel::new())).expect("loaded");
    let LoadedModel::Remote(ref remote_model) = model else {
        panic!("a remote model");
    };
    assert_eq!(remote_model.provider(), "openai");
    assert!(model.as_tts().is_none() && model.as_vad().is_none());
    let stt = model.as_stt().expect("speech to text");
    assert_eq!(
        block_on(stt.transcribe(&[0.0; 800], 8_000, Some("en"))).as_deref(),
        Ok("hi")
    );
    let spec_reads = remote
        .requests()
        .iter()
        .filter(|request| request.url == OPENAI_SPEC)
        .count();
    assert_eq!(spec_reads, 1, "the spec read once, with the listing");

    let absent = block_on(openai.load("whisper-1", &|_| {}, &Cancel::new())).map(drop);
    assert_eq!(absent.map_err(|e| e.code), Err("model-not-found"));
}

#[test]
fn a_provider_whose_spec_cannot_be_read_has_no_models_and_says_why() {
    let host = MemoryHost::default();
    let remote = host.remote();
    remote.key("openai", "sk-test");
    remote.answer(OPENAI_SPEC, 503, b"");
    let engine = Engine::new(Box::new(host), Vec::new()).expect("engine");
    let openai = engine.catalog("openai").expect("openai");
    let status = block_on(openai.status());
    assert_eq!(
        status.reason.map(|reason| reason.code),
        Some("provider-spec-unreadable")
    );
    assert!(block_on(openai.models(None)).expect("models").is_empty());
}
