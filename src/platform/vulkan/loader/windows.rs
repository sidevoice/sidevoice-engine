use libloading::os::windows::{Library, LOAD_LIBRARY_SEARCH_SYSTEM32};

/// Windows: `vulkan-1.dll` from System32 alone, where the system's loader is: never from the application's folder, the
/// current directory or `PATH`, which a user or another program may write to.
pub struct Os;

impl super::Loader for Os {
    fn open() -> Option<libloading::Library> {
        // SAFETY: the Vulkan loader runs no code on load that depends on the caller.
        unsafe { Library::load_with_flags("vulkan-1.dll", LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .ok()
            .map(Into::into)
    }
}
