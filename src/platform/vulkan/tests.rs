//! How the loader's absence and ggml's device list become the engine's decisions. No GPU and no Vulkan loader are
//! needed: the loader is never asked, and the device list is given.

use std::ffi::{c_void, CString};

use super::loader::{failed, first_gpu, vkGetInstanceProcAddr, without_loader, Device, Kind};
use super::Gpu;

const GIB: u64 = 1024 * 1024 * 1024;

fn device(kind: Kind, name: &str, total_bytes: u64) -> Device {
    Device {
        kind,
        name: name.to_owned(),
        total_bytes,
    }
}

#[test]
fn without_a_loader_the_commands_that_need_no_instance_fail_cleanly() {
    for name in [
        "vkEnumerateInstanceVersion",
        "vkEnumerateInstanceExtensionProperties",
        "vkEnumerateInstanceLayerProperties",
        "vkCreateInstance",
    ] {
        let function = without_loader(name.as_bytes()).expect(name);
        // SAFETY: the function is `failed`, which ignores its arguments and returns a `VkResult`.
        let result = unsafe {
            std::mem::transmute::<unsafe extern "system" fn(), unsafe extern "system" fn() -> i32>(
                function,
            )()
        };
        assert_eq!(result, -3, "{name}: VK_ERROR_INITIALIZATION_FAILED");
    }
    assert_eq!(unsafe { failed() }, -3);
}

#[test]
fn without_a_loader_every_other_command_is_unknown() {
    for name in [
        "vkCreateDevice",
        "vkGetInstanceProcAddr",
        "",
        "vkcreateinstance",
    ] {
        assert!(without_loader(name.as_bytes()).is_none(), "{name:?}");
    }
}

/// On a machine that has the loader the symbol forwards to it, and on one that does not it answers as above; either
/// way an unknown command is none, a null name is none, and nothing crashes.
#[test]
fn the_entry_point_never_hands_back_garbage() {
    let unknown = CString::new("vkNoSuchCommand").unwrap();
    // SAFETY: a null instance and a NUL-terminated name, as the contract asks.
    let found = unsafe { vkGetInstanceProcAddr(std::ptr::null_mut::<c_void>(), unknown.as_ptr()) };
    assert!(found.is_none());
}

#[test]
fn whisper_takes_the_first_gpu_of_the_list_whatever_comes_before_it() {
    let gpu = first_gpu([
        device(Kind::Cpu, "CPU", 64 * GIB),
        device(Kind::Accelerator, "BLAS", 0),
        device(Kind::Gpu, "Vulkan0", 12 * GIB),
        device(Kind::Gpu, "Vulkan1", 24 * GIB),
    ]);
    assert_eq!(
        gpu,
        Some(Gpu {
            name: "Vulkan0".to_owned(),
            integrated: false,
            total_bytes: 12 * GIB
        })
    );
}

#[test]
fn a_list_without_a_gpu_has_none() {
    assert_eq!(first_gpu([]), None);
    assert_eq!(first_gpu([device(Kind::Cpu, "CPU", 8 * GIB)]), None);
}

#[test]
fn a_discrete_gpu_gives_its_own_memory_and_an_integrated_one_none() {
    let discrete = first_gpu([device(Kind::Gpu, "dGPU", 8 * GIB)]).unwrap();
    assert_eq!(discrete.model_bytes(), Some(8 * GIB));
    let integrated = first_gpu([device(Kind::Integrated, "iGPU", 16 * GIB)]).unwrap();
    assert!(integrated.integrated);
    assert_eq!(integrated.model_bytes(), None);
    let unknown = first_gpu([device(Kind::Gpu, "dGPU", 0)]).unwrap();
    assert_eq!(unknown.model_bytes(), None);
}

/// The first GPU is the one models are counted on, even when a later one is bigger: it is the one whisper.cpp uses.
#[test]
fn memory_is_the_first_gpus_not_the_largest() {
    let gpu = first_gpu([
        device(Kind::Gpu, "small", 4 * GIB),
        device(Kind::Gpu, "large", 24 * GIB),
    ])
    .unwrap();
    assert_eq!(gpu.model_bytes(), Some(4 * GIB));
}
