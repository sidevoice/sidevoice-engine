use std::path::{Path, PathBuf};
use std::{env, fs};

use sha2::{Digest, Sha256};

/// Windows: the path itself when it is ASCII (any code page reads it); otherwise an NTFS junction to it under
/// `%ProgramData%\sidevoice-engine\narrow`, which is ASCII where `%ProgramData%` is (`C:\ProgramData`), named by a
/// digest of the path. A name that exists is used only if it is a junction to this very path: another user may have
/// made it, and a library must not read its data from there. Short (8.3) names are not an option: hosted runners and
/// many machines have them turned off.
pub struct Fs;

impl super::NarrowPaths for Fs {
    fn narrow(path: &Path) -> Option<PathBuf> {
        if path.to_str().is_some_and(str::is_ascii) {
            return Some(path.to_path_buf());
        }
        let digest = Sha256::digest(path.to_str()?.as_bytes());
        let name: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
        let dir = PathBuf::from(env::var_os("ProgramData")?).join("sidevoice-engine/narrow");
        let link = dir.join(name);
        link.to_str().is_some_and(str::is_ascii).then_some(())?;
        if junction::exists(&link).unwrap_or(false) {
            return (junction::get_target(&link).ok()? == path).then_some(link);
        }
        fs::create_dir_all(&dir).ok()?;
        junction::create(path, &link).ok()?;
        Some(link)
    }
}
