use libloading::Library;

/// Linux: `libvulkan.so.1`, by the dynamic linker's search of the system's libraries.
pub struct Os;

impl super::Loader for Os {
    fn open() -> Option<Library> {
        // SAFETY: the Vulkan loader runs no code on load that depends on the caller.
        unsafe { Library::new("libvulkan.so.1") }.ok()
    }
}
