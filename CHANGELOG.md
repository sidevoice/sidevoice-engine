# Changelog

## [0.7.0](https://github.com/sidevoice/sidevoice-engine/compare/v0.6.0...v0.7.0) (2026-10-30)


### Features

* **platform:** move accelerator discovery behind platform and report requested vs actual accelerator ([#129](https://github.com/sidevoice/sidevoice-engine/issues/129)) ([21c513a](https://github.com/sidevoice/sidevoice-engine/commit/21c513a33cdcb35bb1fc6fad34f421f1bb07e5f2))


### Bug Fixes

* **sherpa-onnx:** fail cleanly, and test under UTF-8, for non-ASCII espeak-ng data on Windows ([#131](https://github.com/sidevoice/sidevoice-engine/issues/131)) ([38a4912](https://github.com/sidevoice/sidevoice-engine/commit/38a4912a8e36296eef3e03bdad622ff101799723))
* **xtask:** platform guard sees framework families, OS aliases and OS commands ([#121](https://github.com/sidevoice/sidevoice-engine/issues/121)) ([93b9202](https://github.com/sidevoice/sidevoice-engine/commit/93b9202eb6be1dcdbf22df7e45e4c7ddfa703406))


### Performance Improvements

* **download:** write a download in 8 MiB parts, not as each network part arrives ([#125](https://github.com/sidevoice/sidevoice-engine/issues/125)) ([7fed220](https://github.com/sidevoice/sidevoice-engine/commit/7fed22064ffb26c471d526d246a6a64985a7a09e))

## [0.6.0](https://github.com/sidevoice/sidevoice-engine/compare/v0.5.0...v0.6.0) (2026-10-26)


### Features

* **windows:** build and test on Windows; CPU backends; uninstall frees disk ([#117](https://github.com/sidevoice/sidevoice-engine/issues/117)) ([2335e39](https://github.com/sidevoice/sidevoice-engine/commit/2335e39491aff9841f28d019e73f019dedd4d4eb))


### Bug Fixes

* **catalog:** re-pin kokoro-82m-v1.0 sherpa-onnx archive digest ([#122](https://github.com/sidevoice/sidevoice-engine/issues/122)) ([bea2a37](https://github.com/sidevoice/sidevoice-engine/commit/bea2a37a89ce5deab24331cceffc312cdb84d90b))
* **provider:** tell a provider's refusal by what it says, not by its HTTP status alone ([#104](https://github.com/sidevoice/sidevoice-engine/issues/104)) ([417febf](https://github.com/sidevoice/sidevoice-engine/commit/417febf3e6e557dafa1d1f988235aeeacbe4bd02))
* **provider:** tell OpenAI's quota from its rate limit ([#107](https://github.com/sidevoice/sidevoice-engine/issues/107)) ([795371a](https://github.com/sidevoice/sidevoice-engine/commit/795371aa6e2bd14d3edf3652e61501976959d432)), closes [#106](https://github.com/sidevoice/sidevoice-engine/issues/106)
* **web:** fail a download that receives no bytes for a minute ([#109](https://github.com/sidevoice/sidevoice-engine/issues/109)) ([270b5ff](https://github.com/sidevoice/sidevoice-engine/commit/270b5ffafaa463a7aee7fb6a5afe18fac9ca863a))

## [0.5.0](https://github.com/sidevoice/sidevoice-engine/compare/v0.4.0...v0.5.0) (2026-10-14)


### Features

* **engine:** rank the builds that run by accelerator class: GPU, then Neural Engine, then CPU ([#94](https://github.com/sidevoice/sidevoice-engine/issues/94)) ([a5421ca](https://github.com/sidevoice/sidevoice-engine/commit/a5421ca782e0db9fc517ca2ffbb243c846fbff2f)), closes [#4](https://github.com/sidevoice/sidevoice-engine/issues/4)


### Bug Fixes

* **catalog:** declare Supertonic's ten voices, with upstream's names and genders ([#96](https://github.com/sidevoice/sidevoice-engine/issues/96)) ([f9cd6a4](https://github.com/sidevoice/sidevoice-engine/commit/f9cd6a412c059d46634d465624ad3900460441ac))


### Performance Improvements

* **engine:** flash attention for whisper.cpp on the GPU ([#100](https://github.com/sidevoice/sidevoice-engine/issues/100)) ([8e3c2a5](https://github.com/sidevoice/sidevoice-engine/commit/8e3c2a5ee2e250ec1b8638b769ed83dbc693746b))

## [0.4.0](https://github.com/sidevoice/sidevoice-engine/compare/v0.3.1...v0.4.0) (2026-10-12)


### Features

* **engine:** when each downloaded model was last used ([#90](https://github.com/sidevoice/sidevoice-engine/issues/90)) ([78b5314](https://github.com/sidevoice/sidevoice-engine/commit/78b5314e064b116fd5ac0f9cdf9b00886d2d732c))

## [0.3.1](https://github.com/sidevoice/sidevoice-engine/compare/v0.3.0...v0.3.1) (2026-10-10)


### Bug Fixes

* **engine:** offer ElevenLabs' speech-to-text models from its spec, which its listing never reports ([#86](https://github.com/sidevoice/sidevoice-engine/issues/86)) ([ab01213](https://github.com/sidevoice/sidevoice-engine/commit/ab01213339142eaab8250f4f3b7d1600754eadd2))

## [0.3.0](https://github.com/sidevoice/sidevoice-engine/compare/v0.2.0...v0.3.0) (2026-10-10)


### ⚠ BREAKING CHANGES

* **engine:** uninstall one build of a model ([#81](https://github.com/sidevoice/sidevoice-engine/issues/81))
* **engine:** catalogues as one interface, and provider facts read from their specs at run time ([#82](https://github.com/sidevoice/sidevoice-engine/issues/82))
* **engine:** remote providers beside the catalogue, and one set of capability interfaces ([#80](https://github.com/sidevoice/sidevoice-engine/issues/80))

### Features

* **engine:** catalogues as one interface, and provider facts read from their specs at run time ([#82](https://github.com/sidevoice/sidevoice-engine/issues/82)) ([4f09da7](https://github.com/sidevoice/sidevoice-engine/commit/4f09da7f10c483b977a60abad3652b449b0d992b))
* **engine:** end-of-turn capability, smart-turn v3 on the linked ONNX Runtime and on transformers.js ([#78](https://github.com/sidevoice/sidevoice-engine/issues/78)) ([8aa89d1](https://github.com/sidevoice/sidevoice-engine/commit/8aa89d1f888f8f2eb40279878c1b15855da36d1b))
* **engine:** remote providers beside the catalogue, and one set of capability interfaces ([#80](https://github.com/sidevoice/sidevoice-engine/issues/80)) ([344bf2d](https://github.com/sidevoice/sidevoice-engine/commit/344bf2dc3165615d591a66b23b706d7c63c5c9a4))
* **engine:** uninstall one build of a model ([#81](https://github.com/sidevoice/sidevoice-engine/issues/81)) ([2613c3a](https://github.com/sidevoice/sidevoice-engine/commit/2613c3a6706144d4dbb99ec53f42665a311d3d03))
* **engine:** voice activity detection (vad), streaming, with Silero on sherpa-onnx and transformers.js ([#71](https://github.com/sidevoice/sidevoice-engine/issues/71)) ([c75831e](https://github.com/sidevoice/sidevoice-engine/commit/c75831e37ad7be6b8da6039240ebc3ff3f7ef137))


### Bug Fixes

* night-sweep engine fixes (ENG-02, ENG-03, ENG-08, ENG-09) ([#83](https://github.com/sidevoice/sidevoice-engine/issues/83)) ([4a35618](https://github.com/sidevoice/sidevoice-engine/commit/4a35618a8dc8d4bc592596afdfc7de61aa572605))
* **release:** stage every npm version for the operator's approval, by trusted publishing ([#75](https://github.com/sidevoice/sidevoice-engine/issues/75)) ([7c0865c](https://github.com/sidevoice/sidevoice-engine/commit/7c0865c772e82bc1a292c271e8f5c53c97841a3c))

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
