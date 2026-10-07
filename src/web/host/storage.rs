use crate::{async_trait, Error, Result, Storage};

/// The JavaScript host's storage (OPFS, IndexedDB, ...), as the engine sees it. Not bridged yet (#8): every call fails
/// with `not-implemented`.
pub(super) struct WebStorage;

#[async_trait(?Send)]
impl Storage for WebStorage {
    async fn contains(&self, _key: &str) -> Result<bool> {
        Err(Error::new("not-implemented"))
    }
}
