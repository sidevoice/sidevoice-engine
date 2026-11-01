use super::*;

#[test]
fn each_target_gets_its_slices_binaries_under_the_names_the_linker_asks_for() {
    let out = std::env::temp_dir().join("xtask-ios-layout");
    empty_dir(&out).expect("scratch");
    for (_, slice) in TARGETS {
        for (xcframework, name) in [
            (SHERPA_XCFRAMEWORK, SHERPA),
            (ONNXRUNTIME_XCFRAMEWORK, ONNXRUNTIME),
        ] {
            let framework = out
                .join(xcframework)
                .join(slice)
                .join(format!("{name}.framework"));
            fs::create_dir_all(&framework).expect("framework");
            fs::write(framework.join(name), slice).expect("binary");
        }
    }
    layout(&out).expect("layout");
    for (triple, slice) in TARGETS {
        let lib = out.join(format!("lib-{triple}/libsherpa-onnx-c-api.dylib"));
        assert_eq!(fs::read_to_string(lib).expect("lib"), slice);
        let runtime = out.join(format!(
            "frameworks-{triple}/onnxruntime.framework/onnxruntime"
        ));
        assert_eq!(fs::read_to_string(runtime).expect("runtime"), slice);
    }
    fs::remove_dir_all(&out).expect("clean");
}

#[test]
fn a_missing_slice_is_an_error() {
    let out = std::env::temp_dir().join("xtask-ios-layout-missing");
    empty_dir(&out).expect("scratch");
    assert!(layout(&out).is_err());
    fs::remove_dir_all(&out).expect("clean");
}
