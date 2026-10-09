<!-- npm shows this file on the package page. Images by absolute URL: npm serves no file of the repository. The
     light/dark pair as in the repository's README; where the page ignores <picture>, the light one shows. -->
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/sidevoice/sidevoice-engine/main/.github/assets/readme-header-on-dark.svg" />
  <img alt="Sidevoice — Give your coding agent a voice. Keep the conversation." src="https://raw.githubusercontent.com/sidevoice/sidevoice-engine/main/.github/assets/readme-header.svg" width="750" />
</picture>

# @sidevoice/engine

The [Sidevoice](https://github.com/sidevoice) engine for the web: it knows a catalogue of local voice models, works
out which build of each fits the device, and takes the chosen one from absent to ready. This package is its
WebAssembly build (`wasm-bindgen --target web`).

```js
import init, { WebEngine } from "@sidevoice/engine";

await init();
// The page says what it has; the engine keeps its files in OPFS and downloads them with fetch.
const engine = await WebEngine.create({
  async capabilities() {
    return { os: "web", arch: "wasm32", accelerators: navigator.gpu ? ["webgpu", "wasm"] : ["wasm"] };
  },
  credential(provider) { // optional: a remote provider's key ("openai", "elevenlabs"), or null
    return localStorage.getItem(`key:${provider}`);
  },
});
const local = engine.catalog("local");
const models = await local.models(); // each { kind: "local", ... } with its builds ranked, whether it is installed
const whisper = await engine.load("whisper-small", undefined, (progress) => show(progress), abort.signal);
const stt = whisper.asStt();
const text = await stt.transcribe(samples, 48000, "es"); // any rate: the engine resamples
const kokoro = await engine.load("kokoro-82m-v1.0");
const { samples: speech, sampleRate } = await kokoro.asTts().speak("Hola", "ef_dora", "es");
const silero = await engine.load("silero-vad");
const mic = await silero.asVad().stream({ minSilenceMs: 500 }); // a state of its own; mic.sampleRate is 16000
const { frames, events } = await mic.accept(pcm); // events: { type: "speech-start", at } | { type: "speech-end", start, end }
const smartTurn = await engine.load("smart-turn-v3.2");
const p = await smartTurn.asEndOfTurn().probability(turnSoFar, 48000); // the probability the speaker has finished
stt.free(); // the model leaves memory once its LocalModel and the Stt and Tts it handed out are freed
whisper.free(); // (or collected)

// Every catalogue alike: "local", then each remote provider, listed live with the app's keys, kept in memory only.
for (const catalog of engine.catalogs()) {
  const { reason, stale, detail } = await catalog.status(); // reason absent when current
  const speakers = await catalog.models("tts"); // { kind: "local", builds, ... } or { kind: "remote", speed?, ... }
}
const elevenlabs = engine.catalog("elevenlabs");
await elevenlabs.refresh(); // read again now
const scribe = await elevenlabs.load("scribe_v2"); // a RemoteModel: the same asStt(), asTts()
const heard = await scribe.asStt().transcribe(samples, 48000, "es");
```

Every promise rejects with an `Error` carrying the engine's stable `code` and its `params`, for the page to
translate. Models run through [transformers.js](https://github.com/huggingface/transformers.js), a dependency of
this package that the engine imports only when a model is loaded; Kokoro's phonemes come from eSpeak NG (the
`espeak-ng` package), under the **GPL-3.0-or-later**: see `THIRD_PARTY_NOTICES.md`.

Source, documentation and issues: https://github.com/sidevoice/sidevoice-engine

Apache-2.0. Sidevoice is a trademark; see TRADEMARKS.md in the repository.
