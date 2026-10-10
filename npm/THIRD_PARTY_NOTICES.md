# Third-party notices

`@sidevoice/engine` is Apache-2.0. It depends on the packages below, which npm installs beside it; each keeps its own
licence. The engine imports them only when a model of the transformers.js backend is loaded, and does not bundle them:
an app that bundles the engine for the browser bundles whatever of them its bundler pulls in, under their licences.

| Package | Version | Licence | What the engine uses it for |
|---|---|---|---|
| [`@huggingface/transformers`](https://github.com/huggingface/transformers.js) | 4.3.1 | Apache-2.0 | Runs the web builds of the catalogue (Whisper, Kokoro, Supertonic) on ONNX Runtime Web |
| [`onnxruntime-web`](https://github.com/microsoft/onnxruntime) (through transformers.js) | as transformers.js pins it | MIT | The ONNX runtime transformers.js runs models on |
| [`espeak-ng`](https://github.com/ianmarmour/espeak-ng.js) ([eSpeak NG](https://github.com/espeak-ng/espeak-ng) compiled to WebAssembly, with its language data) | 1.0.2 | **GPL-3.0-or-later** | Kokoro's phonemes, for every language Kokoro speaks in the browser |

**eSpeak NG is under the GPL-3.0-or-later**, unlike the engine. Whether and how the engine ships with it is an open
decision, tracked in [sidevoice-engine#30](https://github.com/sidevoice/sidevoice-engine/issues/30); until it is taken,
this notice says what the package depends on, and nothing here settles it.

The models themselves are not part of the package: the engine downloads each from the URL its catalogue pins, under
the model's own licence, which the local catalogue's `models()` reports per model.
