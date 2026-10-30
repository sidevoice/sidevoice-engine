use std::path::Path;

/// Unix: narrow paths are bytes and the engine's are UTF-8, so every path opens.
pub struct Fs;

impl super::NarrowPaths for Fs {
    fn opens(_: &Path) -> bool {
        true
    }
}
