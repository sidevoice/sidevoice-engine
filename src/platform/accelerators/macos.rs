use super::Nvidia;

/// macOS: Metal and Core ML are part of the system; no NVIDIA driver ships for it.
pub struct Os;

impl super::Accelerators for Os {
    const APPLE: bool = true;

    fn nvidia() -> Nvidia {
        Nvidia::Absent
    }
}
