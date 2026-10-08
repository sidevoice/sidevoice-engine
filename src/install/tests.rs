//! The installer over a host with storage in memory: a fresh install into a build folder, what is already stored,
//! files shared by two keys or two builds, a digest that does not match, a failed download, cancelling, artifacts
//! refused before downloading, uninstalling, and archives: unpacked once in a native build, refused on the web.

use std::sync::Mutex;

use super::{file_name, member_path, Artifact, Cancel, Installed, Installer, Progress};
use crate::test_support::{artifact, block_on, member, sha256, MemoryHost};
#[cfg(native)]
use crate::test_support::{bzip2, tar, TarEntry};
use crate::Result;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

const MODEL: &[u8] = b"the model's weights";
const LIBRARY: &[u8] = b"the backend's library";
/// The build being installed, whose folder is `models/model/build`.
const BUILD: &str = "model/build";

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

/// Installs `artifacts` as [`BUILD`] on `host`, and every progress reported.
fn install(
    host: &MemoryHost,
    artifacts: &[Artifact],
    cancel: &Cancel,
) -> (Result<Installed>, Vec<Progress>) {
    install_as(BUILD, host, artifacts, cancel)
}

fn install_as(
    build: &str,
    host: &MemoryHost,
    artifacts: &[Artifact],
    cancel: &Cancel,
) -> (Result<Installed>, Vec<Progress>) {
    let reported = Mutex::new(Vec::new());
    let progress = |progress: Progress| reported.lock().unwrap().push(progress);
    let result = block_on(Installer.install(build, artifacts, host, &progress, cancel));
    (result, reported.into_inner().unwrap())
}

/// Where `path` is in [`BUILD`]'s folder, as the memory host says.
fn in_folder(path: &str) -> String {
    format!("memory:models/{BUILD}/{path}")
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
fn a_fresh_install_stores_each_blob_once_and_links_it_into_the_build_folder_under_its_name() {
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
        host.folder(BUILD),
        Some(
            [
                ("model.onnx".to_owned(), (model, None)),
                ("library".to_owned(), (library, None)),
            ]
            .into()
        )
    );
    assert_eq!(
        installed.file("model.onnx"),
        Some(in_folder("model.onnx").as_str())
    );
    assert_eq!(
        installed.file("library"),
        Some(in_folder("library").as_str())
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
fn blobs_already_stored_are_not_downloaded_again_only_linked() {
    let host = host();
    host.store(&sha256(MODEL), MODEL);
    host.store(&sha256(LIBRARY), LIBRARY);
    let (installed, reported) = install(&host, &artifacts(), &Cancel::new());

    assert_eq!(installed.expect("installed").files.len(), 2);
    assert_eq!(host.fetches(), 0);
    assert!(host.folder(BUILD).is_some());
    let done: Vec<_> = reported.iter().map(|progress| progress.done).collect();
    assert_eq!(done, [1, 2]);
}

#[test]
fn a_build_whose_folder_is_stored_is_installed_without_downloading_or_reporting() {
    let host = host();
    let (first, _) = install(&host, &artifacts(), &Cancel::new());
    let (again, reported) = install(&host, &artifacts(), &Cancel::new());

    assert_eq!(again.expect("installed"), first.expect("installed"));
    assert_eq!(host.fetches(), 2, "only the first time");
    assert!(reported.is_empty());
}

#[test]
fn a_file_two_keys_share_is_downloaded_and_placed_once() {
    let host = host();
    let artifacts = [
        artifact("model.onnx", "https://models/model.onnx", MODEL),
        artifact("copy.onnx", "https://models/model.onnx", MODEL),
    ];
    let (installed, _) = install(&host, &artifacts, &Cancel::new());

    let installed = installed.expect("installed");
    assert_eq!(installed.file("model.onnx"), installed.file("copy.onnx"));
    assert_eq!(host.fetches(), 1);
    assert_eq!(host.folder(BUILD).expect("folder").len(), 1);
}

#[test]
fn uninstalling_removes_the_folder_and_only_the_blobs_no_other_build_links() {
    let host = host();
    let other = [artifact("model.onnx", "https://models/model.onnx", MODEL)];
    install(&host, &artifacts(), &Cancel::new())
        .0
        .expect("installed");
    install_as("model/other", &host, &other, &Cancel::new())
        .0
        .expect("installed");

    block_on(Installer.uninstall(BUILD, &artifacts(), &host)).expect("uninstalled");
    assert_eq!(host.folder(BUILD), None);
    assert_eq!(
        host.stored().keys().collect::<Vec<_>>(),
        [&sha256(MODEL)],
        "the other build's file stays"
    );

    block_on(Installer.uninstall("model/other", &other, &host)).expect("uninstalled");
    assert!(host.stored().is_empty());
    assert_eq!(
        block_on(Installer.uninstall("model/other", &other, &host)),
        Ok(()),
        "nothing left to remove"
    );
}

#[test]
fn a_file_whose_bytes_do_not_match_its_digest_is_not_stored_and_no_folder_is() {
    let host = host();
    let mut artifacts = artifacts();
    artifacts[1].sha256 = sha256(b"something else");
    let (installed, _) = install(&host, &artifacts, &Cancel::new());

    assert_eq!(installed.expect_err("mismatch").code, "digest-mismatch");
    assert_eq!(host.stored().keys().collect::<Vec<_>>(), [&sha256(MODEL)]);
    assert_eq!(host.folder(BUILD), None);
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
    let installed = block_on(Installer.install(BUILD, &artifacts(), &host, &progress, &cancel));

    assert_eq!(installed.expect_err("cancelled").code, "cancelled");
    assert!(host.stored().is_empty());
    assert_eq!(host.folder(BUILD), None);
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
fn malformed_digests_conflicting_keys_and_unplaceable_files_are_refused_before_downloading() {
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

    let nameless = [artifact("model", "https://models/", MODEL)];
    let (installed, _) = install(&host, &nameless, &Cancel::new());
    assert_eq!(installed.expect_err("no name").code, "file-name-invalid");

    let same_name = [
        artifact("model", "https://models/model.onnx", MODEL),
        artifact("other", "https://other/model.onnx", LIBRARY),
    ];
    let (installed, _) = install(&host, &same_name, &Cancel::new());
    assert_eq!(installed.expect_err("one place").code, "file-path-conflict");
    assert_eq!(host.fetches(), 0);
}

#[test]
fn a_file_is_named_after_the_last_segment_of_its_url() {
    let named = |url| file_name(url);
    assert_eq!(
        named("https://hf.co/repo/resolve/abc/tiny-encoder.int8.onnx"),
        Some("tiny-encoder.int8.onnx".to_owned())
    );
    assert_eq!(
        named("https://host/model.onnx?download=1#top"),
        Some("model.onnx".to_owned())
    );
    for url in ["https://host/", "https://host/..", "no-slash"] {
        assert_eq!(named(url), None, "{url}");
    }
}

#[cfg(native)]
/// Kokoro's shape: one archive holding a model file and a data directory, each wanted under its own key.
fn kokoro() -> Vec<u8> {
    bzip2(&tar(&[
        TarEntry::Directory("kokoro/"),
        TarEntry::File("kokoro/model.onnx", MODEL),
        TarEntry::Directory("kokoro/espeak-ng-data/"),
        TarEntry::File("kokoro/espeak-ng-data/phontab", b"phonemes"),
    ]))
}

#[cfg(native)]
const KOKORO: &str = "https://models/kokoro.tar.bz2";

#[cfg(native)]
fn kokoro_artifacts(archive: &[u8]) -> Vec<Artifact> {
    vec![
        member("model", KOKORO, archive, "kokoro/model.onnx"),
        member("espeak-ng-data", KOKORO, archive, "kokoro/espeak-ng-data/"),
    ]
}

#[test]
#[cfg(native)]
fn an_archive_is_downloaded_and_unpacked_once_and_each_member_sits_in_the_folder_at_its_path() {
    let archive = kokoro();
    let host = MemoryHost::serving(&[(KOKORO, &archive)]);
    let (installed, reported) = install(&host, &kokoro_artifacts(&archive), &Cancel::new());
    let installed = installed.expect("installed");

    let tree = format!("{}-unpacked", sha256(&archive));
    assert_eq!(host.fetches(), 1);
    assert_eq!(
        installed.file("model"),
        Some(in_folder("kokoro/model.onnx").as_str())
    );
    assert_eq!(
        installed.file("espeak-ng-data"),
        Some(in_folder("kokoro/espeak-ng-data").as_str())
    );
    let folder = host.folder(BUILD).expect("folder");
    assert_eq!(
        folder["kokoro/espeak-ng-data"],
        (tree.clone(), Some("kokoro/espeak-ng-data".to_owned()))
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
    assert_eq!(host.fetches(), 1, "already installed");

    let (other, _) = install_as(
        "model/other",
        &host,
        &kokoro_artifacts(&archive),
        &Cancel::new(),
    );
    assert!(other.is_ok());
    assert_eq!(host.fetches(), 1, "already unpacked, only linked");
}

#[test]
#[cfg(native)]
fn an_archive_wanted_whole_too_is_kept() {
    let archive = kokoro();
    let host = MemoryHost::serving(&[(KOKORO, &archive)]);
    let mut artifacts = kokoro_artifacts(&archive);
    artifacts.push(artifact("tarball", KOKORO, &archive));
    let (installed, _) = install(&host, &artifacts, &Cancel::new());

    let installed = installed.expect("installed");
    assert_eq!(
        installed.file("tarball"),
        Some(in_folder("kokoro.tar.bz2").as_str())
    );
    assert_eq!(host.fetches(), 1);
}

#[test]
#[cfg(native)]
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
#[cfg(native)]
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
#[cfg(native)]
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

#[test]
#[cfg(web)]
fn archives_are_refused_on_the_web_before_downloading() {
    const ARCHIVE: &[u8] = b"an archive";
    let host = MemoryHost::serving(&[("https://models/archive.tar.bz2", ARCHIVE)]);
    let artifacts = [member(
        "model",
        "https://models/archive.tar.bz2",
        ARCHIVE,
        "kokoro/model.onnx",
    )];
    let (installed, _) = install(&host, &artifacts, &Cancel::new());
    assert_eq!(installed.expect_err("refused").code, "archive-unsupported");
    assert_eq!(host.fetches(), 0);
}

#[test]
fn a_member_path_is_relative_plain_and_normalised() {
    assert_eq!(
        member_path("./lib//libfake.so"),
        Some("lib/libfake.so".to_owned())
    );
    assert_eq!(
        member_path("espeak-ng-data/"),
        Some("espeak-ng-data".to_owned())
    );
    for path in ["", ".", "/", "/lib", "..", "a/../b", "a\\b", "c:", "a\nb"] {
        assert_eq!(member_path(path), None, "{path:?}");
    }
}
