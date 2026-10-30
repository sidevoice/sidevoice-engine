use std::path::{Path, PathBuf};

/// Unix: narrow paths are bytes and the engine's are UTF-8, so a path is its own narrow name.
pub struct Fs;

impl super::NarrowPaths for Fs {
    fn narrow(path: &Path) -> Option<PathBuf> {
        Some(path.to_path_buf())
    }
}
