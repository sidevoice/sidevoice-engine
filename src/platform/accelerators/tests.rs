//! The decision `detect` makes from what an OS shows, against fake OSes that show each case.

use super::{Accelerators, Machine, Nvidia};
use crate::host::Accelerator;

struct NoDriver;
struct DriverWithoutAdapter;
struct DriverWithAdapter;
struct AppleSystem;

impl Accelerators for NoDriver {
    const APPLE: bool = false;
    fn nvidia() -> Nvidia {
        Nvidia::Absent
    }
}

impl Accelerators for DriverWithoutAdapter {
    const APPLE: bool = false;
    fn nvidia() -> Nvidia {
        Nvidia::NoAdapter
    }
}

impl Accelerators for DriverWithAdapter {
    const APPLE: bool = false;
    fn nvidia() -> Nvidia {
        Nvidia::Present
    }
}

impl Accelerators for AppleSystem {
    const APPLE: bool = true;
    fn nvidia() -> Nvidia {
        Nvidia::Absent
    }
}

#[test]
fn an_absent_driver_leaves_the_cpu_alone() {
    assert_eq!(NoDriver::detect(), [Accelerator::Cpu]);
}

#[test]
fn a_driver_that_lists_no_adapter_is_not_a_candidate() {
    assert_eq!(DriverWithoutAdapter::detect(), [Accelerator::Cpu]);
}

#[test]
fn a_driver_that_lists_an_adapter_makes_cuda_a_candidate_after_the_cpu() {
    assert_eq!(
        DriverWithAdapter::detect(),
        [Accelerator::Cpu, Accelerator::Cuda]
    );
}

#[test]
fn apple_systems_offer_metal_and_core_ml() {
    assert_eq!(
        AppleSystem::detect(),
        [Accelerator::Cpu, Accelerator::Metal, Accelerator::CoreMl]
    );
}

#[test]
fn this_machine_always_reports_the_cpu_first() {
    let detected = Machine::detect();
    assert_eq!(detected.first(), Some(&Accelerator::Cpu));
    assert_eq!(detected.contains(&Accelerator::Metal), Machine::APPLE);
    // Candidates only: Vulkan and DirectML are not named until a runtime proves them.
    assert!(detected.iter().all(|accelerator| matches!(
        accelerator,
        Accelerator::Cpu | Accelerator::Cuda | Accelerator::Metal | Accelerator::CoreMl
    )));
}
