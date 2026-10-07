/// What a model is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Task {
    /// Speech to text.
    Stt,
    /// Text to speech.
    Tts,
}
