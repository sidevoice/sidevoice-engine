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

**sidevoice-engine** is what runs voice models on the device itself. It knows a catalogue of local models, works out
which build of each fits on this machine, picks one per stage of the voice pipeline, and takes it from absent to
ready: installing, loading, and saying why when it cannot. It has no remote providers: to the rest of Sidevoice, the
device is one more provider.

## How it fits

| Piece | Role |
|---|---|
| **sidevoice-engine** (this repository) | Local models: the catalogue, which build fits here, the choice per stage and its lifecycle. |
| [sidevoice-core](https://github.com/sidevoice/sidevoice-core) | The conversations and the voice pipeline, next to the agents; it keeps the remote providers. |
| [sidevoice-connector](https://github.com/sidevoice/sidevoice-connector) | What you install on the machine where your agents run. It gives them their voice tools and runs the core. |
| [sidevoice-desktop](https://github.com/sidevoice/sidevoice-desktop) | The app you call from. |
| [sidevoice-web](https://github.com/sidevoice/sidevoice-web) | The call interface the app bundles; it can also be served as a static site. |

One Rust repository, one version. Native consumers (the desktop app, later the core as a provider for its own
machine) depend on the crate at a release's git tag and compile it themselves. The web gets a WebAssembly build,
published on npm as `@sidevoice/engine` for every release; every push to `main` also publishes a `nightly`
pre-release on GitHub, never on npm ([`RELEASING.md`](RELEASING.md)).

The platform is injected: a `Host` gives the engine the machine's capabilities, its storage and a way to fetch
files. The engine ships the host of each kind of build, chosen like the backends at compile time: `NativeHost` in
every native build, the browser's to come in the web build (#8). The `Host` interface stays open, so tests and other
platforms bring their own. The backends that run models are internal
to the engine and optional: which exist in a build is decided when it is compiled, whether they work on this machine
when it runs. Models are downloaded when they are needed, never bundled. Engine libraries are meant to be too; for
now, the sherpa-onnx backend is the exception: native builds link it statically, through the official crate, behind
the default `sherpa-onnx` feature (sidevoice-engine#33 is loading it on demand again).

## What a host must report

`Host::capabilities()` (`Capabilities`, in `src/host/capabilities.rs`) is what the host can see without trying
anything, gathered once when the host is built:

| Field | What to report | When it cannot tell |
|---|---|---|
| `runs` | `Native` in a process, `Page` in the wasm32 build | always known |
| `os`, `arch` | `std::env::consts::OS` and `ARCH` values; natively they pick the `backends.json` platform, so an unknown pair gets `no-runtime-for-platform`. A page with nothing better: `"web"` and `"wasm32"` | always given |
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

## Installing, and what stays in memory

A native app builds its host on a directory of its own and hands it to the engine. The native engine's futures expect a
**Tokio runtime** (the app's own, as sidevoice-core and the desktop app have): downloads go through `reqwest`, and
archives are unpacked on Tokio's blocking threads.

```rust
let host = NativeHost::new(app_data_dir.join("engine"))?;
let engine = Engine::new(Box::new(host), sources)?;
let selection = engine.select(Capability::Stt, &Preferences::default()).expect("something fits");
let cancel = Cancel::new(); // `cancel.cancel()` from anywhere stops the install
let handle = engine.prepare(&selection, &|progress: Progress| report(progress), &cancel).await?;
```

- **`NativeHost`** (`src/host/native.rs`) takes one parameter, the data directory, which it creates. It reports `os`
  and `arch` from `std::env::consts`, `cores` from `available_parallelism`, the machine's memory (or its cgroup's
  limit) from `sysinfo`, and `Cpu`, plus `Metal` and `CoreMl` on macOS and `Cuda` where an NVIDIA driver is
  installed. It downloads over HTTPS with `reqwest` and rustls, the HTTP client sidevoice-core and the desktop app
  use, streaming each body as it arrives.
- **The installer does not know what a file is.** A build's model files (catalogue) and its backend's files
  (`backends.json`) are one list of artifacts, checked as each arrives and only stored whole once it matches its
  SHA-256; `key` is only the name the backend finds it by.
- **Storage is laid out as Hugging Face's hub cache.** Each file is a blob, `blobs/<sha256>`, stored once whichever
  builds or versions use it, so it is never downloaded twice. Each build has its folder, `models/<build id>/`, where
  every file sits under its original name (the last segment of its URL, or its path inside its archive), so a
  backend whose engine expects a model directory, or looks at names and extensions, finds what it expects; `load`
  gets those paths. Natively a folder's files are hard links to the blobs (no privilege needed, and on one volume,
  since everything is under the data directory); where the file system refuses a link, the file is copied, which
  costs the space twice and works the same. The browser's OPFS has no links: there the folder will hold a copy of
  each file, and the host will keep which folders use each blob. A build is installed when its folder is stored,
  which happens only once every file is in it; its files stay when its model leaves memory. Uninstalling
  (`Installer::uninstall`) removes the folder, then each of its blobs no other folder links (natively, a blob whose
  link count is back to 1; where the platform does not tell the count, Windows in stable Rust, blobs are kept).
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
- **A build's state** is `Engine::state(&build)`: `Absent → Installing → Installed → Loading → Ready`, or `Failed`
  with its code (`digest-mismatch`, `download-failed`, `storage-failed`, ...).
- **Memory follows use.** A model unused for 10 minutes (`DEFAULT_IDLE_UNLOAD`, or `Engine::with_idle_unload`) is
  unloaded, and its build is `Installed` again. A backend's library is opened with the first of its models and closed
  after the last one is unloaded. The engine has no timer of its own: it unloads idle models on each `prepare` and
  when the app calls `Engine::unload_idle`, which it should do on a schedule (once a minute is plenty).

## Status

The catalogue, the installer, the lifecycle and the native host work, and so does the first real backend:
sherpa-onnx, natively (speech to text with Whisper and NeMo transducers; text to speech with Kokoro, Piper and
Supertonic), linked through the official crate. MLX and transformers.js are stubs, and the browser's host is not
bridged yet.

## Layout

```
src/            the crate sidevoice-engine, one package per concept (`x.rs` is the package, `x/` its parts):
  lib.rs          the front door: declares the packages, exports the public API
  host.rs         Host: the platform contract; host/: capabilities (what a host reports, and
                  capabilities/accelerator.rs), platform (which platform that is), storage (Storage, StorageWriter,
                  FolderWriter: blobs and build folders), fetcher (Fetcher, Download), native (NativeHost, native
                  builds only: native/directory.rs, its storage, and native/http.rs, its downloads)
  catalog.rs      CatalogSource, the merged catalogue and its check; catalog/: family, model (with model/build.rs
                  and model/capability.rs), bundled (the families compiled in)
  backend.rs      Backend and BackendSpec: the contract every backend implements; backend/: runtime (its files:
                  the lookup, and runtime/schema.rs, the shape of backends.json), requirement, registry,
                  library (what open returns, which loads models), loaded_model (what load returns: SttModel,
                  TtsModel), implementations/ (one file per backend)
  resolver.rs     the funnel; resolver/offer.rs, what it returns (an offer, or a rejection and its reason)
  install.rs      the installer (Artifact); install/: progress (Progress, ProgressSink), cancel (Cancel),
                  digest (SHA-256)
  engine.rs       Engine: puts it together; engine/: selection (Preferences, Selection), error (ConfigError),
                  lifecycle (BuildState), memory (what is loaded, and unloading what goes unused)
  web.rs          the bridge to JavaScript, only in the wasm32 build (the npm package): WebEngine; web/host.rs,
                  the JavaScript host (JsHost) as the engine sees it; web/host/: capabilities (reading what it
                  reports), storage (WebStorage), fetcher (WebFetcher)
  maybe_send.rs   Send/Sync in native builds only
backends.json   each backend's runtime files per platform, compiled in; digests written by `cargo xtask pin-backends`
catalog/        families/<family>.json, the bundled catalogue; pins written by `cargo xtask pin-catalog`
build.rs        the three cfg aliases: web, native, apple_silicon
npm/            the npm package's package.json and README, filled in by `cargo xtask npm`
xtask/          build tooling (`cargo xtask`), a package of its own
```

## Build and test

You need Rust 1.98.1 (the version `.github/actions/setup` installs), and a C compiler for the native build (rustls'
crypto, `ring`). A native build links sherpa-onnx statically, and its prebuilt static libraries need the C++ standard
library the platform's C++ toolchain provides (libstdc++ on Linux). `--no-default-features` leaves the backend out.

Where those libraries come from: `cargo xtask sherpa-libs` downloads the archive the `sherpa-onnx-sys` build script
would fetch for this machine (about 21 MB on Linux and macOS), checks it against the digest pinned in
`xtask/sherpa-onnx-libs.json`, unpacks its `lib/` into `~/.cache/sidevoice-engine/sherpa-onnx/` (or the directory
given) and prints that directory; `SHERPA_ONNX_LIB_DIR` makes the build link it and download nothing. CI does exactly
this (`.github/actions/setup`), so the build cache never holds the libraries. Without `SHERPA_ONNX_LIB_DIR`, the build
script downloads them itself, unchecked, into `target/sherpa-onnx-prebuilt/`.

```sh
export SHERPA_ONNX_LIB_DIR="$(cargo xtask sherpa-libs)"
cargo xtask sherpa-libs --pin     # after changing the crate's version: write the archives' digests from GitHub's
cargo xtask sherpa-libs --check   # what link-size.yml runs when the dependencies or the pins change
```

The native tests build and check this platform's backends; the one that downloads a real file through `NativeHost` is
ignored unless asked for, and CI asks:

```sh
cargo test --locked
cargo test --locked -- --include-ignored --skip sherpa_onnx::inference_tests   # with the network, as CI
```

The sherpa-onnx backend's tests that run real models download them first (about 250 MB, cached by digest in
`target/test-models/`, or in `SIDEVOICE_TEST_MODELS`), so plain `cargo test` skips them; CI runs them on each
native platform:

```sh
cargo test --locked --lib sherpa_onnx::inference_tests -- --ignored --nocapture
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

A backend's files are data, in `backends.json`: one `version` per backend and, per platform, the files to download,
each with its `url` (which may say `{version}`) and `sha256`. Every backend lists all six platforms (`macos-aarch64`,
`macos-x86_64`, `linux-x86_64`, `linux-aarch64`, `windows-x86_64`, `web`): `null` where it does not run, which the
engine rejects with `no-runtime-for-platform`, and `[]` where it runs and downloads nothing. A missing or unknown
platform fails the build's tests. Digests are never typed by hand: after changing a version or a file,

```sh
cargo xtask pin-backends           # download every file at its pinned version and write its sha256
cargo xtask pin-backends --check   # what CI runs when backends.json changes
```

The catalogue of models is data too: one file per family in `catalog/families/<family>.json`, compiled in
(`BundledCatalog`), three levels deep. A family has its `id`, the `architecture` its loader runs and its `source`; a
model, its `id`, `capabilities` (`stt`, `tts`), `parameters_m`, `languages` and `license`; a build, its `id`, the
`backend` that runs it, its `precision` (the format's own name for it, as the backend uses it: informational),
`requires` (hard constraints only, and optional: the only `accelerators` it
can take, WebGPU features, the WebAssembly cap), its `memory` (`mb`, with the `source` of the figure, `estimated`,
`declared` or `measured`, and its `basis`), and its `files`. Each file has the `key` the backend finds it by (for
sherpa-onnx, the path of the config field that receives it: `whisper.encoder`, `tokens`, `kokoro.data_dir`, ...,
checked by the backend's tests), a `url`, its `sha256` and its `bytes`; when the url is an archive, `archive_path` names the file or directory inside it, and
keys that name parts of one archive repeat its url, digest and size (it is downloaded and unpacked once). A url is
pinned to a revision (a Hugging Face commit); a GitHub release asset cannot be, so it is marked `mutable` and only its
digest pins it. A model lists every build that exists for it, for every backend `backends.json` declares,
whether or not this build of the engine implements that backend (the resolver rejects those with
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
`src/backend.rs`. Which models it runs is the catalogue's to say (each build names its backend), and what it
downloads is data: its entry in `backends.json`, read by `src/backend/runtime.rs`. Backends are crate-private:
nothing here touches the public API. As an example, whisper.cpp:

1. **Its file**, `src/backend/implementations/whisper_cpp.rs`. If its code cannot compile everywhere, the file
   starts with one `#![cfg(<alias>)]` from `build.rs` (`web`, `native`, `apple_silicon`), with a comment saying why,
   and does not exist elsewhere. Whatever else decides whether it runs (OS, GPU, drivers) is decided at run time.

   ```rust
   // whisper.cpp is a native library: there is no web build of it.
   #![cfg(native)]
   ```

2. **Its `mod` line** in `src/backend/implementations.rs`: `mod whisper_cpp;`. Nothing else lists backends.

3. **Its record**, a `const` `BackendSpec`, and its registration. The `id` is what catalogue builds and
   `backends.json` call it, and never changes. Accelerators go best first; requirements hold for any model (a
   build's memory is the catalogue's). The engine ships `MinMemoryMb` and `MinCores`; a check of its own is a
   `Requirement` next to its file.

   ```rust
   struct WhisperCpp;

   const SPEC: BackendSpec = BackendSpec {
       id: "whisper-cpp",
       accelerators: &[Accelerator::Cuda, Accelerator::Metal, Accelerator::Cpu],
       requirements: &[],
   };

   inventory::submit! { BackendFactory(|| Box::new(WhisperCpp)) }
   ```

4. **Its `probe`, only if needed.** The host reports what is present; the default keeps the declared accelerators
   it reports, which is enough for most backends. Override it only when trying is the only way to know whether the
   backend can use one (here, whether a CUDA driver loads): it narrows the declared accelerators the host reported,
   best first, quickly, without its library or a model, and answers "no" instead of failing. The engine caches the
   answer.

5. **Its `open` and its library's `load`**: called only for the selected build, once its files are installed.
   `open` opens the backend's library at run time (natively, its C API declared in Rust and the library opened with
   `libloading` from the file its `backends.json` entry names; on the web, its JavaScript module, with a dynamic
   import) and returns it as a `Library`. The engine keeps one per backend while any of its models is in memory: it
   calls `open` for the first model of the backend, and drops the library after the last one is unloaded. The
   library's `load` loads the model's files on the accelerator it is given, and returns a `LoadedModel` that
   transcribes, speaks, or both. Neither downloads anything, reads anything outside `files` or keeps state in the
   backend, and both fail with a stable error code, never a sentence. Speech comes back as one buffer with its sample
   rate.

   ```rust
   #[cfg_attr(native, async_trait)]
   #[cfg_attr(web, async_trait(?Send))]
   impl Backend for WhisperCpp {
       fn spec(&self) -> &BackendSpec {
           &SPEC
       }

       async fn open(&self, files: &Installed) -> Result<Box<dyn Library>> {
           // Open the library from `files` with libloading.
       }
   }

   #[cfg_attr(native, async_trait)]
   #[cfg_attr(web, async_trait(?Send))]
   impl Library for WhisperCppLibrary {
       async fn load(
           &self,
           build: &Build,
           accelerator: Accelerator,
           files: &Installed,
       ) -> Result<Box<dyn LoadedModel>> {
           // The model's files from `files`, on `accelerator`.
       }
   }
   ```

6. **Its entry in `backends.json`**, which `src/backend/runtime.rs` reads; its shape is
   `src/backend/runtime/schema.rs`, strict (an unknown or a missing key fails the tests), and *Build and test* above
   says how the platforms and digests work. The entry has its `id`, a name and description, its `upstream` and one
   `version`, and all six platforms: `null` where it does not run, `[]` where it runs and downloads nothing, or the
   files, each with the `name` that `open` finds it by in `files`, a `url` that may say `{version}`, a `sha256`
   that `cargo xtask pin-backends` writes, and, for a member of an archive, its optional `archive_path` inside it
   (which may say `{version}` too). A backend the catalogue names before its code exists (whisper.cpp, today)
   already has an entry, all `null`, which is filled in then.

   ```json
   {
     "id": "whisper-cpp",
     "name": "whisper.cpp",
     "description": "Speech recognition: the shared library.",
     "upstream": "https://github.com/ggml-org/whisper.cpp",
     "version": "…",
     "platforms": {
       "macos-aarch64": [{ "name": "library", "url": "https://…/v{version}/…-macos-arm64.zip", "sha256": "…" }],
       "macos-x86_64": [{ "name": "library", "url": "https://…/v{version}/…-macos-x64.zip", "sha256": "…" }],
       "linux-x86_64": [{ "name": "library", "url": "https://…/v{version}/…-linux-x64.zip", "sha256": "…" }],
       "linux-aarch64": [{ "name": "library", "url": "https://…/v{version}/…-linux-arm64.zip", "sha256": "…" }],
       "windows-x86_64": [{ "name": "library", "url": "https://…/v{version}/…-windows-x64.zip", "sha256": "…" }],
       "web": null
     }
   }
   ```

7. **Its place in the tests**: add its id to the expected lists of `src/backend/tests.rs`, for the platforms where
   it is compiled in. The tests there also check that it probes on the fake host without loading anything, and that
   every backend of the build has its `backends.json` entry.

## Contributing

Issues and pull requests are welcome. Read [`AGENTS.md`](AGENTS.md) first: it holds the rules for code, texts and
tests, for people and coding agents alike. Pull request titles follow
[Conventional Commits](https://www.conventionalcommits.org) (CI checks them) and become the squashed commit, from
which release notes are written ([`RELEASING.md`](RELEASING.md)).

## Licence

[Apache-2.0](LICENSE). The Sidevoice name and logo are trademarks: forks are welcome under their own name — see
[`TRADEMARKS.md`](TRADEMARKS.md).
