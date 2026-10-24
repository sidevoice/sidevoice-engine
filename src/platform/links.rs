//! Hard-link counts, which decide whether uninstalling frees a blob: a blob stored by digest is hard-linked into each
//! build's folder, and is deleted once only its own name is left.

use std::io;
use std::path::Path;

/// What the engine asks of the file system about hard links.
pub trait Links {
    /// How many names the file at `path` has on disk (1 for a file nothing else links), following symbolic links.
    ///
    /// # Errors
    ///
    /// Whatever the OS says when the file cannot be opened or inspected.
    fn count(path: &Path) -> io::Result<u64>;
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::Fs as Files;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Fs as Files;

#[cfg(test)]
mod tests {
    use super::{Files, Links};

    #[test]
    fn it_counts_the_names_of_a_file() {
        let dir =
            std::env::temp_dir().join(format!("sidevoice-engine-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        let (blob, first, second) = (dir.join("blob"), dir.join("first"), dir.join("second"));
        std::fs::write(&blob, b"weights").expect("a file");
        assert_eq!(Files::count(&blob).expect("counted"), 1);

        std::fs::hard_link(&blob, &first).expect("a link");
        std::fs::hard_link(&blob, &second).expect("a link");
        for name in [&blob, &first, &second] {
            assert_eq!(Files::count(name).expect("counted"), 3);
        }

        std::fs::remove_file(&first).expect("removed");
        std::fs::remove_file(&second).expect("removed");
        assert_eq!(Files::count(&blob).expect("counted"), 1);
        assert!(Files::count(&dir.join("missing")).is_err());
        std::fs::remove_dir_all(&dir).expect("removed");
    }
}
