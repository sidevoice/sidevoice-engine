use std::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;

use libloading::Library;
use whisper_rs_sys::{
    ggml_backend_dev_count, ggml_backend_dev_description, ggml_backend_dev_get,
    ggml_backend_dev_memory, ggml_backend_dev_type,
    ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_CPU as CPU,
    ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU as GPU,
    ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU as IGPU,
};

use super::Gpu;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::Os;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::Os;

/// What the engine asks of the OS about the Vulkan loader.
trait Loader {
    /// The loader's library, from where the OS keeps the system's alone; `None` when it is not installed.
    fn open() -> Option<Library>;
}

/// `VK_ERROR_INITIALIZATION_FAILED`.
pub(super) const INITIALIZATION_FAILED: i32 = -3;

/// The commands Vulkan answers without an instance; the only ones ggml asks of the loader before it has one.
const GLOBAL_COMMANDS: [&[u8]; 4] = [
    b"vkEnumerateInstanceVersion",
    b"vkEnumerateInstanceExtensionProperties",
    b"vkEnumerateInstanceLayerProperties",
    b"vkCreateInstance",
];

/// `PFN_vkVoidFunction`.
type Function = Option<unsafe extern "system" fn()>;
/// `PFN_vkGetInstanceProcAddr`.
type GetInstanceProcAddr = unsafe extern "system" fn(*mut c_void, *const c_char) -> Function;

/// The loader, opened once, and its `vkGetInstanceProcAddr`.
struct Entry {
    /// Kept open for the life of the process: the functions it gave are called until then.
    _library: Library,
    get_instance_proc_addr: GetInstanceProcAddr,
}

fn entry() -> Option<&'static Entry> {
    static ENTRY: OnceLock<Option<Entry>> = OnceLock::new();
    ENTRY
        .get_or_init(|| {
            let library = Os::open()?;
            // SAFETY: the symbol is `vkGetInstanceProcAddr`, which has this signature in every loader.
            let get_instance_proc_addr = unsafe {
                *library
                    .get::<GetInstanceProcAddr>(b"vkGetInstanceProcAddr\0")
                    .ok()?
            };
            Some(Entry {
                _library: library,
                get_instance_proc_addr,
            })
        })
        .as_ref()
}

/// What a command that fails every time stands for: it takes the arguments of whichever command it answers for and
/// ignores them. That is sound because this module builds on x86_64 only (see `super`): there `extern "system"` is the
/// one C calling convention (System V on Linux, Microsoft x64 on Windows), in which the caller owns its arguments. On
/// 32-bit Windows `system` is stdcall, where the callee pops them, and this would unbalance the stack.
pub(super) unsafe extern "system" fn failed() -> i32 {
    INITIALIZATION_FAILED
}

/// The function for `name` when there is no loader: a failing one for the commands that need no instance, none for
/// the rest (there is no instance without a loader).
pub(super) fn without_loader(name: &[u8]) -> Function {
    GLOBAL_COMMANDS.contains(&name).then(|| {
        // SAFETY: see `failed`: only the return value is read, and it is a `VkResult` (an `i32`) for these commands.
        unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> i32, unsafe extern "system" fn()>(
                failed,
            )
        }
    })
}

/// Vulkan's entry point to every command, which ggml's Vulkan backend links by name (see the module's text).
///
/// # Safety
///
/// As `vkGetInstanceProcAddr`: `name` is a NUL-terminated string, and `instance` null or a `VkInstance`.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "system" fn vkGetInstanceProcAddr(
    instance: *mut c_void,
    name: *const c_char,
) -> Function {
    match entry() {
        // SAFETY: the loader's own function, with the caller's arguments.
        Some(entry) => unsafe { (entry.get_instance_proc_addr)(instance, name) },
        None if name.is_null() => None,
        // SAFETY: a non-null `name` is NUL-terminated by the contract above.
        None => without_loader(unsafe { CStr::from_ptr(name) }.to_bytes()),
    }
}

/// A function the loader exports by name, found once. ggml's Vulkan backend calls two of the core commands directly by
/// name instead of through `vkGetInstanceProcAddr` (`vkGetPhysicalDeviceFeatures2` and `vkCmdCopyBuffer`), so they are
/// defined here too, or the linker would bring the loader back in for them.
fn exported<T: Copy>(name: &'static [u8]) -> Option<T> {
    let entry = entry()?;
    // SAFETY: `T` is the signature the Vulkan specification gives the command named.
    unsafe { entry._library.get::<T>(name).ok().map(|symbol| *symbol) }
}

/// `vkGetPhysicalDeviceFeatures2`. A physical device exists only when the loader gave it, so the loader is there.
///
/// # Safety
///
/// As `vkGetPhysicalDeviceFeatures2`.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "system" fn vkGetPhysicalDeviceFeatures2(
    physical_device: *mut c_void,
    features: *mut c_void,
) {
    type Command = unsafe extern "system" fn(*mut c_void, *mut c_void);
    static COMMAND: OnceLock<Option<Command>> = OnceLock::new();
    if let Some(command) = COMMAND.get_or_init(|| exported(b"vkGetPhysicalDeviceFeatures2\0")) {
        // SAFETY: the loader's own function, with the caller's arguments.
        unsafe { command(physical_device, features) }
    }
}

/// `vkCmdCopyBuffer`. A command buffer exists only when the loader gave it, so the loader is there.
///
/// # Safety
///
/// As `vkCmdCopyBuffer`.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub unsafe extern "system" fn vkCmdCopyBuffer(
    command_buffer: *mut c_void,
    source: u64,
    destination: u64,
    region_count: u32,
    regions: *const c_void,
) {
    type Command = unsafe extern "system" fn(*mut c_void, u64, u64, u32, *const c_void);
    static COMMAND: OnceLock<Option<Command>> = OnceLock::new();
    if let Some(command) = COMMAND.get_or_init(|| exported(b"vkCmdCopyBuffer\0")) {
        // SAFETY: the loader's own function, with the caller's arguments.
        unsafe { command(command_buffer, source, destination, region_count, regions) }
    }
}

/// What a device of ggml's list is, for choosing among them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Cpu,
    Gpu,
    Integrated,
    Accelerator,
}

/// A device of ggml's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Device {
    pub kind: Kind,
    pub name: String,
    pub total_bytes: u64,
}

/// The device whisper.cpp takes: the first GPU of the list, discrete or integrated, as `whisper_backend_init_gpu`
/// counts them.
pub(super) fn first_gpu(devices: impl IntoIterator<Item = Device>) -> Option<Gpu> {
    devices
        .into_iter()
        .find(|device| matches!(device.kind, Kind::Gpu | Kind::Integrated))
        .map(|device| Gpu {
            name: device.name,
            integrated: device.kind == Kind::Integrated,
            total_bytes: device.total_bytes,
        })
}

/// ggml's devices, listed once the loader is known to be there (so ggml's backends are not even registered here
/// without it: whisper.cpp then runs on the CPU, and the registration, when a model loads, finds no Vulkan).
fn devices() -> Vec<Device> {
    if entry().is_none() {
        return Vec::new();
    }
    // SAFETY: ggml lists its devices once, behind a lock, and the devices live as long as the process.
    let count = unsafe { ggml_backend_dev_count() };
    (0..count)
        .filter_map(|index| {
            // SAFETY: `index` is below the count; a device's strings are NUL-terminated, valid for the process.
            unsafe {
                let device = ggml_backend_dev_get(index);
                if device.is_null() {
                    return None;
                }
                let kind = match ggml_backend_dev_type(device) {
                    GPU => Kind::Gpu,
                    IGPU => Kind::Integrated,
                    CPU => Kind::Cpu,
                    _ => Kind::Accelerator,
                };
                let description = ggml_backend_dev_description(device);
                let name = if description.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(description).to_string_lossy().into_owned()
                };
                let (mut free, mut total) = (0usize, 0usize);
                ggml_backend_dev_memory(device, &mut free, &mut total);
                Some(Device {
                    kind,
                    name,
                    total_bytes: total as u64,
                })
            }
        })
        .collect()
}

/// The GPU whisper.cpp runs on, asked of ggml once.
pub fn gpu() -> Option<&'static Gpu> {
    static GPU_DEVICE: OnceLock<Option<Gpu>> = OnceLock::new();
    std::hint::black_box(&KEEP);
    GPU_DEVICE
        .get_or_init(|| {
            let gpu = first_gpu(devices());
            match &gpu {
                Some(gpu) => tracing::info!(
                    device = %gpu.name,
                    integrated = gpu.integrated,
                    memory_bytes = gpu.total_bytes,
                    "Vulkan GPU for whisper.cpp"
                ),
                None => tracing::info!("no Vulkan GPU for whisper.cpp, which runs on the CPU"),
            }
            gpu
        })
        .as_ref()
}

/// The definitions above, referenced from a static that [`gpu`] reads, so that a binary built from the library keeps them:
/// the linker takes an archive member only for a symbol it already lacks, and the libraries that need these (ggml's)
/// come after the library's own object files, which would otherwise be left out and the loader linked in for them.
#[used]
#[allow(clippy::fn_to_numeric_cast, reason = "only the addresses matter: they keep the definitions linked")]
static KEEP: [usize; 3] = [
    vkGetInstanceProcAddr as usize,
    vkGetPhysicalDeviceFeatures2 as usize,
    vkCmdCopyBuffer as usize,
];
