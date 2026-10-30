use nvml_wrapper::Nvml;

use super::Nvidia;

/// Windows: NVML, the NVIDIA driver's management library, which the driver installs and `nvml-wrapper` loads at run
/// time. Loading it and listing devices replaces looking for `nvcuda.dll` on disk, which shows neither that it loads nor
/// that there is a device.
pub struct Os;

impl super::Accelerators for Os {
    const APPLE: bool = false;

    fn nvidia() -> Nvidia {
        // Any failure to start (no library, no driver, a driver of another version) is no driver.
        let Ok(nvml) = Nvml::init() else {
            return Nvidia::Absent;
        };
        match nvml.device_count() {
            Ok(count) if count > 0 => Nvidia::Present,
            _ => Nvidia::NoAdapter,
        }
    }
}
