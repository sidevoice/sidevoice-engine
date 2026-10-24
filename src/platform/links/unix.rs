use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Unix: the `st_nlink` the file system reports.
pub struct Fs;

impl super::Links for Fs {
    fn count(path: &Path) -> io::Result<u64> {
        std::fs::metadata(path).map(|meta| meta.nlink())
    }
}
