//! Silero VAD on the ONNX Runtime Web that transformers.js ships with. transformers.js has no voice activity pipeline,
//! but it loads any ONNX model as a custom one (`AutoModel` with `model_type: "custom"`, so it reads no config) and
//! runs it on its device and dtype: the same runtime, the same hub (the engine's files) and the same import as every
//! other model of this backend, with no second copy of ONNX Runtime Web for the page to load.
//!
//! The model gives a probability per window; the stream segments speech from it with `crate::backend::Segmenter`,
//! by sherpa-onnx's rules. Silero v5 takes 512 new samples at 16 kHz, seen after the last 64 of the window before
//! (zeros at first), as its own Python wrapper feeds it, and carries its state from one window to the next. An ONNX
//! Runtime session runs one call at a time, so the streams of one model take turns.

use std::ops::Range;
use std::rc::Rc;

use js_sys::{Array, Float32Array};
use wasm_bindgen::prelude::*;

use super::{call, dispose, failed, object, Model};
use crate::backend::{BackendModel, Segmenter, VadModel, VadStreamModel, Window};
use crate::capability::VadOptions;
use crate::Result;

/// The rate Silero runs at here.
const SAMPLE_RATE: u32 = 16_000;
/// The new samples of each window, and the samples of the window before that it is seen with.
const WINDOW: usize = 512;
const CONTEXT: usize = 64;

#[wasm_bindgen(inline_js = r#"
export async function sileroRun(model, Tensor, input, state, rate) {
  state ??= new Tensor('float32', new Float32Array(2 * 1 * 128), [2, 1, 128]);
  const sr = new Tensor('int64', BigInt64Array.from([BigInt(rate)]), []);
  const { output, stateN } = await model({ input: new Tensor('float32', input, [1, input.length]), sr, state });
  return [output.data[0], stateN];
}
"#)]
extern "C" {
    /// One window through the model: `input` (the context, then the window), the state the last window left (null at
    /// first) and the rate; `[probability, state]`.
    #[wasm_bindgen(catch, js_name = sileroRun)]
    async fn silero_run(
        model: &JsValue,
        tensor: &JsValue,
        input: &Float32Array,
        state: &JsValue,
        rate: u32,
    ) -> Result<JsValue, JsValue>;
}

/// A Silero build in memory: the model, transformers.js's `Tensor`, what makes its streams take turns, and its files
/// served for as long as it lives.
pub(super) struct Silero {
    model: JsValue,
    tensor: JsValue,
    turns: Rc<async_lock::Mutex<()>>,
    _model: Model,
}

impl Silero {
    /// The model over the build's files. Fails with `model-load-failed`.
    pub(super) async fn load(model: Model) -> Result<Self> {
        let loaded = async {
            let auto = model.export("AutoModel")?;
            let from_pretrained = js_sys::Reflect::get(&auto, &"from_pretrained".into())?;
            let options = object(&[
                ("config", object(&[("model_type", "custom".into())])),
                ("device", model.device.into()),
                ("dtype", model.dtype.as_str().into()),
            ]);
            let args = [model.served.id().into(), options];
            call(&from_pretrained, &auto, &args).await
        }
        .await;
        let loaded = loaded.map_err(|error| failed("model-load-failed", &error))?;
        let tensor = model
            .export("Tensor")
            .map_err(|error| failed("model-load-failed", &error))?;
        Ok(Self {
            model: loaded,
            tensor,
            turns: Rc::default(),
            _model: model,
        })
    }
}

impl BackendModel for Silero {
    fn as_vad(&mut self) -> Option<&mut dyn VadModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

impl VadModel for Silero {
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    fn window(&self) -> usize {
        WINDOW
    }

    fn stream(&mut self, options: &VadOptions) -> Result<Box<dyn VadStreamModel>> {
        Ok(Box::new(SileroStream {
            model: self.model.clone(),
            tensor: self.tensor.clone(),
            turns: Rc::clone(&self.turns),
            state: JsValue::NULL,
            context: vec![0.0; CONTEXT],
            segmenter: Segmenter::new(options, SAMPLE_RATE, WINDOW, CONTEXT),
        }))
    }
}

impl Drop for Silero {
    fn drop(&mut self) {
        dispose(self.model.clone());
    }
}

/// One stream: the model's state after its last window, that window's last samples, and its segmentation.
struct SileroStream {
    model: JsValue,
    tensor: JsValue,
    turns: Rc<async_lock::Mutex<()>>,
    state: JsValue,
    context: Vec<f32>,
    segmenter: Segmenter,
}

impl SileroStream {
    fn start_over(&mut self) {
        self.state = JsValue::NULL;
        self.context = vec![0.0; CONTEXT];
    }
}

#[async_trait::async_trait(?Send)]
impl VadStreamModel for SileroStream {
    /// Fails with `detection-failed`.
    async fn window(&mut self, pcm: &[f32]) -> Result<Window> {
        let input: Vec<f32> = self.context.iter().chain(pcm).copied().collect();
        let ran = {
            let _turn = self.turns.lock().await;
            let input = Float32Array::from(input.as_slice());
            silero_run(&self.model, &self.tensor, &input, &self.state, SAMPLE_RATE).await
        };
        let ran: Array = ran
            .map_err(|error| failed("detection-failed", &error))?
            .into();
        let probability = ran
            .get(0)
            .as_f64()
            .ok_or_else(|| failed("detection-failed", &ran))?;
        self.state = ran.get(1);
        self.context = pcm[pcm.len().saturating_sub(CONTEXT)..].to_vec();
        Ok(self.segmenter.window(probability as f32))
    }

    fn finish(&mut self) -> Option<Range<u64>> {
        self.start_over();
        self.segmenter.finish()
    }

    fn reset(&mut self) {
        self.start_over();
        self.segmenter.reset();
    }
}
