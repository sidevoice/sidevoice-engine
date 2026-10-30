use std::fs;
use std::path::Path;

use super::Nvidia;

/// Linux: what the NVIDIA kernel driver shows in `/proc/driver/nvidia`: its `version` once loaded, and a folder per
/// device in `gpus`.
pub struct Os;

impl super::Accelerators for Os {
    const APPLE: bool = false;

    fn nvidia() -> Nvidia {
        let proc = Path::new("/proc/driver/nvidia");
        if !proc.join("version").exists() {
            return Nvidia::Absent;
        }
        // A device read properly: an unreadable folder or entry shows nothing.
        match fs::read_dir(proc.join("gpus")).map(|mut devices| devices.next()) {
            Ok(Some(Ok(_))) => Nvidia::Present,
            _ => Nvidia::NoAdapter,
        }
    }
}
