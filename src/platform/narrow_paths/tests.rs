//! Narrow paths, on every OS.

use std::path::Path;

use super::{NarrowPaths, Paths};

#[test]
fn an_ascii_path_always_opens() {
    assert!(Paths::opens(&Path::new("models").join("espeak ng data")));
}

/// CI sets `SIDEVOICE_TEST_MANIFEST` for the jobs that run tests, and then these run as an app must: a non-ASCII path
/// opens on Windows only if the test binary carries the UTF-8 manifest (build.rs). Fails clearly if one slipped through
/// without it.
#[test]
fn under_ci_the_tests_run_with_utf8_narrow_paths() {
    if std::env::var_os("SIDEVOICE_TEST_MANIFEST").is_some() {
        assert!(
            Paths::opens(Path::new("ñandú")),
            "this test binary has no UTF-8 manifest: SIDEVOICE_TEST_MANIFEST must be set when it is built"
        );
    }
}
