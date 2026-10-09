//! Whisper, through transformers.js's `automatic-speech-recognition` pipeline.
//!
//! The pipeline does not detect the language: a multilingual model given none is forced to English (`<|en|>`), so
//! speech in another language comes out translated. When no language is given, this detects it first, as Whisper
//! does: one decoder step after `<|startoftranscript|>` on the first 30-second window, the most likely of the model's
//! language tokens (`generation_config.lang_to_id`), and transcribes in that language.

use js_sys::Float32Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;

use super::{call, dispose, failed, object, Model};
use crate::backend::loaded_model::SttModel;
use crate::backend::BackendModel;
use crate::Result;

#[wasm_bindgen(inline_js = r#"
export async function detectLanguage(pipeline, Tensor, audio) {
  const config = pipeline.model.generation_config ?? {};
  const languages = Object.entries(config.lang_to_id ?? {});
  if (config.is_multilingual === false || languages.length === 0) return null;
  const { input_features } = await pipeline.processor(audio.subarray(0, 30 * 16000));
  const start = new Tensor('int64', BigInt64Array.from([BigInt(config.decoder_start_token_id)]), [1, 1]);
  const outputs = await pipeline.model({ input_features, decoder_input_ids: start });
  try {
    const logits = outputs.logits.to('float32');
    const scores = logits.data.subarray(logits.data.length - logits.dims.at(-1));
    let best = null;
    for (const [token, id] of languages) {
      if (best === null || scores[id] > scores[best[1]]) best = [token, id];
    }
    return best[0].replace(/^<\|/, '').replace(/\|>$/, '');
  } finally {
    input_features.dispose?.();
    for (const value of Object.values(outputs)) value?.dispose?.();
  }
}
"#)]
extern "C" {
    /// The language Whisper hears in the first window of `audio` (mono 16 kHz), as its code (`es`), or null for a
    /// model that speaks English only. Through the pipeline's own processor and model, so on its device and dtype.
    #[wasm_bindgen(catch, js_name = detectLanguage)]
    async fn detect_language(
        pipeline: &JsValue,
        tensor: &JsValue,
        audio: &Float32Array,
    ) -> Result<JsValue, JsValue>;
}

/// A Whisper build in memory: the pipeline, transformers.js's `Tensor`, and its files served for as long as it lives.
pub(super) struct Whisper {
    pipeline: JsValue,
    tensor: JsValue,
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
        let tensor = model
            .export("Tensor")
            .map_err(|error| failed("model-load-failed", &error))?;
        Ok(Self {
            pipeline,
            tensor,
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
    /// its primary subtag (`es` for `es-ES`), which is how Whisper names its languages; with none, the language is
    /// detected first (see the module), so the whole turn is transcribed in the one heard at its start.
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String> {
        let audio = Float32Array::from(pcm);
        let language = match language {
            Some(language) => {
                let primary = language.split('-').next().unwrap_or(language);
                Some(primary.to_ascii_lowercase())
            }
            None => detect_language(&self.pipeline, &self.tensor, &audio)
                .await
                .map_err(|error| failed("transcription-failed", &error))?
                .as_string(),
        };
        let mut options = vec![
            ("task", "transcribe".into()),
            ("chunk_length_s", 30.into()),
            ("stride_length_s", 5.into()),
        ];
        if let Some(language) = language {
            options.push(("language", language.into()));
        }
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
