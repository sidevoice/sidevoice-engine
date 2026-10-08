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
