//! The catalogue this repository ships: one file per family, `catalog/families/<family>.json` at the repository root,
//! compiled in. Its files' URLs, sizes and digests, and its estimated memory, are written by `cargo xtask
//! pin-catalog` from the Hugging Face and GitHub APIs, never by hand. The tests parse every file on every target,
//! check the merge, and check that each file's id is its name.

use super::{CatalogFragment, CatalogSource, Family};
use crate::{Error, Result};

/// Every bundled family: the name of its file, and the file. Listed by `build.rs`, from `catalog/families/` itself:
/// adding a family is adding its file.
const FAMILIES: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/bundled_families.rs"));

/// The catalogue bundled in the engine. Not included by default: whoever builds an [`Engine`](crate::Engine) passes
/// it among its sources.
#[derive(Debug, Clone, Copy, Default)]
pub struct BundledCatalog;

impl CatalogSource for BundledCatalog {
    /// # Errors
    ///
    /// `bundled-catalog-invalid` if a family file does not parse, which the tests rule out.
    fn load(&self) -> Result<CatalogFragment> {
        let families = FAMILIES
            .iter()
            .map(|(name, json)| parse(name, json))
            .collect::<Result<_, _>>()
            .map_err(|_| Error::new("bundled-catalog-invalid"))?;
        Ok(CatalogFragment { families })
    }
}

/// The family in the file `<name>.json`, or why it is not one: what serde says, or an id that is not its file's name.
fn parse(name: &str, json: &str) -> Result<Family, String> {
    let family: Family =
        serde_json::from_str(json).map_err(|error| format!("{name}.json: {error}"))?;
    if family.id != name {
        return Err(format!("{name}.json: its id is {:?}", family.id));
    }
    Ok(family)
}

#[cfg(test)]
mod tests;
