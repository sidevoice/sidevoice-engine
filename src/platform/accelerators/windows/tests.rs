//! The paths NVML may be loaded from. Windows only, because they are Windows paths: nothing here can be phrased for
//! another OS.

use std::ffi::OsString;
use std::path::PathBuf;

use super::library_paths;

fn root(path: &str) -> Option<OsString> {
    Some(OsString::from(path))
}

#[test]
fn nvml_is_loaded_from_system32_then_the_nvsmi_folder_by_absolute_path() {
    let paths = library_paths(root(r"C:\Windows"), root(r"C:\Program Files"));
    assert_eq!(
        paths,
        [
            PathBuf::from(r"C:\Windows\System32\nvml.dll"),
            PathBuf::from(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvml.dll"),
        ]
    );
    assert!(paths.iter().all(|path| path.is_absolute()));
}

#[test]
fn a_missing_empty_or_relative_root_gives_no_path_so_the_working_directory_is_never_searched() {
    assert!(library_paths(None, None).is_empty());
    assert!(library_paths(root(""), root("")).is_empty());
    assert!(library_paths(root("Windows"), root(r".\Program Files")).is_empty());
    assert_eq!(
        library_paths(root("System"), root(r"D:\Programs")),
        [PathBuf::from(
            r"D:\Programs\NVIDIA Corporation\NVSMI\nvml.dll"
        )]
    );
}
