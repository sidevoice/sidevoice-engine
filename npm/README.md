# @sidevoice/engine

The [Sidevoice](https://github.com/sidevoice) engine for the web: it knows a catalogue of local voice models, works
out which build of each fits the device, and takes the chosen one from absent to ready. This package is its
WebAssembly build (`wasm-bindgen --target web`).

```js
import init, { WebEngine } from "@sidevoice/engine";

await init();
const engine = await WebEngine.create(host); // host: { capabilities() }, see the types
engine.backends();
```

Source, documentation and issues: https://github.com/sidevoice/sidevoice-engine

Apache-2.0. Sidevoice is a trademark; see TRADEMARKS.md in the repository.
