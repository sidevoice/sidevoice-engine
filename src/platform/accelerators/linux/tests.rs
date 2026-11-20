//! How Linux's signs become a Vulkan GPU.

use super::vulkan_from;

#[test]
fn an_nvidia_device_is_a_vulkan_gpu_by_itself() {
    assert!(vulkan_from(true, 0, 0));
}

#[test]
fn a_render_node_with_an_installed_driver_is_a_vulkan_gpu() {
    assert!(vulkan_from(false, 1, 1));
}

#[test]
fn a_driver_without_a_render_node_is_software_and_a_node_without_a_driver_is_unusable() {
    assert!(!vulkan_from(false, 0, 3));
    assert!(!vulkan_from(false, 2, 0));
    assert!(!vulkan_from(false, 0, 0));
}
