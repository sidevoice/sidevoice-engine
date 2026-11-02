//! One file downloaded into storage: fetched through the host, hashed as its bytes arrive, written as they come, and
//! committed under its digest only if it is the file expected. A download that stops half way (a failure,
//! the future dropped) stores nothing, but the host may keep what arrived ([`Storage::resume`]): the next attempt asks
//! the server for the rest ([`Fetcher::fetch_from`]), hashes the bytes kept first, and goes on, so the digest is
//! always checked over the whole file. A server that sends the whole file anyway, or bytes kept that turn out wrong,
//! start it over. A cancel removes what arrived: it is the person's, and the install it was part of is cancelled.

use super::digest::Hasher;
use super::{Cancel, Progress, ProgressSink};
use crate::host::{Download, Host, Resumed};
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
    report: Progress,
) -> Result<()> {
    let kept = host.storage().resume(sha256).await?;
    if transfer(url, sha256, host, progress, cancel, report, kept).await? {
        return Ok(());
    }
    // The bytes kept were not the start of this file, and are gone: once more, from nothing.
    let fresh = host.storage().resume(sha256).await?;
    if transfer(url, sha256, host, progress, cancel, report, fresh).await? {
        Ok(())
    } else {
        Err(Error::new("digest-mismatch"))
    }
}

/// One attempt, going on after the bytes `kept` holds if the server sends the rest. `Ok(true)` once the file is stored;
/// `Ok(false)` if it was not, because it hashed wrongly after bytes kept from an earlier attempt (which are discarded);
/// `digest-mismatch` if it hashed wrongly all the same, which no second attempt would change.
async fn transfer(
    url: &str,
    sha256: &str,
    host: &dyn Host,
    progress: &dyn ProgressSink,
    cancel: &Cancel,
    mut report: Progress,
    kept: Resumed,
) -> Result<bool> {
    let Resumed {
        mut writer,
        kept,
        len,
    } = kept;
    let mut download = if len > 0 {
        host.fetcher().fetch_from(url, len).await?
    } else {
        host.fetcher().fetch(url).await?
    };
    let mut hasher = Hasher::default();
    let offset = download.offset();
    if offset != 0 && offset != len {
        return Err(Error::new("download-failed"));
    }
    if offset > 0 {
        let hashed = match hash_kept(kept, &mut hasher, cancel).await {
            Ok(hashed) => hashed,
            Err(error) => {
                if error.code == "cancelled" {
                    writer.discard().await?;
                }
                return Err(error);
            }
        };
        if hashed != len {
            // The bytes kept are not all there.
            writer.discard().await?;
            return Ok(false);
        }
    } else if len > 0 {
        // The server sends the whole file: what was kept goes.
        drop(kept);
        writer.restart().await?;
    }
    report.received += offset;
    report.size = download.size().map(|size| size + offset);
    progress.progress(report);
    let streamed = stream(
        &mut *download,
        &mut *writer,
        &mut hasher,
        progress,
        cancel,
        &mut report,
    )
    .await;
    if let Err(error) = streamed {
        // A cancel is the person's: what arrived goes with it. Any other failure (no network, the app suspended) keeps
        // it for the next attempt.
        if error.code == "cancelled" {
            writer.discard().await?;
        }
        return Err(error);
    }
    if hasher.finish() != sha256 {
        writer.discard().await?;
        return if offset > 0 {
            Ok(false)
        } else {
            Err(Error::new("digest-mismatch"))
        };
    }
    writer.commit().await?;
    Ok(true)
}

/// Feeds the bytes kept to `hasher`; how many there were.
async fn hash_kept(
    kept: Option<Box<dyn Download>>,
    hasher: &mut Hasher,
    cancel: &Cancel,
) -> Result<u64> {
    let mut total = 0;
    if let Some(mut kept) = kept {
        while let Some(bytes) = kept.chunk().await? {
            cancel.check()?;
            hasher.update(&bytes);
            total += bytes.len() as u64;
        }
    }
    Ok(total)
}

/// Hashes and writes what arrives. Returning early leaves the writer to its drop: nothing is stored.
async fn stream(
    download: &mut dyn Download,
    file: &mut dyn crate::StorageWriter,
    hasher: &mut Hasher,
    progress: &dyn ProgressSink,
    cancel: &Cancel,
    report: &mut Progress,
) -> Result<()> {
    while let Some(bytes) = download.chunk().await? {
        cancel.check()?;
        hasher.update(&bytes);
        file.write(&bytes).await?;
        report.received += bytes.len() as u64;
        progress.progress(*report);
    }
    Ok(())
}
