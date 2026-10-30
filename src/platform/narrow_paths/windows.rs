use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

/// Windows: the path itself when it is ASCII (any code page reads it); otherwise its 8.3 short name, which is ASCII,
/// when the volume makes short names, and `None` when it does not (the call then returns the long name).
pub struct Fs;

impl super::NarrowPaths for Fs {
    fn narrow(path: &Path) -> Option<PathBuf> {
        if path.to_str().is_some_and(str::is_ascii) {
            return Some(path.to_path_buf());
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: `wide` is NUL-terminated and outlives both calls; the second call gets a buffer of the length the
        // first one asked for.
        let short = unsafe {
            let len = GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0);
            if len == 0 {
                return None;
            }
            let mut buffer = vec![0u16; len as usize];
            let written = GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), len);
            if written == 0 || written >= len {
                return None;
            }
            buffer.truncate(written as usize);
            PathBuf::from(std::ffi::OsString::from_wide(&buffer))
        };
        short.to_str().is_some_and(str::is_ascii).then_some(short)
    }
}
