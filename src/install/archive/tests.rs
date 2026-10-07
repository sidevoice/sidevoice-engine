//! Unpacking synthetic archives, bzip2-compressed or not, handed over a few bytes at a time: what a tar holds comes
//! out as a tree, and what could escape the tree, link out of it, or grow without bound is refused.

use super::{member_path, unpack, Limits};
use crate::host::Host;
use crate::install::Cancel;
use crate::test_support::{block_on, bzip2, tar, MemoryHost, MemoryTree, TarEntry};
use crate::Result;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// What `archive` unpacks to, within `limits`.
fn unpacked(archive: &[u8], limits: Limits) -> Result<MemoryTree> {
    let host = MemoryHost::default();
    host.store("archive", archive);
    let storage = host.storage();
    block_on(async {
        let bytes = storage.read("archive").await?;
        let mut tree = storage.create_tree("tree").await?;
        unpack(bytes, tree.as_mut(), limits, &Cancel::new()).await?;
        tree.commit().await
    })?;
    Ok(host.tree("tree").expect("committed"))
}

fn code(result: Result<MemoryTree>) -> &'static str {
    result.expect_err("refused").code
}

fn sample() -> Vec<u8> {
    tar(&[
        TarEntry::Directory("./"),
        TarEntry::Directory("lib/"),
        TarEntry::File("lib/libfake.so", b"a library"),
        TarEntry::File("./README", b""),
        TarEntry::LongNamed(&format!("data/{}/voice", "v".repeat(120)), b"long"),
        TarEntry::PaxNamed("data/pax named", b"pax"),
    ])
}

fn expected() -> MemoryTree {
    [
        ("lib".to_owned(), None),
        ("lib/libfake.so".to_owned(), Some(b"a library".to_vec())),
        ("README".to_owned(), Some(Vec::new())),
        (
            format!("data/{}/voice", "v".repeat(120)),
            Some(b"long".to_vec()),
        ),
        ("data/pax named".to_owned(), Some(b"pax".to_vec())),
    ]
    .into()
}

#[test]
fn a_tar_unpacks_to_its_directories_and_files_compressed_or_not() {
    assert_eq!(unpacked(&sample(), Limits::DEFAULT), Ok(expected()));
    assert_eq!(unpacked(&bzip2(&sample()), Limits::DEFAULT), Ok(expected()));
}

#[test]
fn paths_that_leave_the_tree_are_refused() {
    for path in [
        "../evil",
        "/etc/passwd",
        "lib/../../evil",
        "C:/evil",
        "lib\\..\\evil",
    ] {
        let archive = tar(&[TarEntry::File(path, b"x")]);
        assert_eq!(
            code(unpacked(&archive, Limits::DEFAULT)),
            "archive-entry-unsupported",
            "{path}"
        );
    }
}

#[test]
fn links_are_refused_wherever_they_point() {
    for target in ["/etc/passwd", "../outside", "lib/libfake.so"] {
        let archive = tar(&[
            TarEntry::File("lib/libfake.so", b"x"),
            TarEntry::Symlink("lib/link.so", target),
        ]);
        assert_eq!(
            code(unpacked(&bzip2(&archive), Limits::DEFAULT)),
            "archive-entry-unsupported"
        );
    }
}

#[test]
fn an_archive_larger_than_the_limits_is_refused() {
    let bytes = Limits {
        bytes: 8,
        entries: 100,
    };
    let archive = tar(&[TarEntry::File("a", b"12345"), TarEntry::File("b", b"6789")]);
    assert_eq!(code(unpacked(&archive, bytes)), "archive-too-large");

    let entries = Limits {
        bytes: 1 << 20,
        entries: 2,
    };
    let archive = tar(&[
        TarEntry::Directory("a"),
        TarEntry::File("a/b", b""),
        TarEntry::File("a/c", b""),
    ]);
    assert_eq!(code(unpacked(&archive, entries)), "archive-too-large");
}

#[test]
fn a_damaged_or_cut_short_archive_is_corrupt() {
    let mut damaged = sample();
    damaged[512 + 10] ^= 1; // a byte of the second header's name: its checksum no longer matches
    assert_eq!(code(unpacked(&damaged, Limits::DEFAULT)), "archive-corrupt");

    let cut = &sample()[..512 * 2 + 100]; // inside the first file's content
    assert_eq!(code(unpacked(cut, Limits::DEFAULT)), "archive-corrupt");

    let compressed = bzip2(&sample());
    let cut = &compressed[..compressed.len() / 2];
    assert_eq!(code(unpacked(cut, Limits::DEFAULT)), "archive-corrupt");
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
