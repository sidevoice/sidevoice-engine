//! The providers compiled into this build. Each provider submits its factory with `inventory::submit!`, as backends
//! do (`backend/registry.rs`, which also says why the tests that count them are unit tests). Nobody keeps a list.

use crate::provider::Adapter;

/// Makes a provider's adapter: an empty object, which calls nothing until asked.
pub(crate) struct ProviderFactory(pub(crate) fn() -> Box<dyn Adapter>);

inventory::collect!(ProviderFactory);

/// Every provider compiled into this build, by id.
#[must_use]
pub(crate) fn built_in() -> Vec<Box<dyn Adapter>> {
    let mut providers: Vec<_> = inventory::iter::<ProviderFactory>
        .into_iter()
        .map(|factory| (factory.0)())
        .collect();
    providers.sort_by_key(|provider| provider.spec().id);
    providers
}
