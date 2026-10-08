//! The native host: what it reports, its directory (blobs, and build folders of hard links to them), and (with the
//! network, which CI has) a real file and a real archive installed and uninstalled through it.

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
    let blobs = scratch.0.join("blobs");
    let partial = scratch.0.join("partial");

    let mut dropped = block_on(storage.create("abc")).expect("writer");
    block_on(dropped.write(b"half")).expect("written");
    assert_eq!(entries(&partial), 1);
    drop(dropped);
    assert_eq!((entries(&blobs), entries(&partial)), (0, 0));
    assert_eq!(block_on(storage.find("abc")), Ok(None));

    let mut kept = block_on(storage.create("abc")).expect("writer");
    block_on(kept.write(b"who")).expect("written");
    block_on(kept.write(b"le")).expect("written");
    let location = block_on(kept.commit()).expect("committed");
    assert_eq!(Path::new(&location), blobs.join("abc"));
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
fn it_installs_a_real_file_in_its_build_folder_and_uninstalls_it() {
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

    let installed = block_on(Installer.install(
        "whisper-tiny/hf",
        &artifacts,
        &host,
        &progress,
        &Cancel::new(),
    ))
    .expect("installed");
    let location = installed.file("preprocessor_config.json").expect("by key");
    // Under its original name in the build's folder, a hard link to its blob.
    let in_folder = scratch
        .0
        .join("models/whisper-tiny/hf/preprocessor_config.json");
    assert_eq!(Path::new(location), in_folder);
    assert_eq!(
        std::fs::metadata(location).expect("stored").len(),
        PINNED_SIZE
    );
    let blob = scratch.0.join("blobs").join(PINNED_SHA256);
    assert!(blob.is_file());
    #[cfg(unix)]
    assert_eq!(
        std::os::unix::fs::MetadataExt::nlink(&std::fs::metadata(&blob).expect("blob")),
        2
    );
    let sizes = sizes.into_inner().unwrap();
    assert!(sizes
        .iter()
        .any(|progress| progress.size == Some(PINNED_SIZE)));
    assert!(sizes
        .iter()
        .any(|progress| progress.received == PINNED_SIZE));

    // A new host on the same directory finds the build installed, without downloading: the URL no longer matters.
    let again = NativeHost::new(&scratch.0).expect("host");
    let offline = [Artifact {
        url: "https://invalid.invalid/preprocessor_config.json".to_owned(),
        ..artifacts[0].clone()
    }];
    let found =
        block_on(Installer.install("whisper-tiny/hf", &offline, &again, &|_| {}, &Cancel::new()));
    assert_eq!(
        found.expect("found").file("preprocessor_config.json"),
        Some(location)
    );

    let wrong = [Artifact {
        sha256: "0".repeat(64),
        ..artifacts[0].clone()
    }];
    let mismatch =
        block_on(Installer.install("other/wrong", &wrong, &host, &|_| {}, &Cancel::new()));
    assert_eq!(mismatch.map_err(|error| error.code), Err("digest-mismatch"));
    assert_eq!(entries(&scratch.0.join("partial")), 0);
    assert!(!scratch.0.join("models/other").exists(), "no folder");

    let missing = [Artifact {
        url: format!("{PINNED}-missing"),
        sha256: "1".repeat(64),
        ..artifacts[0].clone()
    }];
    let failed =
        block_on(Installer.install("other/missing", &missing, &host, &|_| {}, &Cancel::new()));
    assert_eq!(failed.map_err(|error| error.code), Err("download-failed"));

    block_on(Installer.uninstall("whisper-tiny/hf", &artifacts, host.storage()))
        .expect("uninstalled");
    assert!(
        !scratch.0.join("models/whisper-tiny").exists(),
        "the folder, and the model's"
    );
    assert!(!blob.exists(), "the blob no folder links");
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
    assert_eq!(Path::new(&location), scratch.0.join("blobs").join("tree"));

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
        block_on(Installer.install(&build.id, &artifacts, &host, &|_| {}, &Cancel::new()))
            .expect("installed");
    // Each member at its path inside the archive, in the build's folder.
    let folder = scratch
        .0
        .join("models")
        .join(&build.id)
        .join("kokoro-int8-en-v0_19");
    let model = installed.file("kokoro.model").expect("by key");
    assert_eq!(Path::new(model), folder.join("model.int8.onnx"));
    assert!(Path::new(model).is_file());
    assert!(Path::new(installed.file("kokoro.tokens").expect("by key")).is_file());
    let data = installed.file("kokoro.data_dir").expect("by key");
    assert_eq!(Path::new(data), folder.join("espeak-ng-data"));
    assert!(
        Path::new(data).join("phontab").is_file(),
        "the directory's files with it"
    );
    let blobs = scratch.0.join("blobs");
    assert!(
        !blobs.join(&artifacts[0].sha256).exists(),
        "the archive is removed once unpacked"
    );
    assert!(blobs
        .join(format!("{}-unpacked", artifacts[0].sha256))
        .is_dir());
    assert_eq!(entries(&scratch.0.join("partial")), 0);

    block_on(Installer.uninstall(&build.id, &artifacts, host.storage())).expect("uninstalled");
    assert_eq!(entries(&scratch.0.join("models")), 0);
    assert_eq!(entries(&blobs), 0, "the unpacked archive no folder links");
}

#[test]
fn its_directory_stores_a_build_folder_of_links_only_once_committed() {
    let scratch = Scratch::new();
    let host = NativeHost::new(&scratch.0).expect("host");
    let storage = host.storage();
    let mut file = block_on(storage.create("blob")).expect("writer");
    block_on(file.write(b"weights")).expect("written");
    block_on(file.commit()).expect("committed");
    let mut tree = storage.create_tree("tree").expect("tree");
    tree.file("data/a/phontab").expect("file");
    tree.write(b"phonemes").expect("written");
    tree.commit().expect("committed");

    let mut dropped = block_on(storage.create_folder("model/build")).expect("folder");
    block_on(dropped.link("model.onnx", "blob", None)).expect("linked");
    drop(dropped);
    assert_eq!(entries(&scratch.0.join("partial")), 0);
    assert_eq!(block_on(storage.find_folder("model/build")), Ok(None));
    assert_eq!(block_on(storage.is_linked("blob")), Ok(false));

    for build in ["model/build", "model/other"] {
        let mut folder = block_on(storage.create_folder(build)).expect("folder");
        block_on(folder.link("model.onnx", "blob", None)).expect("a file");
        block_on(folder.link("data", "tree", Some("data"))).expect("a directory");
        let location = block_on(folder.commit()).expect("committed");
        assert_eq!(Path::new(&location), scratch.0.join("models").join(build));
    }
    let model = block_on(storage.find_in_folder("model/build", "model.onnx")).expect("found");
    assert_eq!(
        std::fs::read(model.expect("a file")).expect("read"),
        b"weights"
    );
    let phontab = block_on(storage.find_in_folder("model/build", "data/a/phontab")).expect("found");
    assert_eq!(
        std::fs::read(phontab.expect("a file")).expect("read"),
        b"phonemes"
    );
    assert_eq!(block_on(storage.is_linked("blob")), Ok(true));
    assert_eq!(block_on(storage.is_linked("tree")), Ok(true));

    // A blob two folders link stays linked until both are gone; the model's directory goes with its last build.
    block_on(storage.remove_folder("model/build")).expect("removed");
    assert_eq!(block_on(storage.find_folder("model/build")), Ok(None));
    assert_eq!(block_on(storage.is_linked("blob")), Ok(true));
    block_on(storage.remove_folder("model/other")).expect("removed");
    assert_eq!(block_on(storage.is_linked("blob")), Ok(false));
    assert_eq!(block_on(storage.is_linked("tree")), Ok(false));
    assert_eq!(entries(&scratch.0.join("models")), 0);
    assert_eq!(
        block_on(storage.remove_folder("model/other")),
        Ok(()),
        "nothing to remove"
    );

    for name in ["", "../escape", "a//b", "a/./b", "a b"] {
        let refused = block_on(storage.find_folder(name)).map_err(|error| error.code);
        assert_eq!(refused, Err("storage-name-invalid"), "{name:?}");
    }
    let mut folder = block_on(storage.create_folder("model/third")).expect("folder");
    assert!(
        block_on(folder.link("x", "missing", None)).is_err(),
        "no such blob"
    );
    assert!(
        block_on(folder.link("../x", "blob", None)).is_err(),
        "outside the folder"
    );
}

#[test]
fn a_name_stored_twice_is_stored_once_and_leaves_nothing_partial_whether_file_or_tree() {
    let scratch = Scratch::new();
    let host = NativeHost::new(&scratch.0).expect("host");
    let storage = host.storage();
    let partial = scratch.0.join("partial");

    for _ in 0..2 {
        let mut file = block_on(storage.create("same")).expect("writer");
        block_on(file.write(b"bytes")).expect("written");
        block_on(file.commit()).expect("committed, the second time too");
        let mut tree = storage.create_tree("same-unpacked").expect("tree");
        tree.file("a/b").expect("file");
        tree.write(b"member").expect("written");
        tree.commit().expect("committed, the second time too");
    }
    assert_eq!(entries(&partial), 0);
    let member = block_on(storage.find_member("same-unpacked", "a/b")).expect("found");
    assert_eq!(
        std::fs::read(member.expect("a member")).expect("read"),
        b"member"
    );
}
