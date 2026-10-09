//! Kokoro, through transformers.js's `StyleTextToSpeech2Model` and its tokenizer, with phonemes from eSpeak NG
//! (`phonemes.rs`). A voice is a table of style vectors, one per utterance length in tokens; the voice's id says its
//! language by its first letter, as Kokoro names them (`ef_dora`: Spanish, female).

use std::collections::BTreeMap;

use js_sys::{Float32Array, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use super::phonemes::Espeak;
use super::{call, call_method, dispose, failed, finite, object, voices, Model, VOICES};
use crate::backend::loaded_model::TtsModel;
use crate::backend::BackendModel;
use crate::catalog::Voice;
use crate::install::Installed;
use crate::web::opfs;
use crate::{Error, Result};

/// Kokoro speaks at 24 kHz, whatever the voice.
const SAMPLE_RATE: u32 = 24_000;
/// The width of a style vector.
const STYLE: usize = 256;
/// The most tokens Kokoro reads at once, its two boundary tokens left out.
const MAX_TOKENS: usize = 509;

/// The eSpeak NG voice that phonemizes for a Kokoro voice, by the first letter of its id (Misaki's language codes):
/// `None` for a language eSpeak NG does not phonemize for Kokoro (Japanese and Mandarin need Misaki's own).
fn espeak_voice(voice: &str) -> Option<&'static str> {
    Some(match voice.chars().next()? {
        'a' => "en-us",
        'b' => "en-gb",
        'e' => "es",
        'f' => "fr-fr",
        'h' => "hi",
        'i' => "it",
        'p' => "pt-br",
        _ => return None,
    })
}

/// A Kokoro build in memory.
pub(super) struct Kokoro {
    model: JsValue,
    tokenizer: JsValue,
    tensor: JsValue,
    espeak: Espeak,
    /// Each voice it speaks with, and where its style table is stored.
    voices: BTreeMap<String, String>,
    /// The style tables read so far.
    styles: BTreeMap<String, Vec<f32>>,
    _model: Model,
}

impl Kokoro {
    /// The model and its tokenizer over the build's files, and eSpeak NG. Fails with `model-load-failed`.
    pub(super) async fn load(model: Model, files: &Installed) -> Result<Self> {
        let loaded = async {
            let id: JsValue = model.served.id().into();
            let tokenizer = model.export("AutoTokenizer")?;
            let tokenizer =
                call_method(&tokenizer, "from_pretrained", std::slice::from_ref(&id)).await?;
            let class = model.export("StyleTextToSpeech2Model")?;
            let loaded = call_method(&class, "from_pretrained", &[id, model.options()]).await?;
            let espeak = Espeak::import().await?;
            Ok::<_, JsValue>((loaded, tokenizer, espeak))
        }
        .await;
        let (loaded, tokenizer, espeak) =
            loaded.map_err(|error| failed("model-load-failed", &error))?;
        let tensor = model
            .export("Tensor")
            .map_err(|error| failed("model-load-failed", &error))?;
        let voices = voices(files)
            .into_iter()
            .filter(|voice| espeak_voice(voice).is_some())
            .filter_map(|voice| {
                let at = files.file(&format!("{VOICES}{voice}"))?.to_owned();
                Some((voice, at))
            })
            .collect();
        Ok(Self {
            model: loaded,
            tokenizer,
            tensor,
            espeak,
            voices,
            styles: BTreeMap::new(),
            _model: model,
        })
    }

    /// The style table of `voice`, read from storage the first time. Fails with `speech-failed`.
    async fn styles(&mut self, voice: &str) -> Result<&[f32]> {
        if !self.styles.contains_key(voice) {
            let location = self.voices.get(voice).ok_or(Error::new("unknown-voice"))?;
            let read = async {
                let blob = opfs::open(location).await?.ok_or("not stored")?;
                let bytes = JsFuture::from(blob.array_buffer()).await?;
                Ok::<_, JsValue>(Float32Array::new(&bytes).to_vec())
            }
            .await;
            let table = read.map_err(|error| failed("speech-failed", &error))?;
            self.styles.insert(voice.to_owned(), table);
        }
        Ok(&self.styles[voice])
    }

    /// One stretch of phonemes Kokoro reads at once, spoken. Fails with `speech-failed`.
    async fn speak_phonemes(
        &mut self,
        phonemes: &str,
        voice: &str,
        speed: f32,
    ) -> Result<Vec<f32>> {
        let tokenized = async {
            let options = object(&[("truncation", false.into())]);
            let encoded = super::call(
                &self.tokenizer,
                &self.tokenizer,
                &[phonemes.into(), options],
            )
            .await?;
            let ids = Reflect::get(&encoded, &"input_ids".into())?;
            let dims: js_sys::Array = Reflect::get(&ids, &"dims".into())?.into();
            let tokens = dims.at(-1).as_f64().unwrap_or(2.0) as usize;
            Ok::<_, JsValue>((ids, tokens.saturating_sub(2)))
        }
        .await;
        let (ids, tokens) = tokenized.map_err(|error| failed("speech-failed", &error))?;
        let table = self.styles(voice).await?;
        let style = table
            .get(tokens * STYLE..(tokens + 1) * STYLE)
            .ok_or_else(|| failed("speech-failed", &"no style for this length".into()))?;
        let style = Float32Array::from(style);
        let spoken = async {
            let tensor = |data: JsValue, dims: &[u32]| {
                let dims: js_sys::Array = dims.iter().map(|&d| JsValue::from(d)).collect();
                js_sys::Reflect::construct(
                    self.tensor.unchecked_ref::<js_sys::Function>(),
                    &js_sys::Array::of3(&"float32".into(), &data, &dims),
                )
            };
            let speed = Float32Array::from(&[speed][..]);
            let inputs = object(&[
                ("input_ids", ids),
                ("style", tensor(style.into(), &[1, STYLE as u32])?),
                ("speed", tensor(speed.into(), &[1])?),
            ]);
            let output = call(&self.model, &self.model, &[inputs]).await?;
            let waveform = Reflect::get(&output, &"waveform".into())?;
            let data: Float32Array = Reflect::get(&waveform, &"data".into())?.into();
            Ok::<_, JsValue>(data.to_vec())
        }
        .await;
        finite(spoken.map_err(|error| failed("speech-failed", &error))?)
    }
}

impl BackendModel for Kokoro {
    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[async_trait::async_trait(?Send)]
impl TtsModel for Kokoro {
    /// The voices of the build whose language eSpeak NG phonemizes.
    fn voices(&self) -> Vec<String> {
        self.voices.keys().cloned().collect()
    }

    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    /// A Kokoro voice speaks one language, so `language` is the voice's own and is not read. A text longer than
    /// Kokoro reads at once is spoken a stretch at a time, cut after a punctuation mark or between words, and the
    /// stretches are joined.
    async fn speak(
        &mut self,
        text: &str,
        voice: &Voice,
        _language: Option<&str>,
        speed: f32,
    ) -> Result<Vec<f32>> {
        let voice = voice.id.as_str();
        if !self.voices.contains_key(voice) {
            return Err(Error::new("unknown-voice"));
        }
        let espeak_voice = espeak_voice(voice).ok_or(Error::new("unknown-voice"))?;
        let phonemes = self
            .espeak
            .phonemes(text, espeak_voice)
            .await
            .map_err(|error| failed("speech-failed", &error))?;
        let mut samples = Vec::new();
        for stretch in stretches(&phonemes, MAX_TOKENS) {
            samples.extend(self.speak_phonemes(stretch, voice, speed).await?);
        }
        Ok(samples)
    }
}

impl Drop for Kokoro {
    fn drop(&mut self) {
        dispose(self.model.clone());
    }
}

/// `phonemes` in stretches of at most `max` characters (Kokoro's tokens are its phonemes, one each), each cut after
/// the last punctuation mark that fits, else after the last space, else where it must.
pub(super) fn stretches(phonemes: &str, max: usize) -> Vec<&str> {
    let mut stretches = Vec::new();
    let mut rest = phonemes.trim();
    while rest.chars().count() > max {
        let limit = rest
            .char_indices()
            .nth(max)
            .map_or(rest.len(), |(at, _)| at);
        let head = &rest[..limit];
        let cut = head
            .rfind(|c: char| ".!?;:,—…".contains(c))
            .map(|at| at + head[at..].chars().next().map_or(1, char::len_utf8))
            .or_else(|| head.rfind(' ').filter(|&at| at > 0))
            .unwrap_or(limit);
        stretches.push(rest[..cut].trim());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() {
        stretches.push(rest);
    }
    stretches
}
