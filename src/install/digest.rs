//! SHA-256, as artifacts write it: 64 lowercase hex digits.

use sha2::Digest;

/// Whether `digest` is a SHA-256 as artifacts write it: 64 lowercase hex digits. It is also a storage name, so
/// nothing else may pass.
pub(super) fn is_valid(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The SHA-256 of bytes fed in parts.
#[derive(Default)]
pub(super) struct Hasher(sha2::Sha256);

impl Hasher {
    /// Feeds the next part.
    pub(super) fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// The digest of everything fed, in lowercase hex.
    pub(super) fn finish(self) -> String {
        self.0
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}
