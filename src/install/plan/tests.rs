//! The plan, without downloading anything: what is refused, and what is wanted once.

use super::{check, unpacked, wanted, Wanted};
use crate::install::Artifact;
use crate::test_support::{artifact, member, sha256};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

const MODEL: &[u8] = b"the model's weights";
const ARCHIVE: &[u8] = b"an archive";
const KOKORO: &str = "https://models/kokoro.tar.bz2";

fn code(artifacts: &[Artifact]) -> &'static str {
    check(artifacts).expect_err("refused").code
}

#[test]
fn malformed_digests_are_refused() {
    for digest in [
        "",
        "ABC",
        &"A".repeat(64),
        &format!("../{}", "a".repeat(61)),
    ] {
        let mut bad = artifact("model.onnx", "https://models/model.onnx", MODEL);
        bad.sha256 = digest.to_owned();
        assert_eq!(code(&[bad]), "digest-invalid", "{digest}");
    }
}

#[test]
fn a_key_naming_two_files_or_two_members_is_refused() {
    let conflicting = [
        artifact("model.onnx", "https://models/model.onnx", MODEL),
        artifact("model.onnx", "https://models/other", b"other"),
    ];
    assert_eq!(code(&conflicting), "artifact-key-conflict");
    let two = [
        member("model", KOKORO, ARCHIVE, "kokoro/model.onnx"),
        member("model", KOKORO, ARCHIVE, "kokoro/espeak-ng-data"),
    ];
    assert_eq!(code(&two), "artifact-key-conflict");
}

#[test]
fn archive_paths_must_stay_inside_and_are_normalised() {
    for path in ["../outside", "/kokoro/model.onnx", ""] {
        assert_eq!(
            code(&[member("model", KOKORO, ARCHIVE, path)]),
            "archive-path-invalid",
            "{path}"
        );
    }
    let members = check(&[
        member("model", KOKORO, ARCHIVE, "./kokoro//model.onnx"),
        artifact("config", "https://models/config", MODEL),
    ])
    .expect("checked");
    assert_eq!(members, [Some("kokoro/model.onnx".to_owned()), None]);
}

#[test]
fn each_digest_is_wanted_once_whole_unpacked_or_both() {
    let artifacts = [
        member("model", KOKORO, ARCHIVE, "kokoro/model.onnx"),
        member("voices", KOKORO, ARCHIVE, "kokoro/voices.bin"),
        artifact("archive", KOKORO, ARCHIVE),
        artifact("config", "https://models/config", MODEL),
    ];
    let digest = sha256(ARCHIVE);
    let config = sha256(MODEL);
    assert_eq!(
        wanted(&artifacts),
        [
            Wanted {
                sha256: &digest,
                url: KOKORO,
                whole: true,
                unpacked: true,
            },
            Wanted {
                sha256: &config,
                url: "https://models/config",
                whole: true,
                unpacked: false,
            },
        ]
    );
    assert_eq!(unpacked("abc"), "abc-unpacked");
}
