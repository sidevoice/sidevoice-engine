use super::Nvidia;

/// iOS: Metal and Core ML are part of the system, as on macOS; no NVIDIA driver ships for it. Candidates only: whisper.cpp
/// runs Whisper on Metal here; no backend declares Core ML on iOS (sherpa-onnx runs on the CPU), so it ranks no build.
pub struct Os;

impl super::Accelerators for Os {
    const APPLE: bool = true;

    fn nvidia() -> Nvidia {
        Nvidia::Absent
    }
}
