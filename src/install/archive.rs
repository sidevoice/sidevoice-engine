//! Unpacking an archive the installer has downloaded and checked against its digest: a tar, compressed with bzip2 or
//! not, told apart by its first bytes (never by its URL), read synchronously from storage by `tar` and written into a
//! storage tree. Native builds only: archives come only with native-only builds (sherpa-onnx's release assets), and
//! the web build refuses them before downloading (`archive-unsupported`, in the installer).
//!
//! What comes out is checked, whatever the archive says:
//!
//! - only directories and regular files are unpacked; links (symbolic or hard), devices and anything else fail with
//!   `archive-entry-unsupported`, so nothing can point outside the tree;
//! - each path is relative and stays inside: an absolute path, a `..` segment, a backslash, a colon or a control
//!   character fails with `archive-entry-unsupported` (`.` and empty segments are dropped);
//! - the total size and the number of entries are bounded ([`Limits`]), `archive-too-large` beyond;
//! - a header whose checksum is wrong, an archive cut short, or one that does not decompress, fails with
//!   `archive-corrupt`.
//!
//! `tar` reads ustar and GNU headers, GNU long names and pax extended headers; the paths it reports are checked here.

use std::cell::Cell;
use std::io::{self, Read};

use super::member_path;
use crate::host::TreeWriter;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// How much an archive may unpack to.
#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    /// Bytes of all its files together.
    pub(super) bytes: u64,
    /// Files and directories.
    pub(super) entries: usize,
}

impl Limits {
    /// Far above any model or library the catalogue knows (the largest unpack to a few GB), far below a full disk.
    pub(super) const DEFAULT: Self = Self {
        bytes: 64 << 30,
        entries: 100_000,
    };
}

/// The size of the parts a file is copied in.
const PART: usize = 64 * 1024;

/// Unpacks `archive` into `tree`, asking `check` (cancellation) between parts. `tree` is left uncommitted. Blocks:
/// run it on a thread that may.
pub(super) fn unpack(
    archive: impl Read,
    tree: &mut dyn TreeWriter,
    limits: Limits,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    // Storage failing is not the archive's fault: told apart from what `tar` or bzip2 make of the bytes.
    let failed = Cell::new(false);
    let mut archive = Source {
        inner: archive,
        failed: &failed,
    };
    let read_error = |_| {
        if failed.get() {
            Error::new("storage-failed")
        } else {
            corrupt()
        }
    };
    let mut head = Vec::with_capacity(3);
    (&mut archive)
        .take(3)
        .read_to_end(&mut head)
        .map_err(read_error)?;
    let bzip2 = head.starts_with(b"BZh");
    let archive = io::Cursor::new(head).chain(archive);
    let archive: Box<dyn Read + '_> = if bzip2 {
        Box::new(bzip2::read::MultiBzDecoder::new(archive))
    } else {
        Box::new(archive)
    };

    let mut archive = tar::Archive::new(archive);
    let (mut entries, mut bytes) = (0, 0_u64);
    let mut count = |size: u64| {
        entries += 1;
        bytes = bytes.saturating_add(size);
        if entries > limits.entries || bytes > limits.bytes {
            return Err(Error::new("archive-too-large"));
        }
        Ok(())
    };
    for entry in archive.entries().map_err(read_error)? {
        check()?;
        let mut entry = entry.map_err(read_error)?;
        let path = String::from_utf8(entry.path_bytes().into_owned()).map_err(|_| unsupported())?;
        match entry.header().entry_type() {
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                let size = entry.size();
                count(size)?;
                tree.file(&member_path(&path).ok_or_else(unsupported)?)?;
                copy(&mut entry, size, tree, check).map_err(|error| match error {
                    Copy::Read(error) => read_error(error),
                    Copy::Other(error) => error,
                })?;
            }
            tar::EntryType::Directory => {
                count(0)?;
                // The archive's root ("./") names nothing to create.
                if let Some(path) = member_path(&path) {
                    tree.directory(&path)?;
                } else if !path.split('/').all(|segment| matches!(segment, "" | ".")) {
                    return Err(unsupported());
                }
            }
            tar::EntryType::XGlobalHeader => {}
            _ => return Err(unsupported()),
        }
    }
    Ok(())
}

/// Why copying a file failed: reading it, or anything else (the tree, cancellation).
enum Copy {
    Read(io::Error),
    Other(Error),
}

/// Copies `size` bytes of `entry` into the file started last in `tree`; an entry that ends before is cut short.
fn copy(
    entry: &mut impl Read,
    size: u64,
    tree: &mut dyn TreeWriter,
    check: &dyn Fn() -> Result<()>,
) -> std::result::Result<(), Copy> {
    let mut part = vec![0; PART];
    let mut left = size;
    while left > 0 {
        check().map_err(Copy::Other)?;
        let read = match entry.read(&mut part) {
            Ok(0) => return Err(Copy::Read(io::ErrorKind::UnexpectedEof.into())),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(Copy::Read(error)),
        };
        tree.write(&part[..read]).map_err(Copy::Other)?;
        left = left.saturating_sub(read as u64);
    }
    Ok(())
}

/// The stored archive, noting in `failed` whether reading it failed.
struct Source<'a, R> {
    inner: R,
    failed: &'a Cell<bool>,
}

impl<R: Read> Read for Source<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf).inspect_err(|error| {
            if error.kind() != io::ErrorKind::Interrupted {
                self.failed.set(true);
            }
        })
    }
}

fn corrupt() -> Error {
    Error::new("archive-corrupt")
}

fn unsupported() -> Error {
    Error::new("archive-entry-unsupported")
}
