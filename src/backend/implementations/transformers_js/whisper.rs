//! Whisper, through transformers.js's `automatic-speech-recognition` pipeline.

use js_sys::Float32Array;
use wasm_bindgen::JsValue;

use super::{call, dispose, failed, object, Model};
use crate::backend::loaded_model::SttModel;
use crate::backend::BackendModel;
use crate::Result;

/// A Whisper build in memory: the pipeline, and its files served for as long as it lives.
pub(super) struct Whisper {
    pipeline: JsValue,
    _model: Model,
}

impl Whisper {
    /// The pipeline over the build's files. Fails with `model-load-failed`.
    pub(super) async fn load(model: Model) -> Result<Self> {
        let loaded = async {
            let pipeline = model.export("pipeline")?;
            let args = [
                "automatic-speech-recognition".into(),
                model.served.id().into(),
                model.options(),
            ];
            call(&pipeline, &JsValue::UNDEFINED, &args).await
        }
        .await;
        let pipeline = loaded.map_err(|error| failed("model-load-failed", &error))?;
        Ok(Self {
            pipeline,
            _model: model,
        })
    }
}

impl BackendModel for Whisper {
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[async_trait::async_trait(?Send)]
impl SttModel for Whisper {
    /// Whole turns of any length: in 30-second windows, as Whisper hears them, overlapping by 5. `language` is passed as
    /// its primary subtag (`es` for `es-ES`), which is how Whisper names its languages.
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String> {
        let mut options = vec![
            ("task", "transcribe".into()),
            ("chunk_length_s", 30.into()),
            ("stride_length_s", 5.into()),
        ];
        if let Some(language) = language {
            let primary = language.split('-').next().unwrap_or(language);
            options.push(("language", primary.to_ascii_lowercase().into()));
        }
        let audio = Float32Array::from(pcm);
        let args = [audio.into(), object(&options)];
        let result = call(&self.pipeline, &JsValue::UNDEFINED, &args)
            .await
            .map_err(|error| failed("transcription-failed", &error))?;
        let text = js_sys::Reflect::get(&result, &"text".into())
            .ok()
            .and_then(|text| text.as_string())
            .ok_or_else(|| failed("transcription-failed", &result))?;
        Ok(text.trim().to_owned())
    }
}

impl Drop for Whisper {
    fn drop(&mut self) {
        dispose(self.pipeline.clone());
    }
}
