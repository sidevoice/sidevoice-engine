use serde::Deserialize;

use super::ModelEntry;

/// A family of models one loader runs (whisper, kokoro, ...), with its models. It has no display name: the app
/// translates what it shows from the id.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    /// Its stable id: "whisper", "kokoro", ... A bundled family's file is named after it.
    pub id: String,
    /// The loader a backend runs its models with: "whisper", "kokoro", ...
    pub architecture: String,
    /// Where the family comes from: its original publisher's page.
    pub source: String,
    /// Its models.
    pub models: Vec<ModelEntry>,
}
