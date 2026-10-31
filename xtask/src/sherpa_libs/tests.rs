//! What `sherpa-libs` reads, offline: the version Cargo.toml pins, the archive names, and the pinned file agreeing
//! with both.

use std::io::Write;
use std::{env, fs};

use super::{archive_names, cached, crate_version, pins, unpack_android};
use crate::{empty_dir, read, repo, sha256, write};

#[test]
fn the_crate_version_is_read_from_its_exact_pin() {
    let manifest =
        "[dependencies]\nsherpa-onnx = { version = \"=1.2.3\", default-features = false }\n";
    assert_eq!(crate_version(manifest).as_deref(), Ok("1.2.3"));
    assert!(
        crate_version("sherpa-onnx = \"1.2.3\"\n").is_err(),
        "not exact"
    );
}

#[test]
fn the_pinned_archives_are_the_build_scripts_for_the_version_cargo_toml_pins() {
    let manifest = String::from_utf8_lossy(&read(&repo().join("Cargo.toml")).expect("Cargo.toml"))
        .into_owned();
    let pins = pins().expect("the pinned archives");
    assert_eq!(Ok(pins.version.clone()), crate_version(&manifest));
    let names: Vec<_> = pins
        .archives
        .iter()
        .map(|(platform, archive)| (platform.clone(), archive.name.clone()))
        .collect();
    let mut expected = archive_names(&pins.version);
    expected.sort();
    assert_eq!(names, expected);
    for archive in pins.archives.values() {
        assert_eq!(archive.sha256.len(), 64, "{}", archive.name);
    }
}

/// A `.tar.bz2` holding `files`.
fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    for (path, data) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        tar.append_data(&mut header, path, *data).expect("append");
    }
    let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    bz.write_all(&tar.into_inner().expect("tar")).expect("bz2");
    bz.finish().expect("bz2")
}

#[test]
fn a_cached_archive_is_only_its_checked_bytes() {
    let dir = env::temp_dir().join("xtask-sherpa-libs-cached");
    empty_dir(&dir).expect("scratch");
    let path = dir.join("archive.tar.bz2");
    let pinned = archive(&[("jniLibs/arm64-v8a/lib.so", b"pinned")]);
    write(&path, &pinned).expect("write");
    let digest = sha256(&pinned);

    let bytes = cached(&path, &digest).expect("read").expect("matches");
    assert_eq!(bytes, pinned);

    // Replaced after the check: what was checked is what gets unpacked, and the replacement is not.
    write(
        &path,
        &archive(&[("jniLibs/arm64-v8a/lib.so", b"replaced")]),
    )
    .expect("replace");
    let libs = unpack_android(&bytes, &dir.join("unpacked")).expect("unpacks");
    assert_eq!(
        read(&libs.join("arm64-v8a/lib.so")).expect("lib"),
        b"pinned"
    );

    // And now the file is no longer the pinned one, nor is a missing one.
    assert_eq!(cached(&path, &digest), Ok(None));
    assert_eq!(cached(&dir.join("missing"), &digest), Ok(None));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_android_archive_that_is_not_one_is_refused() {
    let dir = env::temp_dir().join("xtask-sherpa-libs-refused");
    empty_dir(&dir).expect("scratch");
    let no_libs = archive(&[("other/lib.so", b"x")]);
    assert!(unpack_android(&no_libs, &dir.join("no-libs")).is_err());
    assert!(unpack_android(b"not a bz2", &dir.join("garbage")).is_err());
    let _ = fs::remove_dir_all(&dir);
}
