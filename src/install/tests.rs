//! The installer over a host with storage in memory: a fresh install, what is already stored, files shared by two
//! keys, a digest that does not match, a failed download, cancelling, and artifacts refused before downloading.

use std::sync::Mutex;

use super::{Artifact, Cancel, Installed, Installer, Progress};
use crate::test_support::{artifact, block_on, bzip2, member, sha256, tar, MemoryHost, TarEntry};
use crate::Result;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

const MODEL: &[u8] = b"the model's weights";
const LIBRARY: &[u8] = b"the backend's library";

fn host() -> MemoryHost {
    MemoryHost::serving(&[
        ("https://models/model.onnx", MODEL),
        ("https://backends/library", LIBRARY),
    ])
}

fn artifacts() -> Vec<Artifact> {
    vec![
        artifact("model.onnx", "https://models/model.onnx", MODEL),
        artifact("library", "https://backends/library", LIBRARY),
    ]
}

/// Installs `artifacts` on `host`, and every progress reported.
fn install(
    host: &MemoryHost,
    artifacts: &[Artifact],
    cancel: &Cancel,
) -> (Result<Installed>, Vec<Progress>) {
    let reported = Mutex::new(Vec::new());
    let progress = |progress: Progress| reported.lock().unwrap().push(progress);
    let result = block_on(Installer.install(artifacts, host, &progress, cancel));
    (result, reported.into_inner().unwrap())
}

#[test]
fn installed_files_are_found_by_name() {
    let installed = Installed {
        files: [("library".to_owned(), "/data/backends/library".to_owned())].into(),
    };
    assert_eq!(installed.file("library"), Some("/data/backends/library"));
    assert_eq!(installed.file("model.onnx"), None);
}

#[test]
fn a_fresh_install_stores_each_file_under_its_digest_and_finds_it_by_key() {
    let host = host();
    let (installed, reported) = install(&host, &artifacts(), &Cancel::new());
    let installed = installed.expect("installed");

    let model = sha256(MODEL);
    let library = sha256(LIBRARY);
    assert_eq!(
        host.stored(),
        [
            (model.clone(), MODEL.to_vec()),
            (library.clone(), LIBRARY.to_vec())
        ]
        .into()
    );
    assert_eq!(
        installed.file("model.onnx"),
        Some(format!("memory:{model}").as_str())
    );
    assert_eq!(
        installed.file("library"),
        Some(format!("memory:{library}").as_str())
    );
    assert_eq!(host.fetches(), 2);

    let last = reported.last().expect("progress");
    assert_eq!((last.done, last.files), (2, 2));
    let model_size = Some(MODEL.len() as u64);
    assert!(reported.contains(&Progress {
        files: 2,
        done: 0,
        received: MODEL.len() as u64,
        size: model_size,
    }));
    assert!(reported.windows(2).all(|pair| pair[0].done <= pair[1].done));
}

#[test]
fn what_is_already_stored_is_not_downloaded_again() {
    let host = host();
    host.store(&sha256(MODEL), MODEL);
    host.store(&sha256(LIBRARY), LIBRARY);
    let (installed, reported) = install(&host, &artifacts(), &Cancel::new());

    assert_eq!(installed.expect("installed").files.len(), 2);
    assert_eq!(host.fetches(), 0);
    let done: Vec<_> = reported.iter().map(|progress| progress.done).collect();
    assert_eq!(done, [1, 2]);
}

#[test]
fn a_file_two_keys_share_is_downloaded_once() {
    let host = host();
    let artifacts = [
        artifact("model.onnx", "https://models/model.onnx", MODEL),
        artifact("copy.onnx", "https://models/model.onnx", MODEL),
    ];
    let (installed, _) = install(&host, &artifacts, &Cancel::new());

    let installed = installed.expect("installed");
    assert_eq!(installed.file("model.onnx"), installed.file("copy.onnx"));
    assert_eq!(host.fetches(), 1);
}

#[test]
fn a_file_whose_bytes_do_not_match_its_digest_is_not_stored() {
    let host = host();
    let mut artifacts = artifacts();
    artifacts[1].sha256 = sha256(b"something else");
    let (installed, _) = install(&host, &artifacts, &Cancel::new());

    assert_eq!(installed.expect_err("mismatch").code, "digest-mismatch");
    assert_eq!(host.stored().keys().collect::<Vec<_>>(), [&sha256(MODEL)]);
}

#[test]
fn a_failed_download_fails_with_the_fetchers_code() {
    let host = MemoryHost::default();
    let (installed, _) = install(&host, &artifacts(), &Cancel::new());

    assert_eq!(installed.expect_err("not served").code, "download-failed");
    assert!(host.stored().is_empty());
}

#[test]
fn cancelling_stops_the_install_in_the_middle_of_a_file_and_stores_nothing_of_it() {
    let host = host();
    let cancel = Cancel::new();
    let progress = |progress: Progress| {
        if progress.received > 0 {
            cancel.cancel();
        }
    };
    let installed = block_on(Installer.install(&artifacts(), &host, &progress, &cancel));

    assert_eq!(installed.expect_err("cancelled").code, "cancelled");
    assert!(host.stored().is_empty());
    assert_eq!(host.fetches(), 1);
}

#[test]
fn a_cancelled_install_downloads_nothing() {
    let host = host();
    let cancel = Cancel::new();
    cancel.cancel();
    let (installed, _) = install(&host, &artifacts(), &cancel);

    assert_eq!(installed.expect_err("cancelled").code, "cancelled");
    assert_eq!(host.fetches(), 0);
}

#[test]
fn malformed_digests_and_conflicting_keys_are_refused_before_downloading() {
    let host = host();
    for sha256 in [
        "",
        "ABC",
        &"A".repeat(64),
        &format!("../{}", "a".repeat(61)),
    ] {
        let mut artifacts = artifacts();
        artifacts[1].sha256 = sha256.to_owned();
        let (installed, _) = install(&host, &artifacts, &Cancel::new());
        assert_eq!(installed.expect_err(sha256).code, "digest-invalid");
    }

    let conflicting = [
        artifact("model.onnx", "https://models/model.onnx", MODEL),
        artifact("model.onnx", "https://backends/library", LIBRARY),
    ];
    let (installed, _) = install(&host, &conflicting, &Cancel::new());
    assert_eq!(
        installed.expect_err("conflict").code,
        "artifact-key-conflict"
    );
    assert_eq!(host.fetches(), 0);
}

/// Kokoro's shape: one archive holding a model file and a data directory, each wanted under its own key.
fn kokoro() -> Vec<u8> {
    bzip2(&tar(&[
        TarEntry::Directory("kokoro/"),
        TarEntry::File("kokoro/model.onnx", MODEL),
        TarEntry::Directory("kokoro/espeak-ng-data/"),
        TarEntry::File("kokoro/espeak-ng-data/phontab", b"phonemes"),
    ]))
}

const KOKORO: &str = "https://models/kokoro.tar.bz2";

fn kokoro_artifacts(archive: &[u8]) -> Vec<Artifact> {
    vec![
        member("model", KOKORO, archive, "kokoro/model.onnx"),
        member("espeak-ng-data", KOKORO, archive, "kokoro/espeak-ng-data/"),
    ]
}

#[test]
fn an_archive_is_downloaded_and_unpacked_once_and_each_key_finds_its_member() {
    let archive = kokoro();
    let host = MemoryHost::serving(&[(KOKORO, &archive)]);
    let (installed, reported) = install(&host, &kokoro_artifacts(&archive), &Cancel::new());
    let installed = installed.expect("installed");

    let tree = format!("{}-unpacked", sha256(&archive));
    assert_eq!(host.fetches(), 1);
    assert_eq!(
        installed.file("model"),
        Some(format!("memory:{tree}/kokoro/model.onnx").as_str())
    );
    assert_eq!(
        installed.file("espeak-ng-data"),
        Some(format!("memory:{tree}/kokoro/espeak-ng-data").as_str())
    );
    let unpacked = host.tree(&tree).expect("unpacked");
    assert_eq!(unpacked["kokoro/model.onnx"].as_deref(), Some(MODEL));
    assert!(
        host.stored().is_empty(),
        "the archive is removed once unpacked"
    );
    let last = reported.last().expect("progress");
    assert_eq!((last.done, last.files), (1, 1), "one archive");

    let (again, _) = install(&host, &kokoro_artifacts(&archive), &Cancel::new());
    assert_eq!(again.expect("installed"), installed);
    assert_eq!(host.fetches(), 1, "already unpacked");
}

#[test]
fn an_archive_wanted_whole_too_is_kept() {
    let archive = kokoro();
    let host = MemoryHost::serving(&[(KOKORO, &archive)]);
    let mut artifacts = kokoro_artifacts(&archive);
    artifacts.push(artifact("tarball", KOKORO, &archive));
    let (installed, _) = install(&host, &artifacts, &Cancel::new());

    let installed = installed.expect("installed");
    assert_eq!(
        installed.file("tarball"),
        Some(format!("memory:{}", sha256(&archive)).as_str())
    );
    assert_eq!(host.fetches(), 1);
}

#[test]
fn a_member_the_archive_does_not_hold_fails_the_install() {
    let archive = kokoro();
    let host = MemoryHost::serving(&[(KOKORO, &archive)]);
    let artifacts = [member("voices", KOKORO, &archive, "kokoro/voices.bin")];
    let (installed, _) = install(&host, &artifacts, &Cancel::new());

    assert_eq!(
        installed.expect_err("missing").code,
        "archive-member-missing"
    );
}

#[test]
fn an_archive_that_does_not_match_its_digest_is_never_unpacked() {
    let archive = kokoro();
    let host = MemoryHost::serving(&[(KOKORO, &bzip2(b"something else"))]);
    let (installed, _) = install(&host, &kokoro_artifacts(&archive), &Cancel::new());

    assert_eq!(installed.expect_err("mismatch").code, "digest-mismatch");
    assert!(host.stored().is_empty());
    assert!(host
        .tree(&format!("{}-unpacked", sha256(&archive)))
        .is_none());
}

#[test]
fn bad_archive_paths_and_keys_naming_two_members_are_refused_before_downloading() {
    let archive = kokoro();
    let host = MemoryHost::serving(&[(KOKORO, &archive)]);
    for path in ["../outside", "/kokoro/model.onnx", ""] {
        let artifacts = [member("model", KOKORO, &archive, path)];
        let (installed, _) = install(&host, &artifacts, &Cancel::new());
        assert_eq!(installed.expect_err(path).code, "archive-path-invalid");
    }

    let two = [
        member("model", KOKORO, &archive, "kokoro/model.onnx"),
        member("model", KOKORO, &archive, "kokoro/espeak-ng-data"),
    ];
    let (installed, _) = install(&host, &two, &Cancel::new());
    assert_eq!(
        installed.expect_err("conflict").code,
        "artifact-key-conflict"
    );
    assert_eq!(host.fetches(), 0);
}
