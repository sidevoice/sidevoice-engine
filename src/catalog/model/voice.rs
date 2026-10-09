//! A voice of a text-to-speech model, as its source declares it: its id, the languages it speaks, and its gender when
//! the source states one. Nothing here is guessed: a voice the source does not describe is not in the catalogue, and
//! a gender the source does not state is absent.

use serde::Deserialize;

/// One voice: in the catalogue as its source declares it, and what a loaded model's [`Tts::voices`](crate::Tts::voices)
/// returns.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Voice {
    /// What the backend or the provider calls it, and what `speak` takes: "af_bella", "ef_dora", "0", ....
    pub id: String,
    /// Its name, as a person reads it, when its source gives one (an ElevenLabs voice's: "Rachel").
    #[serde(default)]
    pub name: Option<String>,
    /// The languages it speaks: BCP 47 tags.
    pub languages: Vec<String>,
    /// Its gender, only when the source states it.
    #[serde(default)]
    pub gender: Option<Gender>,
}

/// A voice's gender, as a source states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Gender {
    /// A female voice.
    Female,
    /// A male voice.
    Male,
}
