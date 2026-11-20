use std::fs;
use std::path::Path;

use super::Nvidia;

/// Linux: what the NVIDIA kernel driver shows in `/proc/driver/nvidia`: its `version` once loaded, and a folder per
/// device in `gpus`. A Vulkan GPU is the one ggml lists ([`crate::platform::vulkan`]), not a guess from the OS's files.
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

    fn vulkan() -> bool {
        crate::platform::vulkan::present()
    }
}
