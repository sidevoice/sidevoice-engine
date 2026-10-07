use crate::{async_trait, Download, Error, Fetcher, Result};

/// The JavaScript host's downloads, as the engine sees them. Not bridged yet (#8): every call fails with
/// `not-implemented`.
pub(super) struct WebFetcher;

#[async_trait(?Send)]
impl Fetcher for WebFetcher {
    async fn fetch(&self, _url: &str) -> Result<Box<dyn Download>> {
        Err(Error::new("not-implemented"))
    }
}
