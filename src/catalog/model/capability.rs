use serde::Deserialize;

/// What a model can do. A model can do several (`capabilities` in the catalogue), and the list grows (`llm`, ...):
/// it is `#[non_exhaustive]`, so match it with a wildcard arm. The catalogue names each by a stable id: "stt", "tts",
/// "vad".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Capability {
    /// Speech to text.
    Stt,
    /// Text to speech.
    Tts,
    /// Voice activity detection: where speech starts and ends in a stream of audio.
    Vad,
}
