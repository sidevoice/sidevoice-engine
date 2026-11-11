use serde::Deserialize;

use crate::host::AcceleratorClass;

/// The models the engine recommends for speech to text and for text to speech, best first: a hand-written list per
/// capability, which the recommender walks (`crate::recommender`). Measurements on each kind of machine
/// (sidevoice-engine#155) are to re-order the lists, not the code. A model the lists leave out can still be chosen;
/// it is only never recommended.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecommendedLists {
    /// Where the order comes from: who decided it, on what evidence.
    #[serde(default)]
    pub basis: String,
    /// Speech to text, best first.
    pub stt: Vec<ListEntry>,
    /// Text to speech, best first.
    pub tts: Vec<ListEntry>,
}

/// One place of a list: a model of the catalogue, and the class of hardware its recommended build must run on here
/// for this place to count (Whisper large-v3 turbo is comfortable on a GPU only); absent, any.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListEntry {
    /// The model's id.
    pub model: String,
    /// The class its build must run on here: "gpu", "neural-engine" or "cpu".
    #[serde(default)]
    pub on: Option<AcceleratorClass>,
}
