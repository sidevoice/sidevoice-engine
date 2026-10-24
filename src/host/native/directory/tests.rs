use super::linked;
use crate::platform::links::Links;
use std::{io, path::Path};

struct Failed;
impl Links for Failed {
    fn count(_: &Path) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "inspection denied",
        ))
    }
}

struct Inconclusive;
impl Links for Inconclusive {
    fn count(_: &Path) -> io::Result<u64> {
        Ok(0)
    }
}

#[test]
fn it_keeps_blobs_when_link_inspection_fails_or_is_inconclusive() {
    // This source file exists on every test host; injecting the count avoids OS-specific permission tricks.
    let path = Path::new(file!());
    assert!(path.is_file());
    assert!(linked::<Failed>(path));
    assert!(linked::<Inconclusive>(path));
    assert!(linked::<Failed>(path.parent().expect("a parent directory")));
}
