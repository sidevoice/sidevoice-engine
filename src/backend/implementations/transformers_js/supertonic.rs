//! Supertonic, through transformers.js's `text-to-speech` pipeline. A voice is a style table (`voices/<id>.bin`); the
//! language is told to the model by tags around the text (`<es>…</es>`), as its model card does.

use std::collections::BTreeMap;

use js_sys::{Float32Array, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use super::{call, dispose, failed, object, voices, Model, VOICES};
use crate::backend::loaded_model::TtsModel;
use crate::backend::BackendModel;
use crate::install::Installed;
use crate::web::opfs;
use crate::{Error, Result};

/// A Supertonic build in memory.
pub(super) struct Supertonic {
    pipeline: JsValue,
    sample_rate: u32,
    /// Each voice it speaks with, and where its style table is stored.
    voices: BTreeMap<String, String>,
    /// The style tables read so far.
    styles: BTreeMap<String, Float32Array>,
    _model: Model,
}

impl Supertonic {
    /// The pipeline over the build's files. Fails with `model-load-failed`.
    pub(super) async fn load(model: Model, files: &Installed) -> Result<Self> {
        let loaded = async {
            let pipeline = model.export("pipeline")?;
            let args = [
                "text-to-speech".into(),
                model.served.id().into(),
                model.options(),
            ];
            let pipeline = call(&pipeline, &JsValue::UNDEFINED, &args).await?;
            // The rate its config gives: `pipeline.model.config.sampling_rate`.
            let config =
                Reflect::get(&Reflect::get(&pipeline, &"model".into())?, &"config".into())?;
            let rate = Reflect::get(&config, &"sampling_rate".into())?.as_f64();
            Ok::<_, JsValue>((pipeline, rate))
        }
        .await;
        let (pipeline, rate) = loaded.map_err(|error| failed("model-load-failed", &error))?;
        let sample_rate = rate.filter(|rate| *rate > 0.0).ok_or_else(|| {
            failed(
                "model-load-failed",
                &"no sampling_rate in its config".into(),
            )
        })? as u32;
        let voices = voices(files)
            .into_iter()
            .filter_map(|voice| {
                let at = files.file(&format!("{VOICES}{voice}"))?.to_owned();
                Some((voice, at))
            })
            .collect();
        Ok(Self {
            pipeline,
            sample_rate,
            voices,
            styles: BTreeMap::new(),
            _model: model,
        })
    }

    /// The style table of `voice`, read from storage the first time. Fails with `speech-failed`.
    async fn style(&mut self, voice: &str) -> Result<Float32Array> {
        if let Some(style) = self.styles.get(voice) {
            return Ok(style.clone());
        }
        let location = self.voices.get(voice).ok_or(Error::new("unknown-voice"))?;
        let read = async {
            let blob = opfs::open(location).await?.ok_or("not stored")?;
            let bytes = JsFuture::from(blob.array_buffer()).await?;
            Ok::<_, JsValue>(Float32Array::new(&bytes))
        }
        .await;
        let style = read.map_err(|error| failed("speech-failed", &error))?;
        self.styles.insert(voice.to_owned(), style.clone());
        Ok(style)
    }
}

impl BackendModel for Supertonic {
    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[async_trait::async_trait(?Send)]
impl TtsModel for Supertonic {
    fn voices(&self) -> Vec<String> {
        self.voices.keys().cloned().collect()
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// `language` goes to the model as its primary subtag, in tags around the text; with `None` the text goes as it is.
    async fn speak(
        &mut self,
        text: &str,
        voice: &str,
        language: Option<&str>,
        speed: f32,
    ) -> Result<Vec<f32>> {
        let style = self.style(voice).await?;
        let text = match language {
            Some(language) => {
                let primary = language
                    .split('-')
                    .next()
                    .unwrap_or(language)
                    .to_ascii_lowercase();
                format!("<{primary}>{text}</{primary}>")
            }
            None => text.to_owned(),
        };
        let options = object(&[
            ("speaker_embeddings", style.into()),
            ("speed", speed.into()),
        ]);
        let spoken = async {
            let output = call(
                &self.pipeline,
                &JsValue::UNDEFINED,
                &[text.as_str().into(), options],
            )
            .await?;
            let audio: Float32Array = Reflect::get(&output, &"audio".into())?.dyn_into()?;
            Ok::<_, JsValue>(audio.to_vec())
        }
        .await;
        spoken.map_err(|error| failed("speech-failed", &error))
    }
}

impl Drop for Supertonic {
    fn drop(&mut self) {
        dispose(self.pipeline.clone());
    }
}
