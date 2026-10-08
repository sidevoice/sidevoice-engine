//! A value of an ONNX model's metadata (`ModelProto.metadata_props`), which sherpa-onnx's C API does not expose:
//! Kokoro's voice names are there. A model file is a protobuf `ModelProto`; this reads its top-level fields in order,
//! skipping each one (the graph, most of the file, with a seek) until a metadata entry whose key is the one asked for.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// `ModelProto.metadata_props`, a repeated `StringStringEntryProto { key = 1; value = 2; }`.
const METADATA_PROPS: u64 = 14;
/// Larger metadata entries are skipped rather than read: names lists are short.
const MAX_ENTRY: u64 = 1 << 20;

/// The value of `key` in the metadata of the ONNX model at `path`, if it has one and the file reads as a model.
pub(super) fn read(path: &Path, key: &str) -> Option<String> {
    find(&mut BufReader::new(File::open(path).ok()?), key)
}

/// The value of `key` among the top-level metadata entries of the protobuf `ModelProto` in `model`.
pub(super) fn find(model: &mut BufReader<impl Read + std::io::Seek>, key: &str) -> Option<String> {
    loop {
        let tag = varint(model)?;
        match tag & 7 {
            0 => {
                varint(model)?;
            }
            1 => model.seek_relative(8).ok()?,
            5 => model.seek_relative(4).ok()?,
            2 => {
                let len = varint(model)?;
                if tag >> 3 == METADATA_PROPS && len <= MAX_ENTRY {
                    let mut entry = vec![0; usize::try_from(len).ok()?];
                    model.read_exact(&mut entry).ok()?;
                    if let Some(value) = entry_value(&entry, key) {
                        return Some(value);
                    }
                } else {
                    model.seek_relative(i64::try_from(len).ok()?).ok()?;
                }
            }
            _ => return None,
        }
    }
}

/// The value of a `StringStringEntryProto` whose key is `key`.
fn entry_value(mut entry: &[u8], key: &str) -> Option<String> {
    let (mut found_key, mut value) = (None, None);
    while !entry.is_empty() {
        let tag = varint(&mut entry)?;
        if tag & 7 != 2 {
            return None;
        }
        let len = usize::try_from(varint(&mut entry)?).ok()?;
        let bytes = entry.get(..len)?;
        entry = &entry[len..];
        match tag >> 3 {
            1 => found_key = Some(bytes),
            2 => value = Some(bytes),
            _ => {}
        }
    }
    if found_key? != key.as_bytes() {
        return None;
    }
    String::from_utf8(value?.to_vec()).ok()
}

/// A protobuf varint; `None` at the end of the input or on a malformed one.
fn varint(input: &mut impl Read) -> Option<u64> {
    let mut value = 0;
    for shift in (0..64).step_by(7) {
        let mut byte = [0];
        input.read_exact(&mut byte).ok()?;
        value |= u64::from(byte[0] & 0x7f) << shift;
        if byte[0] & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}
