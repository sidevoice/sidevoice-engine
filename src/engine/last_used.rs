//! When each local model was last loaded: one small record in the host's storage, a JSON object of model ids to
//! milliseconds since the Unix epoch, stored as the file `last-used` (beside the blobs, under a name no digest has). The
//! engine notes a model each time it loads it and forgets it once no build of it is installed; the app shows it and
//! may remove what has not been used for a while.
//!
//! The record is the engine's housekeeping, not something a load depends on: a record that cannot be written is left
//! as it was, and one that is missing or cannot be read reads as empty (and is replaced whole by the next write).

use std::collections::BTreeMap;

use crate::host::Storage;
use crate::{Error, Result};

/// The stored record's name.
const NAME: &str = "last-used";

/// When each model was last loaded: its id, and milliseconds since the Unix epoch.
pub(super) type Times = BTreeMap<String, u64>;

/// The record, its changes made one at a time: each reads it, changes it and writes it back whole while it holds the
/// lock, so two loads that finish together both land.
#[derive(Default)]
pub(super) struct LastUsed {
    writing: async_lock::Mutex<()>,
}

impl LastUsed {
    /// When each model was last loaded, as stored; empty when nothing is, or what is cannot be read.
    pub(super) async fn read(&self, storage: &dyn Storage) -> Times {
        read(storage).await.ok().flatten().unwrap_or_default()
    }

    /// Notes that `model` was loaded at `at`. Best effort: a failure leaves the record as it was.
    pub(super) async fn record(&self, storage: &dyn Storage, model: &str, at: u64) {
        let _writing = self.writing.lock().await;
        let mut times = self.read(storage).await;
        times.insert(model.to_owned(), at);
        let _ = write(storage, &times).await;
    }

    /// Forgets `model`. Best effort, as [`LastUsed::record`].
    pub(super) async fn forget(&self, storage: &dyn Storage, model: &str) {
        let _writing = self.writing.lock().await;
        let mut times = self.read(storage).await;
        if times.remove(model).is_some() {
            let _ = write(storage, &times).await;
        }
    }
}

/// The stored record, `None` when there is none or it is not one.
async fn read(storage: &dyn Storage) -> Result<Option<Times>> {
    if storage.find(NAME).await?.is_none() {
        return Ok(None);
    }
    let mut stored = storage.read(NAME).await?;
    let mut bytes = Vec::new();
    while let Some(chunk) = stored.chunk().await? {
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes).ok())
}

/// Replaces the stored record with `times`, whole: storage only stores it once committed.
async fn write(storage: &dyn Storage, times: &Times) -> Result<()> {
    let bytes = serde_json::to_vec(times).map_err(|_| Error::new("storage-failed"))?;
    let mut writer = storage.create(NAME).await?;
    writer.write(&bytes).await?;
    writer.commit().await.map(drop)
}
