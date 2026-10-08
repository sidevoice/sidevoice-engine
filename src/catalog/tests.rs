//! What `Catalog::check` finds in a merged catalogue, and how sources merge.

use super::{Catalog, CatalogFragment, CatalogSource, Problem};
use crate::test_support::{build, family, model, FakeCatalog};
use crate::{Capability, Family, Result};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// A source of these families.
struct Families(Vec<Family>);

impl CatalogSource for Families {
    fn load(&self) -> Result<CatalogFragment> {
        Ok(CatalogFragment {
            families: self.0.clone(),
        })
    }
}

fn problems(families: Vec<Family>) -> Vec<Problem> {
    Catalog::merge(&[Box::new(Families(families)) as Box<dyn CatalogSource>])
        .expect("catalogue")
        .check(&crate::backend::is_known)
}

#[test]
fn a_consistent_catalogue_has_no_problems() {
    let catalog = Catalog::merge(&[Box::new(FakeCatalog) as Box<dyn CatalogSource>]);
    assert_eq!(
        catalog.expect("catalogue").check(&crate::backend::is_known),
        []
    );
}

#[test]
fn sources_merge_in_order_and_a_family_twice_is_a_problem() {
    let catalog = Catalog::merge(&[
        Box::new(FakeCatalog) as Box<dyn CatalogSource>,
        Box::new(FakeCatalog),
    ])
    .expect("catalogue");
    let problems = catalog.check(&crate::backend::is_known);
    assert!(problems.contains(&Problem::DuplicateFamily {
        family: "whisper".to_owned()
    }));
    assert!(problems.contains(&Problem::DuplicateModel {
        model: "whisper-small".to_owned()
    }));
    assert!(problems.contains(&Problem::DuplicateBuild {
        build: "kokoro-onnx".to_owned()
    }));
}

#[test]
fn empty_levels_are_problems() {
    let mut no_capabilities = model("m", Capability::Stt, vec![build("m/b", "sherpa-onnx", 1)]);
    no_capabilities.capabilities.clear();
    let mut no_files = build("n/b", "sherpa-onnx", 1);
    no_files.files.clear();
    let found = problems(vec![
        family("empty", Vec::new()),
        family(
            "f",
            vec![
                no_capabilities,
                model("no-builds", Capability::Stt, Vec::new()),
                model("n", Capability::Stt, vec![no_files]),
            ],
        ),
    ]);
    assert_eq!(
        found,
        [
            Problem::FamilyWithoutModels {
                family: "empty".to_owned()
            },
            Problem::ModelWithoutCapabilities {
                model: "m".to_owned()
            },
            Problem::ModelWithoutBuilds {
                model: "no-builds".to_owned()
            },
            Problem::BuildWithoutFiles {
                build: "n/b".to_owned()
            },
        ]
    );
}

#[test]
fn a_build_needs_a_known_backend_and_files_with_digests_and_distinct_keys() {
    // whisper-cpp is in backends.json, compiled here or not: it is known.
    let known = build("m/whisper-cpp", "whisper-cpp", 1);
    let unknown = build("m/nope", "no-such-backend", 1);
    let mut files = build("m/files", "sherpa-onnx", 1);
    let file = files.files[0].clone();
    files.files.push(crate::ModelFile {
        url: "https://example.com/again".to_owned(),
        ..file.clone()
    });
    files.files[0].sha256 = "A".repeat(64);
    files.files.push(crate::ModelFile {
        key: "short".to_owned(),
        url: "https://example.com/short".to_owned(),
        sha256: "0".repeat(63),
        ..file
    });
    let found = problems(vec![family(
        "f",
        vec![model("m", Capability::Stt, vec![known, unknown, files])],
    )]);
    assert_eq!(
        found,
        [
            Problem::UnknownBackend {
                build: "m/nope".to_owned(),
                backend: "no-such-backend".to_owned()
            },
            Problem::FileWithoutDigest {
                build: "m/files".to_owned(),
                key: "model".to_owned()
            },
            Problem::DuplicateFile {
                build: "m/files".to_owned(),
                key: "model".to_owned()
            },
            Problem::FileWithoutDigest {
                build: "m/files".to_owned(),
                key: "short".to_owned()
            },
        ]
    );
}

#[test]
fn models_are_found_by_any_of_their_capabilities() {
    let mut both = model(
        "both",
        Capability::Stt,
        vec![build("both/b", "sherpa-onnx", 1)],
    );
    both.capabilities.push(Capability::Tts);
    let catalog = Catalog::merge(&[
        Box::new(Families(vec![family("f", vec![both])])) as Box<dyn CatalogSource>
    ])
    .expect("catalogue");
    for capability in [Capability::Stt, Capability::Tts] {
        let ids: Vec<_> = catalog
            .models(capability)
            .map(|model| model.id.as_str())
            .collect();
        assert_eq!(ids, ["both"]);
    }
}

#[test]
fn keys_inside_one_archive_repeat_what_it_is() {
    let mut archive = build("m/archive", "sherpa-onnx", 1);
    let model_file = crate::ModelFile {
        archive_path: Some("m/model.onnx".to_owned()),
        mutable: true,
        ..archive.files[0].clone()
    };
    let voices = crate::ModelFile {
        key: "voices".to_owned(),
        archive_path: Some("m/voices.bin".to_owned()),
        ..model_file.clone()
    };
    let other_size = crate::ModelFile {
        key: "tokens".to_owned(),
        bytes: 2,
        ..voices.clone()
    };
    let not_mutable = crate::ModelFile {
        key: "data".to_owned(),
        mutable: false,
        ..voices.clone()
    };
    archive.files = vec![model_file, voices, other_size, not_mutable];
    let found = problems(vec![family(
        "f",
        vec![model("m", Capability::Stt, vec![archive])],
    )]);
    let inconsistent = |key: &str| Problem::InconsistentDownload {
        build: "m/archive".to_owned(),
        key: key.to_owned(),
    };
    assert_eq!(found, [inconsistent("tokens"), inconsistent("data")]);
}

#[test]
fn a_build_maps_only_arguments_a_call_has_each_to_some_path() {
    let mut fine = build("m/fine", "sherpa-onnx", 1);
    fine.call_params
        .insert("language".to_owned(), vec!["whisper.language".to_owned()]);
    let mut unknown = build("m/unknown", "sherpa-onnx", 1);
    unknown
        .call_params
        .insert("lang".to_owned(), vec!["whisper.language".to_owned()]);
    let mut nowhere = build("m/nowhere", "sherpa-onnx", 1);
    nowhere
        .call_params
        .insert("language".to_owned(), Vec::new());
    let found = problems(vec![family(
        "f",
        vec![model("m", Capability::Stt, vec![fine, unknown, nowhere])],
    )]);
    assert_eq!(
        found,
        [
            Problem::UnknownCallArgument {
                build: "m/unknown".to_owned(),
                argument: "lang".to_owned()
            },
            Problem::UnknownCallArgument {
                build: "m/nowhere".to_owned(),
                argument: "language".to_owned()
            },
        ]
    );
}

#[test]
fn call_params_take_one_path_or_a_list() {
    let build: crate::BuildEntry = serde_json::from_str(
        r#"{"id": "m/b", "backend": "sherpa-onnx", "precision": "int8",
            "memory": {"mb": 1, "source": "estimated", "basis": "a test"}, "files": [],
            "call_params": {"language": ["canary.src_lang", "canary.tgt_lang"], "other": "x.y"}}"#,
    )
    .expect("a build");
    assert_eq!(
        build.call_params["language"],
        ["canary.src_lang", "canary.tgt_lang"]
    );
    assert_eq!(build.call_params["other"], ["x.y"]);
}
