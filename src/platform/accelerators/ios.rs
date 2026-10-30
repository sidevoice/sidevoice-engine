use super::Nvidia;

/// iOS: the CPU only. Metal and Core ML are part of the system, but no backend runs a model on them there yet:
/// sherpa-onnx's ONNX Runtime is not known to carry the Core ML provider in the xcframework. A candidate the engine
/// cannot use would only rank builds for an accelerator they do not get.
pub struct Os;

impl super::Accelerators for Os {
    const APPLE: bool = false;

    fn nvidia() -> Nvidia {
        Nvidia::Absent
    }
}
