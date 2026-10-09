//! smart-turn v3 on the ONNX Runtime Web that transformers.js ships with, as Silero is (`silero.rs`): loaded as a
//! custom model (no config) from its one ONNX file, which smart-turn's repository keeps at its root under a full name
//! (`smart-turn-v3.2-cpu.onnx`, already int8): so it is asked for by that name, in no subfolder, at transformers.js's
//! `fp32`, the dtype that adds no suffix. Its input is made in Rust (`crate::backend::smart_turn`), the same as natively;
//! its one output is the probability that the turn is complete.

use js_sys::Float32Array;
use wasm_bindgen::prelude::*;

use super::{call, dispose, failed, hub, object, Model};
use crate::backend::smart_turn::{self, FRAMES, INPUT, MELS, SECONDS};
use crate::backend::{BackendModel, EndOfTurnModel};
use crate::catalog::BuildEntry;
use crate::{Error, Result};

/// The key of the model's file.
pub(super) const KEY: &str = "smart_turn";

#[wasm_bindgen(inline_js = r#"
export async function smartTurnRun(model, Tensor, name, features, mels, frames) {
  const outputs = await model({ [name]: new Tensor('float32', features, [1, mels, frames]) });
  return Object.values(outputs)[0].data[0];
}
"#)]
extern "C" {
    /// The model's first output for `features` ([1, mels, frames]) as its input `name`.
    #[wasm_bindgen(catch, js_name = smartTurnRun)]
    async fn smart_turn_run(
        model: &JsValue,
        tensor: &JsValue,
        name: &str,
        features: &Float32Array,
        mels: u32,
        frames: u32,
    ) -> Result<JsValue, JsValue>;
}

/// A smart-turn build in memory: the model, transformers.js's `Tensor`, and its file served for as long as it lives.
pub(super) struct SmartTurn {
    model: JsValue,
    tensor: JsValue,
    _model: Model,
}

impl SmartTurn {
    /// The model over the build's file. Fails with `unsupported-model` for a file that is not ONNX, and
    /// `model-load-failed`.
    pub(super) async fn load(model: Model, build: &BuildEntry) -> Result<Self> {
        let file = build
            .files
            .iter()
            .find(|file| file.key == KEY)
            .and_then(|file| hub::repository_path(&file.url))
            .ok_or(Error::new("unsupported-model"))?;
        let (folder, name) = file.rsplit_once('/').unwrap_or(("", file));
        let stem = name
            .strip_suffix(".onnx")
            .ok_or(Error::new("unsupported-model"))?;
        let loaded = async {
            let auto = model.export("AutoModel")?;
            let from_pretrained = js_sys::Reflect::get(&auto, &"from_pretrained".into())?;
            let options = object(&[
                ("config", object(&[("model_type", "custom".into())])),
                ("device", model.device.into()),
                ("dtype", "fp32".into()),
                ("subfolder", folder.into()),
                ("model_file_name", stem.into()),
            ]);
            call(
                &from_pretrained,
                &auto,
                &[model.served.id().into(), options],
            )
            .await
        }
        .await;
        let loaded = loaded.map_err(|error| failed("model-load-failed", &error))?;
        let tensor = model
            .export("Tensor")
            .map_err(|error| failed("model-load-failed", &error))?;
        Ok(Self {
            model: loaded,
            tensor,
            _model: model,
        })
    }
}

impl BackendModel for SmartTurn {
    fn as_end_of_turn(&mut self) -> Option<&mut dyn EndOfTurnModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[async_trait::async_trait(?Send)]
impl EndOfTurnModel for SmartTurn {
    fn seconds(&self) -> u32 {
        SECONDS
    }

    /// Fails with `end-of-turn-failed`.
    async fn probability(&mut self, pcm: &[f32]) -> Result<f32> {
        let features = Float32Array::from(smart_turn::features_yielding(pcm).await.as_slice());
        let (mels, frames) = (MELS as u32, FRAMES as u32);
        let answer = smart_turn_run(&self.model, &self.tensor, INPUT, &features, mels, frames)
            .await
            .map_err(|error| failed("end-of-turn-failed", &error))?;
        let probability = answer
            .as_f64()
            .ok_or_else(|| failed("end-of-turn-failed", &answer))?;
        Ok(probability as f32)
    }
}

impl Drop for SmartTurn {
    fn drop(&mut self) {
        dispose(self.model.clone());
    }
}
