//! Kokoro through sherpa-onnx's offline TTS. v0.19 speaks English only; v1.0 and later are multilingual, and are told
//! each utterance's language as the espeak-ng voice that reads it.

use std::path::Path;
use std::sync::Arc;

use super::c_api::OfflineTtsConfig;
use super::library::Api;
use super::synthesizer::Synthesizer;
use super::{c_string, path, primary_language};
use crate::install::Installed;
use crate::Result;

/// Creates the TTS from the `model`, `voices`, `tokens` and `espeak-ng-data` (a directory) in `files`, on `provider`.
/// A multilingual Kokoro has a `lexicon` too (its English one): the library refuses to make one without it, and with
/// it, a language is passed with each utterance. The voices are named by the model's `speaker_names` metadata.
pub(super) fn load(api: Arc<Api>, files: &Installed, provider: &str) -> Result<Synthesizer> {
    let model_path = path(files, "model")?;
    let model = c_string(model_path)?;
    let voices = c_string(path(files, "voices")?)?;
    let tokens = c_string(path(files, "tokens")?)?;
    let data_dir = c_string(path(files, "espeak-ng-data")?)?;
    let lexicon = files.file("lexicon").map(c_string).transpose()?;
    let provider = c_string(provider)?;
    let mut config = OfflineTtsConfig::default();
    config.model.kokoro.model = model.as_ptr();
    config.model.kokoro.voices = voices.as_ptr();
    config.model.kokoro.tokens = tokens.as_ptr();
    config.model.kokoro.data_dir = data_dir.as_ptr();
    config.model.kokoro.length_scale = 1.0;
    config.model.provider = provider.as_ptr();
    let language: fn(&str) -> Option<String> = match &lexicon {
        Some(lexicon) => {
            config.model.kokoro.lexicon = lexicon.as_ptr();
            |tag| Some(espeak_voice(tag))
        }
        None => |_| None,
    };
    Synthesizer::new(api, &mut config, Some(Path::new(model_path)), language)
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
