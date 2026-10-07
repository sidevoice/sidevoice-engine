//! Unpacking an archive the installer has downloaded and checked against its digest: a tar, compressed with bzip2 or
//! not, told apart by its first bytes (never by its URL), read back from storage a part at a time and written into a
//! storage tree.
//!
//! What comes out is checked, whatever the archive says:
//!
//! - only directories and regular files are unpacked; links (symbolic or hard), devices and anything else fail with
//!   `archive-entry-unsupported`, so nothing can point outside the tree;
//! - each path is relative and stays inside: an absolute path, a `..` segment, a backslash, a colon or a control
//!   character fails with `archive-entry-unsupported` (`.` and empty segments are dropped);
//! - the total size and the number of entries are bounded ([`Limits`]), `archive-too-large` beyond;
//! - a header whose checksum is wrong, or an archive cut short, fails with `archive-corrupt`.
//!
//! The tar reader takes bytes as they come (it is fed, it does not read), because storage hands them over
//! asynchronously; it understands ustar and GNU headers, GNU long names and pax `path` and `size` records.

use std::io::Write;

use crate::host::{Download, TreeWriter};
use crate::install::Cancel;
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

/// Unpacks `archive` into `tree`, checking for `cancel` between parts. `tree` is left uncommitted.
pub(super) async fn unpack(
    mut archive: Box<dyn Download>,
    tree: &mut dyn TreeWriter,
    limits: Limits,
    cancel: &Cancel,
) -> Result<()> {
    let mut decoder = Decoder::Undecided(Vec::new());
    let mut tar = Tar::new(limits);
    while let Some(part) = archive.chunk().await? {
        cancel.check()?;
        let bytes = decoder.feed(&part)?;
        for entry in tar.feed(&bytes)? {
            apply(tree, entry).await?;
        }
    }
    let rest = decoder.finish()?;
    for entry in tar.feed(&rest)? {
        apply(tree, entry).await?;
    }
    tar.finish()
}

async fn apply(tree: &mut dyn TreeWriter, entry: Entry) -> Result<()> {
    match entry {
        Entry::Directory(path) => tree.directory(&path).await,
        Entry::File(path) => tree.file(&path).await,
        Entry::Bytes(bytes) => tree.write(&bytes).await,
    }
}

/// `path` as a path inside a tree, if it is one: relative, `/`-separated, with `.` and empty segments dropped and no
/// `..`, backslash, colon or control character; `None` otherwise, or if nothing is left. Also what an artifact's
/// `archive_path` must be.
pub(super) fn member_path(path: &str) -> Option<String> {
    if path.starts_with('/') || path.len() > 4096 {
        return None;
    }
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            _ if segment
                .chars()
                .any(|c| c == '\\' || c == ':' || c.is_control()) =>
            {
                return None
            }
            _ => segments.push(segment),
        }
    }
    (!segments.is_empty() && segments.len() <= 64).then(|| segments.join("/"))
}

/// What the tar reader finds, in order: a directory, the start of a file, or the next bytes of the file started last.
#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Directory(String),
    File(String),
    Bytes(Vec<u8>),
}

/// The archive's compression, decided by its first bytes.
enum Decoder {
    /// Fewer than three bytes seen yet.
    Undecided(Vec<u8>),
    Plain,
    Bzip2(Box<bzip2::write::BzDecoder<Vec<u8>>>),
}

impl Decoder {
    /// The tar bytes `bytes` decompress to, so far.
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        if let Self::Undecided(head) = self {
            head.extend_from_slice(bytes);
            if head.len() < 3 {
                return Ok(Vec::new());
            }
            let head = std::mem::take(head);
            *self = if head.starts_with(b"BZh") {
                Self::Bzip2(Box::new(bzip2::write::BzDecoder::new(Vec::new())))
            } else {
                Self::Plain
            };
            return self.feed(&head);
        }
        match self {
            Self::Undecided(_) => unreachable!("decided above"),
            Self::Plain => Ok(bytes.to_vec()),
            Self::Bzip2(decoder) => {
                decoder.write_all(bytes).map_err(|_| corrupt())?;
                Ok(std::mem::take(decoder.get_mut()))
            }
        }
    }

    /// What is left once every byte has been fed.
    fn finish(self) -> Result<Vec<u8>> {
        match self {
            Self::Undecided(head) => Ok(head),
            Self::Plain => Ok(Vec::new()),
            Self::Bzip2(mut decoder) => decoder.finish().map_err(|_| corrupt()),
        }
    }
}

const BLOCK: usize = 512;
/// The most a GNU long name or a pax header may take.
const MAX_HEADER_DATA: u64 = 1 << 20;

/// A tar reader that is fed bytes.
struct Tar {
    limits: Limits,
    /// Bytes of a header not complete yet, or of a long name or pax header being collected.
    pending: Vec<u8>,
    state: State,
    entries: usize,
    bytes: u64,
    /// The path the next header names, from a GNU long name or a pax header.
    next_path: Option<String>,
    /// The size of the next entry, from a pax header.
    next_size: Option<u64>,
    /// Seen the end-of-archive block: everything after it is ignored.
    ended: bool,
}

enum State {
    Header,
    /// `remaining` bytes of the entry's content, then `padding` up to the next block.
    Content {
        remaining: u64,
        padding: u64,
        content: Content,
    },
}

enum Content {
    /// A file's bytes, handed on.
    File,
    /// A GNU long name.
    LongName,
    /// A pax extended header for the next entry.
    Pax,
    /// Something to skip (a pax global header).
    Skip,
}

impl Tar {
    fn new(limits: Limits) -> Self {
        Self {
            limits,
            pending: Vec::new(),
            state: State::Header,
            entries: 0,
            bytes: 0,
            next_path: None,
            next_size: None,
            ended: false,
        }
    }

    /// The entries `bytes` completes.
    fn feed(&mut self, mut bytes: &[u8]) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();
        while !bytes.is_empty() && !self.ended {
            match &mut self.state {
                State::Header => {
                    let take = (BLOCK - self.pending.len()).min(bytes.len());
                    self.pending.extend_from_slice(&bytes[..take]);
                    bytes = &bytes[take..];
                    if self.pending.len() == BLOCK {
                        let header = std::mem::take(&mut self.pending);
                        self.header(&header, &mut entries)?;
                    }
                }
                State::Content {
                    remaining,
                    padding,
                    content,
                } => {
                    if *remaining > 0 {
                        let take = usize::try_from(*remaining)
                            .unwrap_or(usize::MAX)
                            .min(bytes.len());
                        match content {
                            Content::File => entries.push(Entry::Bytes(bytes[..take].to_vec())),
                            Content::LongName | Content::Pax => {
                                self.pending.extend_from_slice(&bytes[..take]);
                            }
                            Content::Skip => {}
                        }
                        *remaining -= take as u64;
                        bytes = &bytes[take..];
                    } else if *padding > 0 {
                        let take = usize::try_from(*padding)
                            .unwrap_or(usize::MAX)
                            .min(bytes.len());
                        *padding -= take as u64;
                        bytes = &bytes[take..];
                    }
                    if let State::Content {
                        remaining: 0,
                        padding: 0,
                        ..
                    } = self.state
                    {
                        self.end_of_content()?;
                    }
                }
            }
        }
        Ok(entries)
    }

    /// Fails unless the archive ended where an entry did.
    fn finish(self) -> Result<()> {
        if self.ended || (self.pending.is_empty() && matches!(self.state, State::Header)) {
            Ok(())
        } else {
            Err(corrupt())
        }
    }

    fn header(&mut self, header: &[u8], entries: &mut Vec<Entry>) -> Result<()> {
        if header.iter().all(|&byte| byte == 0) {
            self.ended = true;
            return Ok(());
        }
        let stored = octal(&header[148..156]).ok_or_else(corrupt)?;
        let sum: u64 = header
            .iter()
            .enumerate()
            .map(|(i, &byte)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u64::from(byte)
                }
            })
            .sum();
        if sum != stored {
            return Err(corrupt());
        }
        let size = match self.next_size.take() {
            Some(size) => size,
            None => number(&header[124..136]).ok_or_else(corrupt)?,
        };
        let path = match self.next_path.take() {
            Some(path) => path,
            None => header_path(header)?,
        };
        let content = match header[156] {
            b'0' | b'\0' | b'7' => {
                self.count(size)?;
                entries.push(Entry::File(member_path(&path).ok_or_else(unsupported)?));
                Content::File
            }
            b'5' => {
                self.count(0)?;
                // The archive's root ("./") names nothing to create.
                if let Some(path) = member_path(&path) {
                    entries.push(Entry::Directory(path));
                } else if !path.split('/').all(|segment| matches!(segment, "" | ".")) {
                    return Err(unsupported());
                }
                Content::Skip
            }
            b'L' | b'x' if size > MAX_HEADER_DATA => return Err(corrupt()),
            b'L' => Content::LongName,
            b'x' => Content::Pax,
            b'g' => Content::Skip,
            _ => return Err(unsupported()),
        };
        self.state = State::Content {
            remaining: size,
            padding: (BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64,
            content,
        };
        if size == 0 {
            self.end_of_content()?;
        }
        Ok(())
    }

    /// Counts one entry of `size` bytes against the limits.
    fn count(&mut self, size: u64) -> Result<()> {
        self.entries += 1;
        self.bytes = self.bytes.saturating_add(size);
        if self.entries > self.limits.entries || self.bytes > self.limits.bytes {
            return Err(Error::new("archive-too-large"));
        }
        Ok(())
    }

    /// An entry's content and padding are over: what was collected applies to the next header.
    fn end_of_content(&mut self) -> Result<()> {
        let State::Content { content, .. } = std::mem::replace(&mut self.state, State::Header)
        else {
            return Ok(());
        };
        let data = std::mem::take(&mut self.pending);
        match content {
            Content::LongName => {
                let name = data.split(|&byte| byte == 0).next().unwrap_or_default();
                self.next_path = Some(text(name)?);
            }
            Content::Pax => {
                for (key, value) in pax_records(&data)? {
                    match key {
                        "path" => self.next_path = Some(value.to_owned()),
                        "size" => self.next_size = Some(value.parse().map_err(|_| corrupt())?),
                        _ => {}
                    }
                }
            }
            Content::File | Content::Skip => {}
        }
        Ok(())
    }
}

/// The path a header names: ustar's `prefix/name`, or GNU's and old tar's `name`.
fn header_path(header: &[u8]) -> Result<String> {
    let field = |range: std::ops::Range<usize>| {
        let field = &header[range];
        let end = field
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(field.len());
        text(&field[..end])
    };
    let name = field(0..100)?;
    if &header[257..263] == b"ustar\0" {
        let prefix = field(345..500)?;
        if !prefix.is_empty() {
            return Ok(format!("{prefix}/{name}"));
        }
    }
    Ok(name)
}

/// A pax header's `<length> <key>=<value>\n` records.
fn pax_records(data: &[u8]) -> Result<Vec<(&str, &str)>> {
    let mut records = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let space = rest
            .iter()
            .position(|&byte| byte == b' ')
            .ok_or_else(corrupt)?;
        let length: usize = std::str::from_utf8(&rest[..space])
            .ok()
            .and_then(|length| length.parse().ok())
            .ok_or_else(corrupt)?;
        if length <= space + 1 || length > rest.len() || rest[length - 1] != b'\n' {
            return Err(corrupt());
        }
        let record = std::str::from_utf8(&rest[space + 1..length - 1]).map_err(|_| corrupt())?;
        let (key, value) = record.split_once('=').ok_or_else(corrupt)?;
        records.push((key, value));
        rest = &rest[length..];
    }
    Ok(records)
}

/// A header's number: octal digits (space or NUL padded), or GNU's base-256 for large ones.
fn number(field: &[u8]) -> Option<u64> {
    if field.first().is_some_and(|&byte| byte & 0x80 != 0) {
        let mut value: u64 = u64::from(field[0] & 0x7f);
        for &byte in &field[1..] {
            value = value.checked_mul(256)?.checked_add(u64::from(byte))?;
        }
        return Some(value);
    }
    octal(field)
}

fn octal(field: &[u8]) -> Option<u64> {
    let digits = std::str::from_utf8(field)
        .ok()?
        .trim_matches(|c: char| c == ' ' || c == '\0');
    if digits.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(digits, 8).ok()
}

fn text(bytes: &[u8]) -> Result<String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| unsupported())
}

fn corrupt() -> Error {
    Error::new("archive-corrupt")
}

fn unsupported() -> Error {
    Error::new("archive-entry-unsupported")
}
