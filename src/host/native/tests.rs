//! The native host: what it reports, its directory, and (with the network, which CI has) a real file and a backend's
//! library archive installed through it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::NativeHost;
use crate::host::{Accelerator, Host, Runs};
use crate::install::{Artifact, Cancel, Installer};
use crate::test_support::block_on;

/// A fresh directory under the system's temporary one, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sidevoice-engine-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn entries(dir: &Path) -> usize {
    std::fs::read_dir(dir).expect("a directory").count()
}

#[test]
fn it_reports_this_machine() {
    let scratch = Scratch::new();
    let caps = NativeHost::new(&scratch.0).expect("host").capabilities();

    assert_eq!(caps.runs, Runs::Native);
    assert_eq!(caps.os, std::env::consts::OS);
    assert_eq!(caps.arch, std::env::consts::ARCH);
    assert!(caps.has(Accelerator::Cpu));
    assert_eq!(caps.has(Accelerator::Metal), cfg!(target_os = "macos"));
    assert!(caps.cores.is_some_and(|cores| cores > 0));
    assert!(caps.memory_mb.is_some_and(|mb| mb > 0));
}

#[test]
fn its_directory_stores_a_file_only_once_committed() {
    let scratch = Scratch::new();
    let host = NativeHost::new(&scratch.0).expect("host");
    let storage = host.storage();
    let files = scratch.0.join("files");
    let partial = scratch.0.join("partial");

    let mut dropped = block_on(storage.create("abc")).expect("writer");
    block_on(dropped.write(b"half")).expect("written");
    assert_eq!(entries(&partial), 1);
    drop(dropped);
    assert_eq!((entries(&files), entries(&partial)), (0, 0));
    assert_eq!(block_on(storage.find("abc")), Ok(None));

    let mut kept = block_on(storage.create("abc")).expect("writer");
    block_on(kept.write(b"who")).expect("written");
    block_on(kept.write(b"le")).expect("written");
    let location = block_on(kept.commit()).expect("committed");
    assert_eq!(Path::new(&location), files.join("abc"));
    assert_eq!(std::fs::read(&location).expect("stored"), b"whole");
    assert_eq!(block_on(storage.find("abc")), Ok(Some(location)));
    assert_eq!(entries(&partial), 0);
}

#[test]
fn its_directory_refuses_names_that_are_not_plain() {
    let scratch = Scratch::new();
    let host = NativeHost::new(&scratch.0).expect("host");
    for name in ["", "../escape", "a/b", ".hidden", "a.b"] {
        let refused = block_on(host.storage().find(name));
        assert_eq!(
            refused.map_err(|error| error.code),
            Err("storage-name-invalid")
        );
    }
}

/// `preprocessor_config.json` of openai/whisper-tiny at a pinned revision: 184,990 bytes, behind a redirect.
const PINNED: &str = concat!(
    "https://huggingface.co/openai/whisper-tiny/resolve/",
    "169d4a4341b33bc18d8881c4b69c2e104e1cc0af/preprocessor_config.json"
);
const PINNED_SHA256: &str = "9b5cd03a36fbb8a627c64d98a5b5b126ead95a77720723944487311f0110b666";
const PINNED_SIZE: u64 = 184_990;

#[test]
#[ignore = "downloads from Hugging Face: CI runs it (cargo test -- --include-ignored)"]
fn it_installs_a_real_file_and_finds_it_installed_afterwards() {
    let scratch = Scratch::new();
    let host = NativeHost::new(&scratch.0).expect("host");
    let artifacts = [Artifact {
        key: "preprocessor_config.json".to_owned(),
        url: PINNED.to_owned(),
        sha256: PINNED_SHA256.to_owned(),
        archive_path: None,
    }];
    let sizes = std::sync::Mutex::new(Vec::new());
    let progress = |progress: crate::Progress| sizes.lock().unwrap().push(progress);

    let installed = block_on(Installer.install(&artifacts, &host, &progress, &Cancel::new()))
        .expect("installed");
    let location = installed.file("preprocessor_config.json").expect("by key");
    assert_eq!(
        Path::new(location),
        scratch.0.join("files").join(PINNED_SHA256)
    );
    assert_eq!(
        std::fs::metadata(location).expect("stored").len(),
        PINNED_SIZE
    );
    let sizes = sizes.into_inner().unwrap();
    assert!(sizes
        .iter()
        .any(|progress| progress.size == Some(PINNED_SIZE)));
    assert!(sizes
        .iter()
        .any(|progress| progress.received == PINNED_SIZE));

    // A new host on the same directory finds it, without downloading: the URL no longer matters.
    let again = NativeHost::new(&scratch.0).expect("host");
    let offline = [Artifact {
        url: "https://invalid.invalid/".to_owned(),
        ..artifacts[0].clone()
    }];
    let found = block_on(Installer.install(&offline, &again, &|_| {}, &Cancel::new()));
    assert_eq!(
        found.expect("found").file("preprocessor_config.json"),
        Some(location)
    );

    let wrong = [Artifact {
        sha256: "0".repeat(64),
        ..artifacts[0].clone()
    }];
    let mismatch = block_on(Installer.install(&wrong, &host, &|_| {}, &Cancel::new()));
    assert_eq!(mismatch.map_err(|error| error.code), Err("digest-mismatch"));
    assert_eq!(entries(&scratch.0.join("partial")), 0);

    let missing = [Artifact {
        url: format!("{PINNED}-missing"),
        sha256: "1".repeat(64),
        ..artifacts[0].clone()
    }];
    let failed = block_on(Installer.install(&missing, &host, &|_| {}, &Cancel::new()));
    assert_eq!(failed.map_err(|error| error.code), Err("download-failed"));
}

#[test]
fn its_directory_stores_a_tree_only_once_committed_and_finds_its_members() {
    let scratch = Scratch::new();
    let host = NativeHost::new(&scratch.0).expect("host");
    let storage = host.storage();

    let mut dropped = storage.create_tree("tree").expect("tree");
    dropped.file("lib/half.so").expect("file");
    drop(dropped);
    assert_eq!(entries(&scratch.0.join("partial")), 0);
    assert_eq!(block_on(storage.find("tree")), Ok(None));

    let mut tree = storage.create_tree("tree").expect("tree");
    tree.directory("data/empty").expect("directory");
    tree.file("lib/libfake.so").expect("file");
    tree.write(b"a lib").expect("written");
    tree.write(b"rary").expect("written");
    let location = tree.commit().expect("committed");
    assert_eq!(Path::new(&location), scratch.0.join("files").join("tree"));

    let library = block_on(storage.find_member("tree", "lib/libfake.so"))
        .expect("found")
        .expect("a member");
    assert_eq!(std::fs::read(library).expect("stored"), b"a library");
    let lib = block_on(storage.find_member("tree", "lib")).expect("found");
    assert!(lib.is_some_and(|lib| Path::new(&lib).is_dir()));
    assert!(block_on(storage.find_member("tree", "data/empty")).is_ok_and(|dir| dir.is_some()));
    assert_eq!(
        block_on(storage.find_member("tree", "lib/other.so")),
        Ok(None)
    );
    let escape = block_on(storage.find_member("tree", "../tree")).map_err(|error| error.code);
    assert_eq!(escape, Err("storage-name-invalid"));

    block_on(storage.remove("tree")).expect("removed");
    assert_eq!(block_on(storage.find("tree")), Ok(None));
    assert_eq!(
        block_on(storage.remove("tree")),
        Ok(()),
        "nothing to remove"
    );
}

#[test]
fn its_directory_reads_a_stored_file_back() {
    let scratch = Scratch::new();
    let host = NativeHost::new(&scratch.0).expect("host");
    let storage = host.storage();
    let mut file = block_on(storage.create("file")).expect("writer");
    let bytes = vec![7; 200_000];
    block_on(file.write(&bytes)).expect("written");
    block_on(file.commit()).expect("committed");

    let mut read = block_on(storage.read("file")).expect("reader");
    assert_eq!(read.size(), Some(bytes.len() as u64));
    let mut back = Vec::new();
    while let Some(part) = block_on(read.chunk()).expect("read") {
        back.extend(part);
    }
    assert_eq!(back, bytes);

    let mut opened = Vec::new();
    std::io::Read::read_to_end(&mut storage.open("file").expect("opened"), &mut opened)
        .expect("read");
    assert_eq!(opened, bytes);
}

#[test]
#[ignore = "downloads Kokoro v0.19's archive from the bundled catalogue (about 100 MB): CI runs it"]
fn it_installs_a_real_archive_and_hands_over_its_members_files_and_directories_alike() {
    use crate::catalog::{CatalogSource, ModelFile};

    let catalogue = crate::BundledCatalog.load().expect("the bundled catalogue");
    let build = catalogue
        .families
        .iter()
        .flat_map(|family| &family.models)
        .flat_map(|model| &model.builds)
        .find(|build| build.id == "kokoro-82m-v0.19/sherpa-onnx-int8")
        .expect("an archived build");
    let artifacts: Vec<_> = build.files.iter().map(ModelFile::artifact).collect();
    assert!(artifacts
        .iter()
        .all(|artifact| artifact.archive_path.is_some()));

    let scratch = Scratch::new();
    let host = NativeHost::new(&scratch.0).expect("host");
    let installed =
        block_on(Installer.install(&artifacts, &host, &|_| {}, &Cancel::new())).expect("installed");
    assert!(Path::new(installed.file("kokoro.model").expect("by key")).is_file());
    assert!(Path::new(installed.file("kokoro.tokens").expect("by key")).is_file());
    assert!(Path::new(installed.file("kokoro.data_dir").expect("by key")).is_dir());
    let archive = scratch.0.join("files").join(&artifacts[0].sha256);
    assert!(!archive.exists(), "the archive is removed once unpacked");
    assert_eq!(entries(&scratch.0.join("partial")), 0);
}
