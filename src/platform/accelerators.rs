//! What the operating system shows of the machine's accelerators: the candidates the native host reports in its
//! [`Capabilities`](crate::host::Capabilities). A candidate is a sign the OS gives (a driver, a framework), never a proof
//! that a model runs on it: each backend's `probe` and `load` find that out, and are the only ones that can say which
//! device a model really got ([`AcceleratorUse`](crate::AcceleratorUse)).

use crate::host::Accelerator;

/// What an OS shows of an NVIDIA driver.
pub enum Nvidia {
    /// No driver: nothing to load, or one that does not start.
    Absent,
    /// A driver that starts but lists no NVIDIA device.
    NoAdapter,
    /// A driver that starts and lists at least one NVIDIA device.
    Present,
}

/// What the engine asks of the OS about accelerators. An OS says what it shows; this trait decides what the host reports.
pub trait Accelerators {
    /// Whether Apple's GPU and neural-engine frameworks (Metal, Core ML) are part of this OS.
    const APPLE: bool;

    /// What the OS shows of an NVIDIA driver.
    fn nvidia() -> Nvidia;

    /// The accelerators the host reports here: the CPU always, Metal and Core ML where [`Accelerators::APPLE`], CUDA
    /// where [`Accelerators::nvidia`] lists a device. Candidates, not proof.
    #[must_use]
    fn detect() -> Vec<Accelerator> {
        let mut accelerators = vec![Accelerator::Cpu];
        if Self::APPLE {
            accelerators.extend([Accelerator::Metal, Accelerator::CoreMl]);
        }
        if matches!(Self::nvidia(), Nvidia::Present) {
            accelerators.push(Accelerator::Cuda);
        }
        accelerators
    }
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Os as Machine;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Os as Machine;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Os as Machine;

#[cfg(test)]
mod tests;
