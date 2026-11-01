use super::*;

#[test]
fn the_runtime_libraries_are_packaged_per_abi_and_nothing_else() {
    let root = env::temp_dir().join("xtask-android-package");
    empty_dir(&root).expect("scratch");
    let libs = root.join("libs");
    for abi in ABIS {
        fs::create_dir_all(libs.join(abi)).expect("abi");
        for name in RUNTIME_LIBS.iter().chain(&["libother.so"]) {
            fs::write(libs.join(abi).join(name), abi).expect("lib");
        }
    }
    let out = root.join("out");
    package(&libs, &out).expect("package");
    for abi in ABIS {
        let mut names: Vec<_> = fs::read_dir(out.join(abi))
            .expect("abi dir")
            .map(|e| e.expect("entry").file_name().into_string().expect("utf8"))
            .collect();
        names.sort();
        let mut want = RUNTIME_LIBS.to_vec();
        want.sort_unstable();
        assert_eq!(names, want);
        assert_eq!(
            fs::read_to_string(out.join(abi).join(RUNTIME_LIBS[0])).expect("copy"),
            abi
        );
    }
    fs::remove_dir_all(&root).expect("clean");
}

#[test]
fn a_missing_library_is_an_error_naming_it() {
    let root = env::temp_dir().join("xtask-android-package-missing");
    empty_dir(&root).expect("scratch");
    let error = package(&root.join("none"), &root.join("out")).expect_err("no libraries");
    assert!(error.contains(RUNTIME_LIBS[0]), "{error}");
    fs::remove_dir_all(&root).expect("clean");
}
