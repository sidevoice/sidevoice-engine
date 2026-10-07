use crate::{async_trait, Error, Fetcher, Result, Storage};

/// The storage and downloads of a JavaScript host, not bridged yet: every call fails with `not-implemented`.
pub(super) struct Unimplemented;

#[async_trait(?Send)]
impl Storage for Unimplemented {
    async fn contains(&self, _key: &str) -> Result<bool> {
        Err(Error::new("not-implemented"))
    }
}

#[async_trait(?Send)]
impl Fetcher for Unimplemented {
    async fn fetch(&self, _url: &str, _sha256: &str, _key: &str) -> Result<()> {
        Err(Error::new("not-implemented"))
    }
}
