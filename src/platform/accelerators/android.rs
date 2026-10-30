use super::Nvidia;

/// Android: the phone's CPU only. NNAPI, the system's neural-network API, is deprecated since Android 15 and no backend
/// asks for it; no NVIDIA driver ships for the platform.
pub struct Os;

impl super::Accelerators for Os {
    const APPLE: bool = false;

    fn nvidia() -> Nvidia {
        Nvidia::Absent
    }
}
