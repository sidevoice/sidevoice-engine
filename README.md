<!-- Header: .github/assets/readme-header*.svg, from the Sidevoice brand's banner. Badges: shieldcn
     (https://shieldcn.dev), each a light/dark pair so the row follows the reader's GitHub theme. -->
<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/readme-header-on-dark.svg" />
  <img alt="Sidevoice — Give your coding agent a voice. Keep the conversation." src=".github/assets/readme-header.svg" width="750" />
</picture>

<p>
  <a href="https://github.com/sidevoice/sidevoice-engine/actions/workflows/release.yml"><picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/ci/sidevoice/sidevoice-engine.svg?variant=secondary&size=sm&workflow=release.yml&branch=main&mode=dark" /><img alt="release status" src="https://shieldcn.dev/github/ci/sidevoice/sidevoice-engine.svg?variant=secondary&size=sm&workflow=release.yml&branch=main&mode=light" /></picture></a>
  <a href="LICENSE"><picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/license/sidevoice/sidevoice-engine.svg?variant=secondary&size=sm&mode=dark" /><img alt="licence" src="https://shieldcn.dev/github/license/sidevoice/sidevoice-engine.svg?variant=secondary&size=sm&mode=light" /></picture></a>
  <picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/badge/status-skeleton.svg?variant=secondary&size=sm&mode=dark" /><img alt="status: skeleton" src="https://shieldcn.dev/badge/status-skeleton.svg?variant=secondary&size=sm&mode=light" /></picture>
</p>

# sidevoice-engine

Reading your coding agent's plans, diffs and summaries all day is tiring. **Sidevoice** turns the conversation you
already have with your agent into a voice call. The agent keeps its context and keeps writing as usual; it also
speaks its replies, and you answer by voice and can interrupt it — from the sofa or on a walk, not only at your desk.

**sidevoice-engine** is what runs voice models for Sidevoice: on the device itself, or on a provider's servers. It knows
one catalogue of local and remote models, works out which build of each fits on this machine, picks one per stage of
the voice pipeline, and takes it from absent to ready: installing, loading, and saying why when it cannot. A remote
model (OpenAI, ElevenLabs) is used through the same interface as a local one; the app supplies its key, the engine
never stores one.

## How it fits

| Piece | Role |
|---|---|
| **sidevoice-engine** (this repository) | Models, local and remote: the catalogue, which build fits here, the choice per stage and its lifecycle. |
| [sidevoice-core](https://github.com/sidevoice/sidevoice-core) | The conversations, next to the agents. The voice pipeline is leaving it for sidevoice-voice, built on this engine (sidevoice-core#89). |
| [sidevoice-connector](https://github.com/sidevoice/sidevoice-connector) | What you install on the machine where your agents run. It gives them their voice tools and runs the core. |
| [sidevoice-desktop](https://github.com/sidevoice/sidevoice-desktop) | The app you call from. |
| [sidevoice-web](https://github.com/sidevoice/sidevoice-web) | The call interface the app bundles; it can also be served as a static site. |

One Rust repository, one version. Native consumers (the desktop app, later the core as a provider for its own
machine) depend on the crate at a release's git tag and compile it themselves. The web gets a WebAssembly build,
published on npm as `@sidevoice/engine` for every release; every push to `main` also publishes a `nightly`
pre-release on GitHub, never on npm ([`RELEASING.md`](RELEASING.md)). To try a pull request's engine before it
merges, its CI keeps the npm package it built for 7 days, as the Actions artifact `engine-npm-<head sha>`
([`RELEASING.md`](RELEASING.md#a-pull-requests-package)).

The platform is injected: a `Host` gives the engine the machine's capabilities, its storage, a way to fetch files,
its HTTP for API calls, and the keys of remote providers. The engine ships the host of each kind of build, chosen like the backends at compile time: `NativeHost` in
every native build, and in the web build the page's, built by `WebEngine.create(host)` from what the page reports,
with the engine's own storage (OPFS, the browser's private file system), downloads and API calls (`fetch`). The
`Host` interface stays open, so tests and other platforms bring their own. The backends that run models are internal
to the engine and optional: which exist in a build is decided when it is compiled, whether they work on this machine
when it runs. Models are downloaded when they are needed, never bundled. Engine libraries are meant to be too; for
now, two backends are the exception: native builds link sherpa-onnx statically, through the official crate, and
whisper.cpp, through `whisper-rs` (sidevoice-engine#33 is loading them on demand).

## What a host must report

`Host::capabilities()` (`Capabilities`, in `src/host/capabilities.rs`) is what the host can see without trying
anything, gathered once when the host is built:

| Field | What to report | When it cannot tell |
|---|---|---|
| `runs` | `Native` in a process, `Page` in the wasm32 build | always known |
| `os`, `arch` | `std::env::consts::OS` and `ARCH` values. A page with nothing better: `"web"` and `"wasm32"` | always given |
| `accelerators` | what is present: `Cpu` natively, `Wasm` in a page, `Metal` and `CoreMl` on Apple, `Cuda` with an NVIDIA GPU, `WebGpu` when the page exposes `navigator.gpu`. Order does not matter | leave it out |
| `memory_mb` | memory available to models, in MB | `None` |
| `cores` | CPU cores | `None` |

- **The host reports, the backend confirms.** Whether a backend can actually use an accelerator (a CUDA driver, a
  granted WebGPU adapter, a Core ML model that loads) is found out by the backend's probe, never by the host. The
  probe can only narrow what the host reported: an accelerator the host left out is absent.
- **Unknown passes.** A requirement on memory or cores the host cannot tell is met: the engine does not reject a build
  on a guess; loading is what finds out. A host that is unsure of a number says `None` rather than guessing low.
- **The accelerator list is fixed.** `Accelerator` is an enum of the engine (adding one is a change here, since it
  needs backend code anyway), marked `#[non_exhaustive]`.
- **A JavaScript host** resolves `capabilities()` to `{ os, arch, accelerators: string[], memoryMb?, cores? }`, the
  accelerators by their stable ids: `cpu`, `cuda`, `coreml`, `metal`, `webgpu`, `wasm`. It is checked strictly: a
  missing or malformed field, an unknown id, or a `memoryMb` or `cores` that is not a positive whole number fails with
  `host-capabilities-<field>`. Leaving `memoryMb` or `cores` out (or `null`) is how a page says it cannot tell.

A build is offered on the first accelerator in its backend's order of preference that the host reports, the probe
confirms and the build allows: which accelerators a build runs on is its backend's to know, unless the build cannot
take some of them, which it says as a hard constraint (`"requires": {"accelerators": ["cpu"]}`).

## Using a model, and what stays in memory

A native app builds its host on a directory of its own and hands it to the engine. The native engine's futures expect a
**Tokio runtime** (the app's own, as sidevoice-core and the desktop app have): downloads go through `reqwest`, and
archives are unpacked on Tokio's blocking threads.

```rust
let host = NativeHost::new(app_data_dir.join("engine"))?;
let engine = Engine::new(Box::new(host), vec![Box::new(BundledCatalog)])?;
let models = engine.models().await?; // every model, its builds ranked, installed or not, the recommended build
let cancel = Cancel::new(); // `cancel.cancel()` from anywhere stops the install
let whisper = engine.load("whisper-small", None, &|progress: Progress| report(progress), &cancel).await?;
let text = whisper.as_stt().expect("speech to text").transcribe(&samples, 48_000, Some("es")).await?;
let kokoro = engine.load("kokoro-82m-v1.0", None, &|_| {}, &cancel).await?;
let tts = kokoro.as_tts().expect("text to speech");
let voices = tts.voices().await; // Voice { id, languages, gender? }
let audio = tts.speak("Hola", "ef_dora", Some("es"), None).await?; // Audio { samples, sample_rate }
let silero = engine.load("silero-vad", None, &|_| {}, &cancel).await?;
let mut mic = silero.as_vad().expect("voice activity").stream(VadOptions::default()).await?;
let heard = mic.accept(&pcm_at_16_khz).await?; // VadOutput { frames, events: [SpeechStart { at }, SpeechEnd { start, end }] }
let smart_turn = engine.load("smart-turn-v3.2", None, &|_| {}, &cancel).await?;
let p = smart_turn.as_end_of_turn().expect("end of turn").probability(&turn_so_far, 48_000).await?; // P(complete)
```

- **`Engine::models`** lists every model of the catalogue with its catalogue data, whether it is installed, every
  build ranked (those that run here first; each with its backend, the accelerator it would use, its precision, what
  it downloads, its memory, whether it runs here and why not, and whether it is installed) and the build the engine
  recommends. `Engine::install` and `Engine::uninstall` take a model id (and, to install, a build id or `None`);
  uninstalling removes each of the model's build folders, keeps any file another build's folder links, and refuses a
  model that is loaded (`model-in-use`).
- **`Engine::load`** installs the build if it is not (with `None`: an installed build that runs here, else the
  recommended one) and returns a `LoadedModel`: `as_stt()`, `as_tts()` and `as_vad()` are what it can do. Speech to
  text takes audio at any rate (the engine resamples it); text to speech returns `Audio` at the model's own rate. A
  model's voices are `Voice { id, languages, gender? }`, as the catalogue declares them where their source does.
- **Voice activity is a stream.** `Vad::stream(options)` opens a `VadStream` with a state of its own (several can run
  at once): it takes mono samples at the model's rate (`sample_rate()`, 16 kHz for Silero; it is not resampled) in
  pieces of any length, runs the model on each whole window (`window()`, 512 samples) and answers with a `VadFrame`
  per window (where it ends, whether the stream is in speech, and the model's probability where the backend tells it:
  transformers.js does, sherpa-onnx 1.13.8 segments inside its library and does not) and the `VadEvent`s:
  `SpeechStart { at }` when speech is confirmed, `SpeechEnd { start, end }` when it is over, in samples. `VadOptions`
  holds the threshold, `min_silence_ms` and `min_speech_ms` (sherpa-onnx's defaults: 0.5, 500, 250); every backend
  segments by sherpa-onnx's rules, so the events mean the same on each. `finish()` ends the speech in progress and
  starts over; `reset()` forgets it. A stream keeps its model in memory while it lives.
- **End of turn is a probability.** `EndOfTurn::probability(audio, sample_rate)` takes the turn so far (typically at a
  pause the voice activity detector found), keeps the last `seconds()` the model hears (8 for smart-turn) and brings
  them to 16 kHz, and returns the probability, from 0 to 1, that the speaker has finished. The model's input (Whisper's
  log-mel features, as smart-turn's own inference makes them) is made by the engine, the same on every backend.
- **Memory follows the `LoadedModel`s.** Loading a build that is already in memory returns it again; calls on one
  model wait for one another; the model is unloaded when the last `LoadedModel` of its build is dropped, and a
  backend's library is opened with the first of its models and closed after the last one. The engine keeps only weak
  references: no clock, no idle unloading.
- **`NativeHost`** (`src/host/native.rs`) takes one parameter, the data directory, which it creates. It reports `os`
  and `arch` from `std::env::consts`, `cores` from `available_parallelism`, the machine's memory (or its cgroup's
  limit) from `sysinfo`, and `Cpu`, plus `Metal` and `CoreMl` on macOS and `Cuda` where an NVIDIA driver is
  installed. It downloads over HTTPS with `reqwest` and rustls, the HTTP client sidevoice-core and the desktop app
  use, streaming each body as it arrives.
- **The installer does not know what a file is.** A build's files (catalogue) are one list of artifacts, checked as each
  arrives and only stored whole once it matches its SHA-256; `key` is only the name the backend finds it by.
- **Storage is laid out as Hugging Face's hub cache.** Each file is a blob, `blobs/<sha256>`, stored once whichever
  builds or versions use it, so it is never downloaded twice. Each build has its folder, `models/<build id>/`, where
  every file sits under its original name: its path in its Hugging Face repository (`onnx/model_q8.onnx`, as the
  hub's own snapshot folders keep it), a release asset's name, or its path inside its archive. So a backend whose
  engine expects a model directory, or looks at names, extensions and subfolders, finds what it expects; `load`
  gets those paths. Natively a folder's files are hard links to the blobs (no privilege needed, and on one volume,
  since everything is under the data directory); where the file system refuses a link, the file is copied, which
  costs the space twice and works the same. The browser's OPFS has no links: there each folder holds a copy of each
  file, made in `partial/` and moved into `models/` when the folder is committed, and `folders.json` lists which
  blobs each stored folder holds (writing it is what commits a folder, and it says whether a blob is still needed). A
  build is installed when its folder is stored, which happens only once every file is in it; its files stay when its
  model leaves memory. Uninstalling (`Engine::uninstall`) removes the folder, then each of its blobs no other folder
  links (natively, a blob whose link count is back to 1; where the platform does not tell the count, Windows in
  stable Rust, blobs are kept; on the web, a blob no folder in `folders.json` lists).
- **Archives.** A file entry may carry an `archive_path`: it is then a member of the archive at its `url` and
  `sha256` (a tar, bzip2-compressed or not, told apart by its bytes), a file or a directory. Several keys may share
  one archive (Kokoro: `kokoro.model` and `kokoro.data_dir`, espeak-ng's data, from one tarball). Each distinct
  archive is downloaded once, checked against its digest, and only then unpacked, once, into a blob tree,
  `blobs/<sha256>-unpacked`, whose members are linked into the build's folder at their paths; the archive itself is
  then removed. Unpacking takes directories and regular files only (no links of any kind), refuses any path that
  leaves the tree, and bounds the total size and the number of entries. The tar format is read by the `tar` crate,
  from storage, on a blocking thread. Archives come only with native-only builds (sherpa-onnx's release assets): the
  web build refuses them before downloading, with `archive-unsupported`.
- **Progress is a callback** (any `Fn(Progress)`): files done of all, and the bytes of the file being downloaded.
  **Cancelling** is a `Cancel` handle; dropping the future stops the install too. Neither leaves a partial file.

## Remote models and their keys

A remote model is a catalogue model like any other, with builds of a remote backend (`openai`, `elevenlabs`): it is
listed by `Engine::models`, installed, loaded and called through the same `LoadedModel` (`as_stt`, `as_tts`,
`voices`), and runs on the `remote` accelerator, which every host has.

- **Its build has no files**, only the provider's id of the model (`api_model`) and, in `call_params`, the request
  field a call's language goes in (OpenAI's `language`, ElevenLabs' `language_code`; none for a model that refuses
  one). Installing it only checks that the host has the provider's key (`credential-missing` otherwise), and stores
  nothing; it is `installed` while the host has the key. Uninstalling removes nothing.
- **Keys are the app's.** The engine asks the host for a provider's key each time it needs one (`Host::credentials`,
  the `Credentials` trait) and keeps it only for that call. A native app passes where its keys are:
  `NativeHost::new(dir)?.with_credentials(keychain)`, any `Credentials` (the OS keychain on desktop); without, the host
  has none. A page's host may have `credential(provider)`, returning (or resolving to) the key, or `null`, from the
  browser's storage.
- **Calls go through the host's HTTP** (`Host::http`, the `HttpClient` trait): `reqwest` natively, `fetch` on the web.
  Loading makes no call, except that an ElevenLabs text-to-speech model lists the account's voices. Speech comes back
  as 16-bit PCM at 24 kHz; a turn is sent as a 16-bit WAV at 16 kHz. Streaming is not used (sidevoice-engine#35).
- **What fails** has its codes: `credential-missing`, `credential-rejected` (401 or 403), `rate-limited` (429), the
  shared `transcription-failed`, `speech-failed`, `unknown-voice`, and the host's `request-failed` (no answer) and
  `credentials-failed` (the keys could not be read).

## Status

The catalogue, the installer, the lifecycle and the native host work, and so do four local backends and two remote
ones:

| Backend | Runs | On | Linked through |
|---|---|---|---|
| `sherpa-onnx` | speech to text with Whisper and NeMo transducers; text to speech with Kokoro, Piper and Supertonic; voice activity with Silero | the CPU, natively | the official `sherpa-onnx` crate (static ONNX Runtime) |
| `whisper-cpp` | speech to text with Whisper's ggml builds | Metal on Apple silicon, the CPU elsewhere (Windows compiles in principle, untested), natively | `whisper-rs` (whisper.cpp and ggml, built from source) |
| `onnxruntime` | end of turn with smart-turn v3 | the CPU, natively | `ort` (safe API, no runtime of its own) over the ONNX Runtime sherpa-onnx already links |
| `transformers-js` | speech to text with Whisper; text to speech with Kokoro (Spanish included, through eSpeak NG) and Supertonic 2; voice activity with Silero (its ONNX Runtime Web, run window by window); end of turn with smart-turn | WebGPU or WebAssembly, in the browser | the npm package's `@huggingface/transformers`, imported when a model loads |
| `openai` | speech to text with `gpt-4o-transcribe` and `gpt-4o-mini-transcribe`; text to speech with `gpt-4o-mini-tts` | OpenAI's servers, natively and in the browser | the host's HTTP (`/v1/audio/transcriptions`, `/v1/audio/speech`), with the app's key |
| `elevenlabs` | speech to text with Scribe v2; text to speech with Flash v2.5 and Multilingual v2, with the account's voices | ElevenLabs' servers, natively and in the browser | the host's HTTP (`/v1/speech-to-text`, `/v1/text-to-speech`), with the app's key |

In the browser the page's host stores files in OPFS and downloads them with `fetch`. MLX is a stub.

## Layout

```
src/            the crate sidevoice-engine, one package per concept (`x.rs` is the package, `x/` its parts):
  lib.rs          the front door: declares the packages, exports the public API
  host.rs         Host: the platform contract; host/: capabilities (what a host reports, and
                  capabilities/accelerator.rs), storage (Storage, StorageWriter, FolderWriter: blobs and build folders),
                  fetcher (Fetcher, Download), http (HttpClient: API calls), credentials (Credentials: the keys of
                  remote providers), native (NativeHost, native builds only: native/directory.rs, its storage, and
                  native/http.rs, its downloads and API calls)
  catalog.rs      CatalogSource, the merged catalogue and its check; catalog/: family, model (with model/build.rs
                  and model/capability.rs), bundled (the families compiled in)
  backend.rs      Backend and BackendSpec: the contract every backend implements, the ids a catalogue may name
                  (KNOWN) and BackendInfo (what Engine::backends lists); backend/: requirement, registry, library
                  (what open returns, which loads models), loaded_model (what load returns: SttModel, TtsModel,
                  VadModel and its streams), segmenter (speech from per-window probabilities, by sherpa-onnx's
                  rules), smart_turn (smart-turn's input, Whisper's log-mel features), remote (what the remote backends
                  share: the provider through the host, forms, PCM),
                  implementations/ (one file per backend)
  resolver.rs     the funnel; resolver/offer.rs, what it returns (an offer, or a rejection and its reason)
  install.rs      the installer (Artifact), which runs its steps; install/: plan (what is wanted, checked first),
                  download (one file fetched, verified and committed), archive (unpacking), progress (Progress,
                  ProgressSink), cancel (Cancel), digest (SHA-256)
  engine.rs       Engine: models, install, uninstall, load; engine/: model (Model, ModelBuild: what models lists),
                  loaded (LoadedModel, Stt, Tts; loaded/vad.rs: Vad, VadStream and their values; loaded/end_of_turn.rs:
                  EndOfTurn), audio (Audio,
                  resampling), memory (weak references: one library per backend, one model per build), error
                  (ConfigError)
  web.rs          the bridge to JavaScript, only in the wasm32 build (the npm package): WebEngine, LoadedModel, Stt,
                  Tts, Vad, VadStream, EndOfTurn; web/values.rs, the engine's values as JavaScript objects; web/opfs.rs, the browser's private
                  file system; web/host.rs, the JavaScript host (JsHost) as the engine sees it; web/host/:
                  capabilities (reading what it reports), storage (WebStorage, in OPFS), fetcher (WebFetcher, `fetch`, for
                  downloads and API calls)
  maybe_send.rs   Send/Sync in native builds only
catalog/        families/<family>.json, the bundled catalogue; pins written by `cargo xtask pin-catalog`
build.rs        the three cfg aliases: web, native, apple_silicon
npm/            the npm package's package.json and README, filled in by `cargo xtask npm`
xtask/          build tooling (`cargo xtask`), a package of its own
```

## Build and test

You need Rust 1.98.1 (the version `.github/actions/setup` installs), and a C compiler for the native build (rustls'
crypto, `ring`). A native build links two backends' engines statically; there is no build without them. A third, `onnxruntime`,
links none: it runs on the ONNX Runtime inside sherpa-onnx's libraries, through `ort` built with no runtime of its own.

- **whisper.cpp** is compiled, with ggml, from the sources `whisper-rs-sys` bundles, so the build needs CMake, a C++
  compiler and libclang (for `bindgen`, which writes the bindings). whisper.cpp tunes ggml for the building machine's
  CPU unless `GGML_NATIVE=OFF` is set in the build's environment, which an app that ships its binary to other machines
  should set.
- **sherpa-onnx** comes as prebuilt static libraries, which need the C++ standard library the platform's C++ toolchain
  provides (libstdc++ on Linux). On Linux x86_64 they use libstdc++'s old string ABI, so whisper.cpp must be compiled
  with it too, or the two copies of `std::regex`'s internals the linker merges disagree and ONNX Runtime aborts
  (`free(): invalid pointer`): this repository's `.cargo/config.toml` sets
  `CXXFLAGS_x86_64_unknown_linux_gnu = "-D_GLIBCXX_USE_CXX11_ABI=0"`, and an app that links both backends there needs
  the same in its own.

Where sherpa-onnx's libraries come from: `cargo xtask sherpa-libs` downloads the archive the `sherpa-onnx-sys` build
script would fetch for this machine (about 21 MB on Linux and macOS), checks it against the digest pinned in
`xtask/sherpa-onnx-libs.json`, unpacks its `lib/` into `~/.cache/sidevoice-engine/sherpa-onnx/` (or the directory
given) and prints that directory; `SHERPA_ONNX_LIB_DIR` makes the build link it and download nothing. CI does exactly
this (`.github/actions/setup`), so the build cache never holds the libraries. Without `SHERPA_ONNX_LIB_DIR`, the build
script downloads them itself, unchecked, into `target/sherpa-onnx-prebuilt/`.

The same command keeps the sherpa-onnx config fields a build's files may fill
(`src/backend/implementations/sherpa_onnx/config/fields.rs`): generated from the source of the crate version
Cargo.lock pins, every field that takes a file, by its path (`whisper.encoder`, `kokoro.data_dir`). Never edited by
hand; after bumping the crate, `--pin` writes it again, and `--check` fails until it is.

```sh
export SHERPA_ONNX_LIB_DIR="$(cargo xtask sherpa-libs)"
cargo xtask sherpa-libs --pin     # after changing the crate's version: the archives' digests (GitHub's) and the config fields
cargo xtask sherpa-libs --check   # what link-size.yml runs when the dependencies, the pins or the fields change
```

The native tests build and check this platform's backends; the one that downloads a real file through `NativeHost` is
ignored unless asked for, and CI asks:

```sh
cargo test --locked
cargo test --locked -- --include-ignored --skip sherpa_onnx::inference_tests --skip the_voice_loop   # with the network, as CI
```

The sherpa-onnx backend's tests that run real models download them first (about 250 MB, cached by digest in
`target/test-models/`, or in `SIDEVOICE_TEST_MODELS`), so plain `cargo test` skips them; CI runs them on each
native platform:

```sh
cargo test --locked --lib sherpa_onnx::inference_tests -- --ignored --nocapture
```

The whole voice loop is an integration test, `tests/voice_loop.rs`, and uses only the public API, as an app does:
`NativeHost`, the bundled catalogue, `Engine::models` (the builds the plan names), `Engine::load`, then the loaded
model's `as_tts` (`voices`, `speak`) and `as_stt` (`transcribe`). Each text-to-speech build of the plan
(`tests/voice_loop.json`) says a sentence in English or Spanish, each speech-to-text build of that language
transcribes it (Whisper base on sherpa-onnx and on whisper.cpp among them), real recorded clips are transcribed too,
and every transcript must stay within the plan's one word error rate, a loose 50%: the loop checks that the circuit
works and catches a wrong configuration, it does not measure quality. Each voice activity detector of the plan (Silero
on sherpa-onnx) then hears each recorded clip set between two seconds of silence, fed 20 ms at a time: every segment it
reports must lie in the clip, give or take 0.3 s, be ended by the silence after it, and together cover half the clip
(`tests/voice_loop/vad.rs`). Each end-of-turn model (smart-turn) hears each clip whole and cut inside a word (its loudest 20 ms in the middle), each followed by a
0.2 s pause, the moment silence alone would end the turn: it must call the whole clip complete (P ≥ 0.5) and the
cut one not (`tests/voice_loop/end_of_turn.rs`). It downloads about 1.5 GB the first time (kept by
digest in `$SIDEVOICE_VOICE_LOOP`), so it is ignored unless asked for; the `e2e` workflow runs it on Linux x86_64 and
arm64 and on macOS arm64 through `cargo xtask e2e`, which puts its table, and the accelerator each build was loaded
on, in the job's summary:

```sh
cargo test --locked --test voice_loop -- --ignored --nocapture
cargo xtask e2e [DIR]   # the same, keeping its files in DIR (target/voice-loop by default), as CI
```

The wasm32 tests run in Node and need the wasm32 target, Node.js and npm, and the wasm-bindgen CLI at the version of
`wasm-bindgen` in `Cargo.lock` on the `PATH` (`wasm-bindgen` and `wasm-bindgen-test-runner`, e.g.
`cargo install wasm-bindgen-cli --version <that version>`). The npm package is built and installed as a consumer
would, then run in Node, by the two `cargo xtask` commands:

```sh
cargo test --locked --target wasm32-unknown-unknown --lib
cargo xtask npm        # target/npm/sidevoice-engine-X.Y.Z.tgz
cargo xtask npm-smoke
cargo test --locked --manifest-path xtask/Cargo.toml   # the build tooling's own tests
```

The tests that need a page (OPFS, an HTTP server) are ignored in Node and run in a headless Chrome, through
ChromeDriver (`CHROMEDRIVER`, or `chromedriver` on the `PATH`, and `CHROME` for the Chrome it starts, of the same
version). The voice loop runs in Chrome too, through the npm package as a page uses it: Whisper base transcribes the
loop's recorded clips and hears Kokoro (Spanish) and Supertonic 2 back, and Silero finds the speech of each clip, and smart-turn tells each whole clip from a cut inside a word, by the
native loop's rules, on WebAssembly (`xtask/web-e2e.json`; a few
hundred MB downloaded on every run, into a profile that is thrown away; `CHROME`, else `google-chrome`):

```sh
cargo xtask test-browser
cargo xtask web-e2e [DIR]
```

The catalogue of models is data too: one file per family in `catalog/families/<family>.json`, compiled in
(`BundledCatalog`), three levels deep. A family has its `id`, the `architecture` its loader runs and its `source`; a
model, its `id`, `capabilities` (`stt`, `tts`, `vad`, `end-of-turn`), `parameters_m`, `languages` (none for a model that hears no
language in particular, as a voice activity detector) and `license`; a build, its `id`, the
`backend` that runs it, its `precision` (the format's own name for it, as the backend uses it: informational),
`requires` (hard constraints only, and optional: the only `accelerators` it
can take, WebGPU features, the WebAssembly cap), its `memory` (`mb`, with the `source` of the figure, `estimated`,
`declared` or `measured`, and its `basis`), and its `files`. Each file has the `key` the backend finds it by (for
sherpa-onnx, the path of the config field that receives it: `whisper.encoder`, `tokens`, `kokoro.data_dir`, ...,
checked by the backend's tests), a `url`, its `sha256` and its `bytes`; when the url is an archive, `archive_path` names the file or directory inside it, and
keys that name parts of one archive repeat its url, digest and size (it is downloaded and unpacked once). A
sherpa-onnx build may also have `call_params`: where each argument of a call goes in a config sherpa-onnx fixes when
the model is created, as the paths of the fields that take it (`{"language": "whisper.language"}`; Canary's
`["canary.src_lang", "canary.tgt_lang"]`). Its keys are checked against the calls' arguments when the catalogue is,
and its paths against the generated fields by the backend's tests; when a call's value changes, the model's
recognizer is made again, and the last two are kept. A url is
pinned to a revision (a Hugging Face commit); a GitHub release asset cannot be, so it is marked `mutable` and only its
digest pins it. A model lists every build that exists for it, for every backend the engine knows (`KNOWN`,
in `src/backend.rs`), whether or not this build of the engine implements that backend (the resolver rejects those with
`backend-not-in-this-build`). Each build lists exactly the files its format's reference implementation loads, no
more and no less (transformers.js for the web ONNX builds, mlx-audio for MLX, whisper.cpp for ggml, sherpa-onnx for
its exports), so a backend that lands later needs no catalogue change; a file that lives outside the model's
repository is pinned from its real source. Builds carry no order: ranking them is the resolver's. Reading is strict
(an unknown or a missing key fails the tests) and the merge of every source is checked by `Catalog::check`. Sizes,
digests and pinned revisions come from the Hugging Face and GitHub APIs, never by hand: a new file is written with its
`key` and a `url` (on Hugging Face at any revision, `…/resolve/main/…`), and then

```sh
cargo xtask pin-catalog            # pin every url, write sizes, digests and estimated memory
cargo xtask pin-catalog --check    # what CI runs when the catalogue changes
```

Estimated memory is the weights plus 30% (an archive counts once, at its size); a `declared` or `measured` figure is
never overwritten.

## How to add a backend

A backend is a record, an optional `probe` and an `open` whose library loads models; the engine does the rest for
every backend alike. The contract, with what each part must and must not do, is the documentation of
`src/backend.rs`. Which models it runs, and what they download, is the catalogue's to say (each build names its
backend). Backends are crate-private: nothing here touches the public API, except that `Engine::backends` lists the
new one. As an example, a Vosk backend (whisper.cpp's, `whisper_cpp.rs`, is a real one to compare with; it is linked
for now, see sidevoice-engine#33):

1. **Its file**, `src/backend/implementations/vosk.rs`. If its code cannot compile everywhere, the file
   starts with one `#![cfg(<alias>)]` from `build.rs` (`web`, `native`, `apple_silicon`), with a comment saying why,
   and does not exist elsewhere. If it links a library, that dependency goes under the same platform in `Cargo.toml`:
   the two must agree, or the file does not compile. Whatever else decides whether it runs (OS, GPU, drivers) is decided at run time.

   ```rust
   // Vosk's library is native here: its web build would be another backend.
   #![cfg(native)]
   ```

2. **Its `mod` line** in `src/backend/implementations.rs`: `mod vosk;`. Nothing else lists backends.

3. **Its record**, a `const` `BackendSpec`, and its registration. The `id` is what catalogue builds call it, never
   changes, and is in `KNOWN` (`src/backend.rs`), where an id goes as soon as the catalogue names it, code or not.
   Its `name`, `description` (one sentence, in English: not UI) and `upstream` say
   what it is. Accelerators go best first; requirements hold for any model (a build's memory is the catalogue's).
   The engine ships `MinMemoryMb` and `MinCores`; a check of its own is a `Requirement` next to its file.

   ```rust
   struct Vosk;

   const SPEC: BackendSpec = BackendSpec {
       id: "vosk",
       name: "Vosk",
       description: "Kaldi speech recognition, offline.",
       upstream: "https://github.com/alphacep/vosk-api",
       accelerators: &[Accelerator::Cuda, Accelerator::Cpu],
       requirements: &[],
   };

   inventory::submit! { BackendFactory(|| Box::new(Vosk)) }
   ```

4. **Its `probe`, only if needed.** The host reports what is present; the default keeps the declared accelerators
   it reports, which is enough for most backends. Override it only when trying is the only way to know whether the
   backend can use one (here, whether a CUDA driver loads): it narrows the declared accelerators the host reported,
   best first, quickly, without its library or a model, and answers "no" instead of failing. The engine caches the
   answer.

5. **Its `open` and its library's `load`**: called only for the chosen build, once its files are installed. `open`
   opens the backend's library at run time (natively, its C API declared in Rust and the library opened with
   `libloading`; on the web, its JavaScript module, with a dynamic import) and returns it as a `Library`. Where a
   downloaded library's files are declared is sidevoice-engine#33: today every backend's library comes with the
   engine. The engine keeps one per backend while any of its models is in memory: it calls `open` for the first model
   of the backend, and drops the library once the app has dropped the last one. The library's `load` loads the
   model's files on the accelerator it is given, and returns a `BackendModel` that transcribes, speaks, or both.
   Neither downloads anything, reads anything outside `files` or keeps state in the backend, and both fail with a
   stable error code, never a sentence. Speech comes back as one buffer with its sample rate.

   ```rust
   #[cfg_attr(native, async_trait)]
   #[cfg_attr(web, async_trait(?Send))]
   impl Backend for Vosk {
       fn spec(&self) -> &BackendSpec {
           &SPEC
       }

       async fn open(&self, files: &Installed) -> Result<Box<dyn Library>> {
           // Open the library with libloading.
       }
   }

   #[cfg_attr(native, async_trait)]
   #[cfg_attr(web, async_trait(?Send))]
   impl Library for VoskLibrary {
       async fn load(
           &self,
           build: &BuildEntry,
           accelerator: Accelerator,
           files: &Installed,
       ) -> Result<Box<dyn BackendModel>> {
           // The model's files from `files`, on `accelerator`.
       }
   }
   ```

6. **Its place in the tests**: add its id to the expected lists of `src/backend/tests.rs`, for the platforms where
   it is compiled in. The tests there also check that it probes on the fake host without loading anything, and that
   every backend of the build is in `KNOWN` and describes itself.

## Contributing

Issues and pull requests are welcome. Read [`AGENTS.md`](AGENTS.md) first: it holds the rules for code, texts and
tests, for people and coding agents alike. Pull request titles follow
[Conventional Commits](https://www.conventionalcommits.org) (CI checks them) and become the squashed commit, from
which release notes are written ([`RELEASING.md`](RELEASING.md)).

## Licence

[Apache-2.0](LICENSE). The Sidevoice name and logo are trademarks: forks are welcome under their own name — see
[`TRADEMARKS.md`](TRADEMARKS.md).
