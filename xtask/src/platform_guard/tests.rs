use super::*;

#[test]
fn scans_multiline_cfgs_and_apis_but_ignores_literals_and_wasm() {
    let source = r#"
        #[cfg(
            any(unix, target_os = "windows")
        )]
        #[cfg_attr(target_family = "unix", allow(dead_code))]
        fn os() { cfg!(windows); std::os::fd; libc::x; rustix::x;
            windows_sys::x; windows::x; objc2::x; core_foundation::x; }
        #[cfg(target_family = "wasm")]
        #[cfg(target_arch = "wasm32")]
        fn host() { let _ = "libc::x cfg!(unix)"; }
        // std::os::fd cfg!(windows)
    "#;
    assert_eq!(occurrences(source.parse().unwrap()), 10);
    assert!(exempt(Path::new("src/platform/memory/linux.rs")));
    assert!(exempt(Path::new("xtask/src/platform.rs")));
    assert!(!exempt(Path::new("tests/platform_test.rs")));
}

#[test]
fn debt_allows_existing_or_lower_counts_but_rejects_growth_and_new_files() {
    let debt = BTreeMap::from([("old.rs".into(), 2), ("removed.rs".into(), 1)]);
    assert!(compare(&BTreeMap::from([("old.rs".into(), 2)]), &debt).is_empty());
    assert!(compare(&BTreeMap::from([("old.rs".into(), 1)]), &debt).is_empty());
    let errors = compare(
        &BTreeMap::from([("old.rs".into(), 3), ("new.rs".into(), 1)]),
        &debt,
    );
    assert_eq!(errors.len(), 2);
}

#[test]
fn debt_file_itself_cannot_add_files_or_raise_allowances() {
    let base = parse_debt("old.rs 2\nremoved.rs 1\n").unwrap();
    assert!(compare_debt(&parse_debt("old.rs 1\n").unwrap(), &base).is_empty());
    assert!(compare_debt(&base, &base).is_empty());
    let debt = parse_debt("old.rs 3\nnew.rs 1\nzero.rs 0\n").unwrap();
    assert_eq!(compare_debt(&debt, &base).len(), 3);
}

#[test]
fn grouped_and_renamed_imports_keep_the_os_path_segments() {
    let source = "use std::{os::unix::fs::MetadataExt}; use std::os as host_os; use libc as c;";
    assert_eq!(occurrences(source.parse().unwrap()), 3);
    let source = "use {std::{io, os as host_os}, rustix as r, windows_sys as w, windows as win, objc2 as o, core_foundation as cf};";
    assert_eq!(occurrences(source.parse().unwrap()), 6);
    assert_eq!(
        occurrences("use std::{io, fs}; use ordinary as libc;".parse().unwrap()),
        0
    );
}
