use std::ffi::OsString;
use std::path::PathBuf;

use nvml_wrapper::Nvml;

use super::Nvidia;

#[cfg(test)]
mod tests;

/// Windows: NVML, the NVIDIA driver's management library, which `nvml-wrapper` loads at run time. Loading it and listing
/// devices replaces looking for `nvcuda.dll` on disk, which shows neither that it loads nor that there is a device.
///
/// It is loaded only from where the driver installs it, by absolute path ([`library_paths`]): a bare `nvml.dll` would be
/// looked for in the working directory and on `PATH` too, and a library planted there would run in a process on a
/// machine that has no NVIDIA driver at all.
pub struct Os;

impl super::Accelerators for Os {
    const APPLE: bool = false;

    fn nvidia() -> Nvidia {
        let paths = library_paths(
            std::env::var_os("SystemRoot"),
            std::env::var_os("ProgramFiles"),
        );
        // Any failure to start (no library, no driver, a driver of another version) is no driver.
        let Some(nvml) = paths
            .iter()
            .find_map(|path| Nvml::builder().lib_path(path.as_os_str()).init().ok())
        else {
            return Nvidia::Absent;
        };
        match nvml.device_count() {
            Ok(count) if count > 0 => Nvidia::Present,
            _ => Nvidia::NoAdapter,
        }
    }
}

/// Where NVML may be loaded from, in order: `System32` (where current drivers put it) and NVIDIA's documented
/// `NVSMI` folder. Only absolute paths: a root that is missing, empty or relative gives none.
fn library_paths(system_root: Option<OsString>, program_files: Option<OsString>) -> Vec<PathBuf> {
    let under = |root: Option<OsString>, rest: &[&str]| {
        let mut path = PathBuf::from(root?);
        path.extend(rest);
        path.is_absolute().then_some(path)
    };
    [
        under(system_root, &["System32", "nvml.dll"]),
        under(program_files, &["NVIDIA Corporation", "NVSMI", "nvml.dll"]),
    ]
    .into_iter()
    .flatten()
    .collect()
}
