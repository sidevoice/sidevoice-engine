//! Kokoro through sherpa-onnx's offline TTS. v0.19 speaks English only; v1.0 and later are multilingual, and are told
//! each utterance's language as the espeak-ng voice that reads it.

use sherpa_onnx::{OfflineTtsConfig, OfflineTtsKokoroModelConfig, OfflineTtsModelConfig};

use super::synthesizer::{Language, Synthesizer};
use super::{path, primary_language};
use crate::install::Installed;
use crate::Result;

/// Creates the TTS from the `model`, `voices`, `tokens` and `espeak-ng-data` (a directory) in `files`, on `provider`.
/// A multilingual Kokoro has a `lexicon` too (its English one): the library exits the process when it makes one
/// without it, and with it, a language is passed with each utterance. The voices are named by the model's
/// `speaker_names` metadata.
pub(super) fn load(files: &Installed, provider: &str) -> Result<Synthesizer> {
    let model = path(files, "model")?;
    let lexicon = files
        .file("lexicon")
        .map(|_| path(files, "lexicon"))
        .transpose()?;
    let language: Language = if lexicon.is_some() {
        |tag| Some(espeak_voice(tag))
    } else {
        |_| None
    };
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            kokoro: OfflineTtsKokoroModelConfig {
                model: Some(model.clone()),
                voices: Some(path(files, "voices")?),
                tokens: Some(path(files, "tokens")?),
                data_dir: Some(path(files, "espeak-ng-data")?),
                length_scale: 1.0,
                lexicon,
                ..OfflineTtsKokoroModelConfig::default()
            },
            provider: Some(provider.to_owned()),
            ..OfflineTtsModelConfig::default()
        },
        ..OfflineTtsConfig::default()
    };
    Synthesizer::new(config, Some(&model), language)
}

/// The espeak-ng voice that reads the BCP 47 tag `tag`: its primary language, except that English and Portuguese keep
/// their region (`en-gb`, `pt-br`), and English with none is American, as Kokoro's own English is.
pub(super) fn espeak_voice(tag: &str) -> String {
    let lowered = tag.replace('_', "-").to_ascii_lowercase();
    match (primary_language(tag).as_str(), lowered.split('-').nth(1)) {
        ("en", Some("gb")) => "en-gb".to_owned(),
        ("en", _) => "en-us".to_owned(),
        ("pt", Some("br")) => "pt-br".to_owned(),
        (primary, _) => primary.to_owned(),
    }
}
