use super::Nvidia;

/// iOS: Metal and Core ML are part of the system, as on macOS; no NVIDIA driver ships for it. Candidates only: no
/// backend declares either here (`local_engines::WHISPER_CPP_METAL` builds whisper.cpp without Metal on iOS, and
/// sherpa-onnx runs on the CPU), so they rank no build.
pub struct Os;

impl super::Accelerators for Os {
    const APPLE: bool = true;

    fn nvidia() -> Nvidia {
        Nvidia::Absent
    }
}
