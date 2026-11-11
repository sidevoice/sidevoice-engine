//! Which memory a model on each accelerator is checked against.

use super::{Accelerator, AcceleratorMemory, Capabilities, Runs};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

fn caps(memory_mb: Option<u32>, accelerator_memory: Vec<AcceleratorMemory>) -> Capabilities {
    Capabilities {
        runs: Runs::Native,
        os: "macos".to_owned(),
        arch: "aarch64".to_owned(),
        accelerators: vec![Accelerator::Metal, Accelerator::CoreMl, Accelerator::Cpu],
        memory_mb,
        cores: Some(8),
        accelerator_memory,
        gpu_stops_in_background: false,
    }
}

#[test]
fn a_gpu_with_its_own_figure_is_checked_against_it_one_left_out_has_none_and_a_cpu_takes_the_machines(
) {
    let metal = AcceleratorMemory {
        accelerator: Accelerator::Metal,
        mb: 10_922,
    };
    let mac = caps(Some(16_384), vec![metal]);
    assert_eq!(mac.memory_for(Accelerator::Metal), Some(10_922));
    assert_eq!(mac.memory_for(Accelerator::Cpu), Some(16_384));
    // Core ML has no figure here: it has no memory, and no model runs on it.
    assert_eq!(mac.memory_for(Accelerator::CoreMl), Some(0));
    // Nor does any other accelerator left out.
    assert_eq!(mac.memory_for(Accelerator::Cuda), Some(0));
    // A CPU is always the machine's, whatever an accelerator reports.
    let odd = caps(
        Some(8_192),
        vec![AcceleratorMemory {
            accelerator: Accelerator::Cpu,
            mb: 1,
        }],
    );
    assert_eq!(odd.memory_for(Accelerator::Cpu), Some(8_192));
}

#[test]
fn unknown_memory_is_unknown_unless_the_accelerator_says() {
    let metal = AcceleratorMemory {
        accelerator: Accelerator::Metal,
        mb: 4_096,
    };
    let unknown = caps(None, vec![metal]);
    assert_eq!(unknown.memory_for(Accelerator::Cpu), None);
    assert_eq!(unknown.memory_for(Accelerator::Metal), Some(4_096));
    // With the machine's memory unknown too, an accelerator left out is not checked either.
    assert_eq!(unknown.memory_for(Accelerator::CoreMl), None);
    assert_eq!(caps(None, Vec::new()).memory_for(Accelerator::Metal), None);
}
