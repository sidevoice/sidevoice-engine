//! Each accelerator's class, the classes' order (the ranking's first key) and their stable ids.

use super::{Accelerator, AcceleratorClass};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn each_accelerator_is_a_gpu_a_neural_engine_or_a_cpu() {
    let classes = [
        (Accelerator::Metal, AcceleratorClass::Gpu),
        (Accelerator::Cuda, AcceleratorClass::Gpu),
        (Accelerator::WebGpu, AcceleratorClass::Gpu),
        (Accelerator::CoreMl, AcceleratorClass::NeuralEngine),
        (Accelerator::Cpu, AcceleratorClass::Cpu),
        (Accelerator::Wasm, AcceleratorClass::Cpu),
    ];
    for (accelerator, class) in classes {
        assert_eq!(accelerator.class(), class, "{accelerator:?}");
    }
}

#[test]
fn a_gpu_ranks_before_a_neural_engine_and_a_neural_engine_before_a_cpu() {
    let mut classes = [
        AcceleratorClass::Cpu,
        AcceleratorClass::Gpu,
        AcceleratorClass::NeuralEngine,
    ];
    classes.sort();
    assert_eq!(
        classes,
        [
            AcceleratorClass::Gpu,
            AcceleratorClass::NeuralEngine,
            AcceleratorClass::Cpu
        ]
    );
}

#[test]
fn each_class_has_its_stable_id() {
    let ids = [
        AcceleratorClass::Gpu,
        AcceleratorClass::NeuralEngine,
        AcceleratorClass::Cpu,
    ]
    .map(AcceleratorClass::id);
    assert_eq!(ids, ["gpu", "neural-engine", "cpu"]);
}
