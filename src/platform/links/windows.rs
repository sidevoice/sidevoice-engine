use std::io;
use std::path::Path;

use winapi_util::{file, Handle};

/// Windows: `nNumberOfLinks` from `GetFileInformationByHandle` (std's `number_of_links` is unstable).
pub struct Fs;

impl super::Links for Fs {
    fn count(path: &Path) -> io::Result<u64> {
        let handle = Handle::from_path_any(path)?;
        Ok(file::information(&handle)?.number_of_links())
    }
}
