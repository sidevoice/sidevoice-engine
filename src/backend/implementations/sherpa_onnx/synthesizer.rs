//! Every text-to-speech model through sherpa-onnx's offline TTS, alike: the TTS its config makes, its voices named,
//! and a call's language told to it. Nothing here knows one model from another.
//!
//! A call's language reaches the model as the generation's extra option `lang`, which sherpa-onnx prefers to its
//! config's `lang` and to the model's own (`offline-tts-kokoro-impl.h`, `offline-tts-supertonic-impl.cc`), and which a
//! model that takes none ignores (VITS). Where the config has espeak-ng's data (a `*.data_dir` key), it is the
//! espeak-ng voice that reads the text: the BCP 47 tag lowercased (`en-us`, `pt-br`) where espeak-ng has a voice of
//! that name, its primary subtag (`es`) otherwise. A tag without a region takes the region of the chosen voice when
//! the catalogue declares the voice in that language with one (`en` and a voice declared `en-US` read as `en-us`).
//! Without espeak-ng's data, it is the primary subtag.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use async_trait::async_trait;
use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig};

use super::{model_metadata, text};
use crate::backend::loaded_model::TtsModel;
use crate::backend::BackendModel;
use crate::catalog::Voice;
use crate::install::Installed;
use crate::{Error, Result};

/// A TTS model in memory: the TTS, its sample rate, its voices by speaker id, and the espeak-ng voices its data has
/// (none without espeak-ng's data).
pub(super) struct Synthesizer {
    tts: OfflineTts,
    sample_rate: u32,
    voices: Vec<String>,
    espeak: BTreeSet<String>,
}

impl Synthesizer {
    /// Creates the TTS from `config`, the build's `files` in it (`config.rs`). Its voices are named by the
    /// `speaker_names` metadata of the file of a `*.model` key, when it names as many as the model has speakers;
    /// otherwise by speaker id, `"0"`, `"1"`, .... Fails with `model-load-failed`.
    pub(super) fn load(config: &OfflineTtsConfig, files: &Installed) -> Result<Self> {
        let failed = || Error::new("model-load-failed");
        let tts = OfflineTts::create(config).ok_or_else(failed)?;
        let sample_rate = u32::try_from(tts.sample_rate()).map_err(|_| failed())?;
        let speakers = usize::try_from(tts.num_speakers()).unwrap_or_default();
        let named = key_ending(files, ".model")
            .and_then(|model| model_metadata::read(Path::new(model), "speaker_names"))
            .map(|names| names.split(',').map(str::to_owned).collect::<Vec<_>>())
            .filter(|names| names.len() == speakers);
        let voices = named.unwrap_or_else(|| (0..speakers).map(|id| id.to_string()).collect());
        let espeak = key_ending(files, ".data_dir")
            .map(|dir| espeak_voices(Path::new(dir)))
            .unwrap_or_default();
        Ok(Self {
            tts,
            sample_rate,
            voices,
            espeak,
        })
    }
}

/// The installed file of the first key that ends in `suffix`.
fn key_ending<'a>(files: &'a Installed, suffix: &str) -> Option<&'a str> {
    files
        .files
        .iter()
        .find(|(key, _)| key.ends_with(suffix))
        .map(|(_, path)| path.as_str())
}

/// The names of the voices espeak-ng's data in `data_dir` has (its `lang/` files, `lang/roa/es-419`), lowercased.
fn espeak_voices(data_dir: &Path) -> BTreeSet<String> {
    fn walk(dir: &Path, names: &mut BTreeSet<String>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, names);
            } else {
                names.insert(entry.file_name().to_string_lossy().to_ascii_lowercase());
            }
        }
    }
    let mut names = BTreeSet::new();
    walk(&data_dir.join("lang"), &mut names);
    names
}

/// The espeak-ng voice that reads the BCP 47 tag `tag` spoken with a voice declared in `declared`, among `voices`.
/// A `tag` without a region takes the first declared language with its primary subtag and a region (`en` and
/// `en-US` → `en-US`); then the tag lowercased if `voices` has one of that name, its primary subtag otherwise.
pub(super) fn espeak_voice(tag: &str, declared: &[String], voices: &BTreeSet<String>) -> String {
    let lowered = tag.replace('_', "-").to_ascii_lowercase();
    let regional = |language: &String| {
        let language = language.replace('_', "-").to_ascii_lowercase();
        let (primary, region) = language.split_once('-')?;
        (primary == lowered && !region.is_empty()).then_some(language)
    };
    let lowered = if lowered.contains('-') {
        lowered
    } else {
        declared.iter().find_map(regional).unwrap_or(lowered)
    };
    if voices.contains(&lowered) {
        return lowered;
    }
    lowered.split('-').next().unwrap_or_default().to_owned()
}

impl BackendModel for Synthesizer {
    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl TtsModel for Synthesizer {
    fn voices(&self) -> Vec<String> {
        self.voices.clone()
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Synthesises on the calling thread: the engine decides where to run it. `voice` is one of
    /// [`TtsModel::voices`], or fails with `unknown-voice`.
    async fn speak(
        &mut self,
        words: &str,
        voice: &Voice,
        language: Option<&str>,
        speed: f32,
    ) -> Result<Vec<f32>> {
        let failed = || Error::new("speech-failed");
        let sid = self
            .voices
            .iter()
            .position(|name| *name == voice.id)
            .ok_or(Error::new("unknown-voice"))?;
        let lang = language.map(|tag| espeak_voice(tag, &voice.languages, &self.espeak));
        let extra =
            lang.map(|lang| HashMap::from([("lang".to_owned(), serde_json::Value::String(lang))]));
        let config = GenerationConfig {
            sid: i32::try_from(sid).map_err(|_| failed())?,
            speed,
            extra,
            ..GenerationConfig::default()
        };
        let audio = self
            .tts
            .generate_with_config(&text(words)?, &config, None::<fn(&[f32], f32) -> bool>)
            .ok_or_else(failed)?;
        Ok(audio.samples().to_vec())
    }
}
