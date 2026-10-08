use crate::{async_trait, Download, Error, Result, Storage, StorageWriter};

/// The JavaScript host's storage (OPFS, IndexedDB, ...), as the engine sees it. Not bridged yet (#8): every call fails
/// with `not-implemented`.
pub(super) struct WebStorage;

#[async_trait(?Send)]
impl Storage for WebStorage {
    async fn find(&self, _name: &str) -> Result<Option<String>> {
        Err(Error::new("not-implemented"))
    }

    async fn find_member(&self, _tree: &str, _path: &str) -> Result<Option<String>> {
        Err(Error::new("not-implemented"))
    }

    async fn create(&self, _name: &str) -> Result<Box<dyn StorageWriter>> {
        Err(Error::new("not-implemented"))
    }

    async fn read(&self, _name: &str) -> Result<Box<dyn Download>> {
        Err(Error::new("not-implemented"))
    }

    async fn remove(&self, _name: &str) -> Result<()> {
        Err(Error::new("not-implemented"))
    }
}
