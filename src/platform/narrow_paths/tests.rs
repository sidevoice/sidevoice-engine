//! Narrow paths, on every OS.

use std::path::Path;

use super::{NarrowPaths, Paths};

#[test]
fn an_ascii_path_always_opens() {
    assert!(Paths::opens(&Path::new("models").join("espeak ng data")));
}
