//! What `sherpa-libs` reads, offline: the version Cargo.toml pins, the archive names, and the pinned file agreeing
//! with both.

use super::{archive_names, crate_version, pins};
use crate::{read, repo};

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
