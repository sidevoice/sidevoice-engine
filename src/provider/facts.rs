//! What a provider's API does not say about its models, as `cargo xtask pin-providers` derived it from the provider's
//! official OpenAPI spec into the backend's `facts.json`: which models are speech to text and which text to speech,
//! the request field a call's language goes in, the speed range, and the voices when the spec fixes them. Nobody
//! writes it by hand, and it is read strictly: an unknown or a missing key is an error.

use serde::Deserialize;

use crate::catalog::Capability;

/// One provider's facts.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Facts {
    /// The spec they were derived from, and its SHA-256: provenance, which the drift check reads.
    #[allow(
        dead_code,
        reason = "provenance, read by `cargo xtask pin-providers --check`"
    )]
    source: String,
    #[allow(
        dead_code,
        reason = "provenance, read by `cargo xtask pin-providers --check`"
    )]
    sha256: String,
    /// The speech-to-text models.
    speech_to_text: Vec<ModelFacts>,
    /// The text-to-speech models.
    text_to_speech: Vec<ModelFacts>,
    /// The voices every text-to-speech model of the provider takes, when the spec fixes them (OpenAI's); empty for a
    /// provider whose voices are the account's, listed live (ElevenLabs').
    pub(crate) voices: Vec<String>,
}

/// One model's facts.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelFacts {
    /// The provider's id of the model.
    pub(crate) model: String,
    /// The request field a call's language goes in; `None` when the model's request takes none.
    pub(crate) language: Option<String>,
    /// The lowest and the highest speed it takes; `None` when its request takes none.
    pub(crate) speed: Option<[f32; 2]>,
}

impl Facts {
    /// `json`, a backend's compiled-in `facts.json`, which its tests read too: a file that does not parse is a bug
    /// of the build, never of the device.
    pub(crate) fn parse(json: &str) -> Self {
        serde_json::from_str(json).expect("a provider's facts.json, as pin-providers writes it")
    }

    /// The model `id`, and what it does, when the spec describes it.
    pub(crate) fn model(&self, id: &str) -> Option<(Capability, &ModelFacts)> {
        self.models().find(|(_, facts)| facts.model == id)
    }

    /// Every model the spec describes, speech to text first.
    pub(crate) fn models(&self) -> impl Iterator<Item = (Capability, &ModelFacts)> {
        let stt = self
            .speech_to_text
            .iter()
            .map(|facts| (Capability::Stt, facts));
        let tts = self
            .text_to_speech
            .iter()
            .map(|facts| (Capability::Tts, facts));
        stt.chain(tts)
    }
}

impl ModelFacts {
    /// `speed` within the model's range; `None` for a model that takes no speed.
    pub(crate) fn speed(&self, speed: f32) -> Option<f32> {
        self.speed.map(|[low, high]| speed.clamp(low, high))
    }
}
