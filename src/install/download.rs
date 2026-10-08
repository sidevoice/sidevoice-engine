//! One file downloaded into storage: fetched through the host, hashed as its bytes arrive, written as they come, and
//! committed under its digest only if it is the file expected. Anything else (a mismatch, a failed download, a cancel,
//! the future dropped) leaves nothing stored: the writer is dropped uncommitted.

use super::digest::Hasher;
use super::{Cancel, Progress, ProgressSink};
use crate::host::Host;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// Downloads `url` into storage as `sha256`, which its bytes must hash to (`digest-mismatch` otherwise). `report` is
/// the progress so far, before this file; `progress` hears of each part, and `cancel` is checked between parts.
pub(super) async fn download(
    url: &str,
    sha256: &str,
    host: &dyn Host,
    progress: &dyn ProgressSink,
    cancel: &Cancel,
    mut report: Progress,
) -> Result<()> {
    let mut download = host.fetcher().fetch(url).await?;
    let mut file = host.storage().create(sha256).await?;
    let mut hasher = Hasher::default();
    report.size = download.size();
    progress.progress(report);
    while let Some(bytes) = download.chunk().await? {
        // Returning drops `file` uncommitted: nothing is stored.
        cancel.check()?;
        hasher.update(&bytes);
        file.write(&bytes).await?;
        report.received += bytes.len() as u64;
        progress.progress(report);
    }
    if hasher.finish() != sha256 {
        return Err(Error::new("digest-mismatch"));
    }
    file.commit().await.map(drop)
}
