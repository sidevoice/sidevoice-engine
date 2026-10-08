//! Supertonic through sherpa-onnx's offline TTS: multilingual without espeak-ng, told each utterance's language by its
//! primary subtag (`es`, `en`, ...; English when it is told none). Its voices are the styles in its voice file, by
//! speaker id.

use sherpa_onnx::{OfflineTtsConfig, OfflineTtsModelConfig, OfflineTtsSupertonicModelConfig};

use super::synthesizer::Synthesizer;
use super::{path, primary_language};
use crate::install::Installed;
use crate::{Error, Result};

/// Creates the TTS from the `duration_predictor`, `text_encoder`, `vector_estimator`, `vocoder`, `tts_json`,
/// `unicode_indexer` and `voice_style` in `files`, on `provider`. sherpa-onnx exits the process when the indexer's
/// path does not end in `.bin` (a check of its C++ core, so linking the crate changes nothing), so such a path is
/// refused first (`model-load-failed`); the catalogue takes Supertonic from its archive, whose members keep their
/// names, rather than as files stored by digest.
pub(super) fn load(files: &Installed, provider: &str) -> Result<Synthesizer> {
    let indexer = path(files, "unicode_indexer")?;
    if !indexer_named_as_required(&indexer) {
        return Err(Error::new("model-load-failed"));
    }
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            supertonic: OfflineTtsSupertonicModelConfig {
                duration_predictor: Some(path(files, "duration_predictor")?),
                text_encoder: Some(path(files, "text_encoder")?),
                vector_estimator: Some(path(files, "vector_estimator")?),
                vocoder: Some(path(files, "vocoder")?),
                tts_json: Some(path(files, "tts_json")?),
                unicode_indexer: Some(indexer),
                voice_style: Some(path(files, "voice_style")?),
            },
            provider: Some(provider.to_owned()),
            ..OfflineTtsModelConfig::default()
        },
        ..OfflineTtsConfig::default()
    };
    Synthesizer::new(config, None, |tag| Some(primary_language(tag)))
}

/// Whether the library takes `path` as the unicode indexer: it must end in `.bin`.
pub(super) fn indexer_named_as_required(path: &str) -> bool {
    path.ends_with(".bin")
}
