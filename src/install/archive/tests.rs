//! Unpacking synthetic archives, bzip2-compressed or not, read a few bytes at a time: what a tar holds comes out as a
//! tree, and what could escape the tree, link out of it, or grow without bound is refused.

use super::{unpack, Limits};
use crate::host::Host;
use crate::test_support::{bzip2, tar, MemoryHost, MemoryTree, TarEntry};
use crate::{Error, Result};

/// What `archive` unpacks to, within `limits`.
fn unpacked(archive: &[u8], limits: Limits) -> Result<MemoryTree> {
    let host = MemoryHost::default();
    host.store("archive", archive);
    let storage = host.storage();
    let mut tree = storage.create_tree("tree")?;
    unpack(storage.open("archive")?, tree.as_mut(), limits, &|| Ok(()))?;
    tree.commit()?;
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
fn long_names_and_pax_paths_name_the_entry_they_precede() {
    let long = format!("data/{}/voice", "v".repeat(300));
    let archive = tar(&[
        TarEntry::LongNamed(&long, b"gnu"),
        TarEntry::File("plain", b"ustar"),
        TarEntry::PaxNamed(&format!("{long}-pax"), b"pax"),
    ]);
    let tree = unpacked(&bzip2(&archive), Limits::DEFAULT).expect("unpacked");
    assert_eq!(
        tree,
        [
            (long.clone(), Some(b"gnu".to_vec())),
            ("plain".to_owned(), Some(b"ustar".to_vec())),
            (format!("{long}-pax"), Some(b"pax".to_vec())),
        ]
        .into()
    );
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
    let archive = tar(&[TarEntry::PaxNamed("../evil", b"x")]);
    assert_eq!(
        code(unpacked(&archive, Limits::DEFAULT)),
        "archive-entry-unsupported",
        "a pax path"
    );
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
fn hard_links_devices_and_fifos_are_refused() {
    for kind in *b"1346" {
        let archive = tar(&[
            TarEntry::File("lib/libfake.so", b"x"),
            TarEntry::Special("lib/other", kind),
        ]);
        assert_eq!(
            code(unpacked(&archive, Limits::DEFAULT)),
            "archive-entry-unsupported",
            "{}",
            char::from(kind)
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

    let cut = &sample()[..512 * 2 + 100]; // inside the third header
    assert_eq!(code(unpacked(cut, Limits::DEFAULT)), "archive-corrupt");

    let cut = &sample()[..512 * 3 + 4]; // inside the first file's content
    assert_eq!(code(unpacked(cut, Limits::DEFAULT)), "archive-corrupt");

    let compressed = bzip2(&sample());
    let cut = &compressed[..compressed.len() / 2];
    assert_eq!(code(unpacked(cut, Limits::DEFAULT)), "archive-corrupt");
}

#[test]
fn storage_failing_is_not_the_archive_being_corrupt() {
    struct Failing;
    impl std::io::Read for Failing {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::Other.into())
        }
    }
    let host = MemoryHost::default();
    let mut tree = host.storage().create_tree("tree").expect("tree");
    let failed = unpack(Failing, tree.as_mut(), Limits::DEFAULT, &|| Ok(()));
    assert_eq!(failed, Err(Error::new("storage-failed")));
}

#[test]
fn unpacking_stops_once_asked_to() {
    let host = MemoryHost::default();
    host.store("archive", &sample());
    let storage = host.storage();
    let mut tree = storage.create_tree("tree").expect("tree");
    let stopped = unpack(
        storage.open("archive").expect("stored"),
        tree.as_mut(),
        Limits::DEFAULT,
        &|| Err(Error::new("cancelled")),
    );
    assert_eq!(stopped, Err(Error::new("cancelled")));
}
