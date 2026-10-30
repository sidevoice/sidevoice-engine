//! The narrow name of a path, on every OS.

use super::{NarrowPaths, Paths};

#[test]
fn an_ascii_path_is_its_own_narrow_name() {
    let path = std::path::Path::new("models").join("espeak ng data");
    assert_eq!(Paths::narrow(&path), Some(path));
}

#[test]
fn a_narrow_name_is_the_path_or_ascii_and_the_same_directory() {
    let name = format!("sidevoice-engine-narrow-ñandú-{}", std::process::id());
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    if let Some(narrow) = Paths::narrow(&dir) {
        assert!(narrow == dir || narrow.to_str().is_some_and(str::is_ascii));
        assert!(narrow.is_dir());
    }
    std::fs::remove_dir_all(&dir).expect("removed");
}
