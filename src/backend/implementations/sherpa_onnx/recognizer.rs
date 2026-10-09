//! Every speech-to-text model through sherpa-onnx's offline recognizer, alike: one whole turn at a time, with the
//! recognizer its config makes. Nothing here knows one model from another.
//!
//! A call's language reaches the model only where its build maps it into the config (`call_params`,
//! sidevoice-engine#46): Whisper's `whisper.language`, Canary's `canary.src_lang` and `canary.tgt_lang`. A live
//! recognizer cannot change its config, so a call in another language makes another recognizer, which reads the weights
//! again; the last [`KEPT`] made are kept, so a conversation that goes back and forth between two languages, or between
//! one and detecting it, reloads nothing (sidevoice-engine#42). Each recognizer kept holds its own copy of the weights.
//! Without a language, or for a build that maps none, the config is the build's own: Whisper detects the language.

use std::collections::BTreeMap;

use async_trait::async_trait;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig};

use super::config;
use crate::backend::loaded_model::SttModel;
use crate::backend::BackendModel;
use crate::{Error, Result};

/// The sample rate [`SttModel::transcribe`] takes (the engine resamples to it).
const SAMPLE_RATE: i32 = 16_000;

/// How many recognizers, one per language a call asked for, a model keeps.
pub(super) const KEPT: usize = 2;

/// The argument of a call a build's `call_params` may map: the language.
const LANGUAGE: &str = "language";

/// A speech-to-text model in memory: its config, where a call's language goes in it, and the recognizers made, the
/// most recently used first.
pub(super) struct Recognizer {
    config: OfflineRecognizerConfig,
    call_params: BTreeMap<String, Vec<String>>,
    made: Vec<(Option<String>, OfflineRecognizer)>,
}

impl Recognizer {
    /// Creates the recognizer from `config`, the build's files in it (`config.rs`), with no language set; a call's
    /// language goes where `call_params` says. Fails with `unsupported-model` for a path of `call_params` no field a
    /// call may set has, and `model-load-failed`.
    pub(super) fn load(
        config: OfflineRecognizerConfig,
        call_params: &BTreeMap<String, Vec<String>>,
    ) -> Result<Self> {
        let mut checked = config.clone();
        for argument in call_params.keys() {
            config::set_call(&mut checked, call_params, argument, None)?;
        }
        let recognizer = create(&config)?;
        Ok(Self {
            config,
            call_params: call_params.clone(),
            made: vec![(None, recognizer)],
        })
    }

    /// The recognizer for `language`, made if no kept one is.
    fn for_language(&mut self, language: Option<&str>) -> Result<&OfflineRecognizer> {
        let language = language
            .filter(|_| self.call_params.contains_key(LANGUAGE))
            .map(primary_subtag);
        let (config, call_params) = (&self.config, &self.call_params);
        recent(&mut self.made, language, |language| {
            let mut config = config.clone();
            config::set_call(&mut config, call_params, LANGUAGE, language.as_deref())?;
            create(&config)
        })
    }
}

/// The primary subtag of the BCP 47 tag `tag`, lowercased: what Whisper and Canary name a language by (`es`).
pub(super) fn primary_subtag(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// The value kept under `key` in `made`, moved to the front, or made by `make` and put there; past [`KEPT`], the least
/// recently used goes.
pub(super) fn recent<K: PartialEq, V>(
    made: &mut Vec<(K, V)>,
    key: K,
    make: impl FnOnce(&K) -> Result<V>,
) -> Result<&V> {
    match made.iter().position(|(kept, _)| *kept == key) {
        Some(at) => {
            let found = made.remove(at);
            made.insert(0, found);
        }
        None => {
            let value = make(&key)?;
            made.insert(0, (key, value));
            made.truncate(KEPT);
        }
    }
    Ok(&made[0].1)
}

fn create(config: &OfflineRecognizerConfig) -> Result<OfflineRecognizer> {
    OfflineRecognizer::create(config).ok_or(Error::new("model-load-failed"))
}

impl BackendModel for Recognizer {
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl SttModel for Recognizer {
    /// Decodes on the calling thread: the engine decides where to run it. `language` goes where the build maps it (see
    /// the module's docs), as its primary subtag.
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String> {
        let recognizer = self.for_language(language)?;
        let stream = recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE, pcm);
        recognizer.decode(&stream);
        let result = stream
            .get_result()
            .ok_or(Error::new("transcription-failed"))?;
        Ok(result.text.trim().to_owned())
    }
}
