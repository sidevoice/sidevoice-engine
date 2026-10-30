use std::path::Path;

use windows_sys::Win32::Globalization::GetACP;

/// The code page number of UTF-8.
const UTF8: u32 = 65001;

/// Windows: a path opens when it is ASCII (every ANSI code page reads it) or the process's ANSI code page is UTF-8,
/// which the executable's manifest asks for (`activeCodePage`).
pub struct Fs;

impl super::NarrowPaths for Fs {
    fn opens(path: &Path) -> bool {
        // SAFETY: `GetACP` takes no arguments and only reads the process's code page.
        path.to_str().is_some_and(str::is_ascii) || unsafe { GetACP() } == UTF8
    }
}
