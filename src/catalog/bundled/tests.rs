use super::{parse, BundledCatalog, FAMILIES};
use crate::catalog::{Catalog, CatalogSource};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn every_bundled_family_parses_and_the_merge_has_no_problems() {
    for (name, json) in FAMILIES {
        if let Err(error) = parse(name, json) {
            panic!("{error}");
        }
    }
    let catalog = Catalog::merge(&[Box::new(BundledCatalog) as Box<dyn CatalogSource>]);
    assert_eq!(catalog.expect("bundled catalogue").check(), []);
}

#[test]
fn every_bundled_file_is_pinned_to_a_revision_or_marked_mutable() {
    let fragment = BundledCatalog.load().expect("bundled catalogue");
    let builds = fragment
        .families
        .iter()
        .flat_map(|family| &family.models)
        .flat_map(|model| &model.builds);
    for build in builds {
        for file in &build.files {
            let what = format!("{} {}", build.id, file.key);
            assert!(file.bytes > 0, "{what}: no size");
            // A release asset cannot be pinned by its URL: its digest is all that pins it, and the data says so.
            if file.url.starts_with("https://github.com/")
                && file.url.contains("/releases/download/")
            {
                assert!(file.mutable, "{what}: a release asset not marked mutable");
                continue;
            }
            assert!(
                !file.mutable,
                "{what}: marked mutable, but pinned to a revision"
            );
            let revision = file
                .url
                .strip_prefix("https://huggingface.co/")
                .and_then(|rest| rest.split("/resolve/").nth(1))
                .and_then(|rest| rest.split('/').next());
            let commit = |revision: &str| {
                revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit())
            };
            assert!(
                revision.is_some_and(commit),
                "{what}: {} is not pinned; run `cargo xtask pin-catalog`",
                file.url
            );
        }
    }
}

/// The directory and the list agree: a family file that is not compiled in fails here. Native only: the test reads
/// the repository.
#[cfg(native)]
#[test]
fn every_file_in_catalog_families_is_bundled() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("catalog/families");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("catalog/families")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("UTF-8")
        })
        .collect();
    files.sort();
    let bundled: Vec<_> = FAMILIES
        .iter()
        .map(|(name, _)| format!("{name}.json"))
        .collect();
    assert_eq!(files, bundled);
}

/// A family with one model and one build, each with every key, as JSON with `{edit}` applied to it.
fn family_with(edit: impl FnOnce(&mut serde_json::Value)) -> Result<(), String> {
    let mut family = serde_json::json!({
        "id": "f",
        "architecture": "f",
        "source": "https://example.com",
        "models": [{
            "id": "m",
            "capabilities": ["stt", "tts"],
            "parameters_m": 1,
            "languages": ["en"],
            "license": "MIT",
            "builds": [{
                "id": "m/b",
                "backend": "sherpa-onnx",
                "precision": "q5_1",
                "requires": {"accelerators": ["cpu"], "webgpu_features": ["shader-f16"], "wasm_max_mb": 2048},
                "memory": {"mb": 1, "source": "measured", "basis": "a test"},
                "files": [{
                    "key": "model",
                    "url": "https://example.com/m.tar.bz2",
                    "sha256": "",
                    "bytes": 1,
                    "archive_path": "m/model.onnx",
                    "mutable": true
                }]
            }]
        }]
    });
    edit(&mut family);
    parse("f", &family.to_string()).map(drop)
}

#[test]
fn a_family_file_is_read_strictly() {
    assert_eq!(family_with(|_| ()), Ok(()));
    // A build's `requires` and a file's `archive_path` and `mutable` are the only optional keys, and the constraints
    // in `requires` are each optional.
    assert_eq!(
        family_with(|family| {
            let build = family["models"][0]["builds"][0].as_object_mut();
            build.expect("a build").remove("requires");
            let file = family["models"][0]["builds"][0]["files"][0].as_object_mut();
            let file = file.expect("a file");
            file.remove("archive_path");
            file.remove("mutable");
        }),
        Ok(())
    );
    assert_eq!(
        family_with(|family| family["models"][0]["builds"][0]["requires"] = serde_json::json!({})),
        Ok(())
    );

    let fails = |what: &str, edit: &dyn Fn(&mut serde_json::Value)| {
        let error = family_with(edit).expect_err(what);
        assert!(error.starts_with("f.json: "), "{what}: {error}");
    };
    fails("an unknown family key", &|family| {
        family["name"] = "Whisper".into()
    });
    fails("an unknown model key", &|family| {
        family["models"][0]["task"] = "stt".into()
    });
    fails("an unknown build key", &|family| {
        family["models"][0]["builds"][0]["accelerators"] = serde_json::json!(["cpu"]);
    });
    fails("an unknown file key", &|family| {
        family["models"][0]["builds"][0]["files"][0]["name"] = "m".into();
    });
    fails("an unknown constraint", &|family| {
        family["models"][0]["builds"][0]["requires"]["os"] = serde_json::json!(["linux"]);
    });
    fails("an unknown accelerator", &|family| {
        family["models"][0]["builds"][0]["requires"]["accelerators"] = serde_json::json!(["npu"]);
    });
    fails("a missing family key", &|family| {
        family
            .as_object_mut()
            .expect("a family")
            .remove("architecture");
    });
    fails("a missing model key", &|family| {
        family["models"][0]
            .as_object_mut()
            .expect("a model")
            .remove("license");
    });
    fails("a missing build key", &|family| {
        family["models"][0]["builds"][0]
            .as_object_mut()
            .expect("a build")
            .remove("memory");
    });
    fails("a missing file key", &|family| {
        let file = &mut family["models"][0]["builds"][0]["files"][0];
        file.as_object_mut().expect("a file").remove("bytes");
    });
    fails("an unknown capability", &|family| {
        family["models"][0]["capabilities"] = serde_json::json!(["llm"]);
    });
    fails("a precision that is not a name", &|family| {
        family["models"][0]["builds"][0]["precision"] = serde_json::json!({"name": "q5_1"});
    });
    fails("an unknown memory source", &|family| {
        family["models"][0]["builds"][0]["memory"]["source"] = "guessed".into();
    });
    fails("an id that is not the file's name", &|family| {
        family["id"] = "g".into()
    });
}
