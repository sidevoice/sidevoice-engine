//! When each local model was last loaded: one small record in the host's storage, a JSON object of model ids to
//! milliseconds since the Unix epoch, stored as the file `last-used` (beside the blobs, under a name no digest has). The
//! engine notes a model each time it loads it and forgets it once no build of it is installed; the app shows it and
//! may remove what has not been used for a while.
//!
//! The record is the engine's housekeeping, not something a load depends on: noting or forgetting never fails what
//! asked for it. A record that is missing or is not one (malformed) reads as empty, and the next change replaces it
//! whole. A record the storage fails to read is left as it is: the change is skipped, so the times it holds for other
//! models are not lost.
//!
//! Changes are serialised within one engine. Two engines over one storage (two processes on one data directory, two
//! tabs of one origin over its OPFS) are not kept apart: each may overwrite the other's latest change. The native data
//! directory is the engine's alone (`NativeHost`), and the cost of such a race is one stale or missing time, never a
//! failed load.

use std::collections::BTreeMap;

use crate::host::{Host, Storage};
use crate::{Error, Result};

/// The stored record's name.
const NAME: &str = "last-used";

/// When each model was last loaded: its id, and milliseconds since the Unix epoch.
pub(super) type Times = BTreeMap<String, u64>;

/// The record, its changes made one at a time: each takes the time, reads the record, changes it and writes it back
/// whole while it holds the lock, so two loads that finish together both land, and in the order they took the lock.
#[derive(Default)]
pub(super) struct LastUsed {
    writing: async_lock::Mutex<()>,
}

impl LastUsed {
    /// When each model was last loaded, as stored; empty when nothing is, or what is cannot be read.
    pub(super) async fn read(&self, storage: &dyn Storage) -> Times {
        read(storage).await.unwrap_or_default()
    }

    /// Notes that `model` was loaded now, by `host`'s clock, in `host`'s storage. Best effort: a failure leaves the
    /// record as it was.
    pub(super) async fn record(&self, host: &dyn Host, model: &str) {
        let _writing = self.writing.lock().await;
        // Taken once the lock is held: a change that waited for another never writes an earlier time over a later one.
        let at = host.now_ms();
        let Ok(mut times) = read(host.storage()).await else {
            return;
        };
        times.insert(model.to_owned(), at);
        let _ = write(host.storage(), &times).await;
    }

    /// Forgets `model`. Best effort, as [`LastUsed::record`].
    pub(super) async fn forget(&self, storage: &dyn Storage, model: &str) {
        let _writing = self.writing.lock().await;
        let Ok(mut times) = read(storage).await else {
            return;
        };
        if times.remove(model).is_some() {
            let _ = write(storage, &times).await;
        }
    }
}

/// The stored record: empty when there is none or it is not one; what the storage fails with when it cannot read it.
async fn read(storage: &dyn Storage) -> Result<Times> {
    if storage.find(NAME).await?.is_none() {
        return Ok(Times::new());
    }
    let mut stored = storage.read(NAME).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = stored.chunk().await? {
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes).unwrap_or_default())
}

/// Replaces the stored record with `times`, whole: storage only stores it once committed.
async fn write(storage: &dyn Storage, times: &Times) -> Result<()> {
    let bytes = serde_json::to_vec(times).map_err(|_| Error::new("storage-failed"))?;
    let mut writer = storage.create(NAME).await?;
    writer.write(&bytes).await?;
    writer.commit().await.map(drop)
}
