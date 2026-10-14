#[cfg(test)]
mod tests;

/// Hardware or a runtime a model can run on.
///
/// The list is fixed: each accelerator needs backend code anyway, and a fixed list lets the funnel, the catalogue and
/// the app name it. Adding one is a change to the engine. It is `#[non_exhaustive]`, so adding one is not a breaking
/// change: match it with a wildcard arm. A JavaScript host and the catalogue name each by a stable id: "cpu", "cuda",
/// "coreml", "metal", "webgpu", "wasm".
///
/// Each variant says when a host reports it: what it can see, not whether it works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Accelerator {
    /// The CPU, natively. Every native host reports it.
    Cpu,
    /// An NVIDIA GPU through CUDA. Reported when the host sees an NVIDIA GPU; the probe checks the driver.
    Cuda,
    /// Apple's Core ML. Reported on Apple platforms; the probe checks that a model loads.
    CoreMl,
    /// An Apple GPU through Metal. Reported on Apple silicon.
    Metal,
    /// The GPU from a page, through WebGPU. Reported when the page exposes `navigator.gpu`; the probe checks that an
    /// adapter is granted.
    WebGpu,
    /// The CPU from a page, through WebAssembly. Every page reports it.
    Wasm,
}

impl Accelerator {
    /// What kind of hardware it is: a GPU, a neural engine or a CPU ([`AcceleratorClass`]). The engine ranks the
    /// builds that run here by it, and the app shows it.
    #[must_use]
    pub const fn class(self) -> AcceleratorClass {
        // Exhaustive on purpose: a new accelerator does not compile until it is given its class.
        match self {
            Self::Metal | Self::Cuda | Self::WebGpu => AcceleratorClass::Gpu,
            Self::CoreMl => AcceleratorClass::NeuralEngine,
            Self::Cpu | Self::Wasm => AcceleratorClass::Cpu,
        }
    }
}

/// The kind of hardware an accelerator is: each [`Accelerator`] has one ([`Accelerator::class`]). The engine ranks the
/// builds that run here by it, in the order of its variants (a GPU first, then a neural engine, then a CPU;
/// sidevoice-engine#4); within a class, catalogue order stands until measured speed and quality rank them
/// (sidevoice-engine#16). An app shows it as it is and does not derive it again.
///
/// A JavaScript app gets it by its stable id ([`AcceleratorClass::id`]): "gpu", "neural-engine", "cpu". It is
/// `#[non_exhaustive]`: match it with a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum AcceleratorClass {
    /// A GPU: Metal, CUDA, WebGPU.
    Gpu,
    /// A neural engine: Core ML (Apple's Neural Engine, or whatever Core ML schedules it on).
    NeuralEngine,
    /// A CPU, natively or in WebAssembly.
    Cpu,
}

impl AcceleratorClass {
    /// Its stable id, as the app names it: "gpu", "neural-engine", "cpu".
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Gpu => "gpu",
            Self::NeuralEngine => "neural-engine",
            Self::Cpu => "cpu",
        }
    }
}
