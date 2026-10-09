# Changelog

## [0.2.0](https://github.com/sidevoice/sidevoice-engine/compare/v0.1.0...v0.2.0) (2026-10-09)


### Features

* backends.json, each backend's downloads per platform, pinned by cargo xtask ([#17](https://github.com/sidevoice/sidevoice-engine/issues/17)) ([5572b16](https://github.com/sidevoice/sidevoice-engine/commit/5572b168de0f997714bfde39bb3ce1fda82a6f47))
* **backend:** sherpa-onnx config fields generated from the pinned crate ([#49](https://github.com/sidevoice/sidevoice-engine/issues/49)) ([adf3257](https://github.com/sidevoice/sidevoice-engine/commit/adf3257c494cbee6ca825f5225b15dbcb9eeb311))
* **backend:** sherpa-onnx runs Whisper and Kokoro natively, through the official crate ([#27](https://github.com/sidevoice/sidevoice-engine/issues/27)) ([960ae14](https://github.com/sidevoice/sidevoice-engine/commit/960ae14c74eb857c4f260c939ab2b16a79d4b339))
* **backend:** whisper.cpp through whisper-rs, linked natively with Metal on Apple silicon ([#40](https://github.com/sidevoice/sidevoice-engine/issues/40)) ([de128c8](https://github.com/sidevoice/sidevoice-engine/commit/de128c87088d4601d78601b69dc298923fd4781d))
* capabilities contract, fixed accelerator list and build accelerator restrictions ([#15](https://github.com/sidevoice/sidevoice-engine/issues/15)) ([ef86e40](https://github.com/sidevoice/sidevoice-engine/commit/ef86e4005268e64e4cd439654e000c88ac4f91f5))
* **catalog:** more speech models, and the sherpa-onnx loaders they need ([#31](https://github.com/sidevoice/sidevoice-engine/issues/31)) ([d2296e1](https://github.com/sidevoice/sidevoice-engine/commit/d2296e1d12b72bb3ec9c301a99b517464ff5bacb))
* **catalog:** Whisper large-v3/turbo, Canary-180m-flash and Qwen3-ASR 0.6B, as data ([#51](https://github.com/sidevoice/sidevoice-engine/issues/51)) ([4902bd2](https://github.com/sidevoice/sidevoice-engine/commit/4902bd2072a66494b3c0e49dde91d7dc9d8683ee))
* engine interfaces: Host, Backend, catalogue, funnel and backend discovery ([#1](https://github.com/sidevoice/sidevoice-engine/issues/1)) ([5c8f987](https://github.com/sidevoice/sidevoice-engine/commit/5c8f987372c19fe59a57c9d1e702b1eb0270d14b))
* **engine:** the model interface: models, install, uninstall, load -&gt; LoadedModel (as_stt, as_tts) ([#58](https://github.com/sidevoice/sidevoice-engine/issues/58)) ([35cba73](https://github.com/sidevoice/sidevoice-engine/commit/35cba731a17236e69496f5137f563e4852ff0efd))
* installer, build lifecycle and the built-in NativeHost ([#29](https://github.com/sidevoice/sidevoice-engine/issues/29)) ([a550916](https://github.com/sidevoice/sidevoice-engine/commit/a550916c381833d8c162bba99e4d0c9c9ef1888a))
* **sherpa-onnx:** a call's language reaches the config where the build's call_params map it ([#60](https://github.com/sidevoice/sidevoice-engine/issues/60)) ([d618d25](https://github.com/sidevoice/sidevoice-engine/commit/d618d250df08d303f5184816a890d02596df471c))
* the catalogue as data: model families compiled in, Capability, pinned by cargo xtask ([#28](https://github.com/sidevoice/sidevoice-engine/issues/28)) ([e57743f](https://github.com/sidevoice/sidevoice-engine/commit/e57743f2228644504fe22315df8c3dcb5db43abb))
* **web:** the bridge to JavaScript, only in the wasm32 build ([#11](https://github.com/sidevoice/sidevoice-engine/issues/11)) ([238a002](https://github.com/sidevoice/sidevoice-engine/commit/238a002e673bc42785c30f6ffb86f69472be4a3a))
* **web:** the transformers.js backend, OPFS storage, fetch downloads and WebEngine on the model interface ([#41](https://github.com/sidevoice/sidevoice-engine/issues/41)) ([158274d](https://github.com/sidevoice/sidevoice-engine/commit/158274dfe5cfe179a0bc715e911173af45b40739))


### Bug Fixes

* **catalog:** transformers.js fp16 builds run on WebGPU only ([#56](https://github.com/sidevoice/sidevoice-engine/issues/56)) ([e2801ee](https://github.com/sidevoice/sidevoice-engine/commit/e2801ee6c5b2dc6be8bba6955b7c10338320878f))
* **sherpa-onnx:** a language without a region takes the chosen voice's region for espeak-ng ([#65](https://github.com/sidevoice/sidevoice-engine/issues/65)) ([ba3fb56](https://github.com/sidevoice/sidevoice-engine/commit/ba3fb56840267728dd969df801f913654503f54e)), closes [#47](https://github.com/sidevoice/sidevoice-engine/issues/47)
