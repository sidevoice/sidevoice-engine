# AGENTS.md

Rules for any coding agent (and person) working in this repository.

## Language of the code and of the product

- Code, identifiers, comments, commit messages and docs are in **English**.
- **What the engine tells people carries keys.** Why a model was rejected, why it cannot be installed or loaded, a
  failure a person will read: the engine says it as a stable code with its parameters, never as a finished
  sentence. The app translates by code through its per-language message bundles; the engine's English text for each
  code is the fallback when a bundle lacks it, and is never parsed. Adding such a reason means adding its code and
  its English text. No other language, Spanish included, is ever hard-coded, and nothing here picks a language for
  the person.
- Logs and developer-facing errors are English; they are not UI.

## Before changing things

Read `README.md` (what the engine is and where things are) and `RELEASING.md` (how it is versioned and released).
The repository is Rust only: one crate (`src/`), and the build tooling `cargo xtask` (`xtask/`).

- **Models come from catalogues, one interface** (sidevoice-engine#63; `Engine::catalogs`, `Catalog`: an id, a status,
  its models by capability, `refresh`, `load`). The local catalogue (files, families, builds, backends, accelerators,
  install; its builds also through `Engine::install`, `uninstall` and `load`) is one; each remote provider
  (`src/provider/`: OpenAI, ElevenLabs) is another; an engine on the host would be one more. A provider is not a
  backend: it has no builds, no install and no accelerator, and its models are not in the local catalogue. Each
  provider is one file that registers itself, as backends do; it reads its official OpenAPI spec at run time through
  the host, derives from it what the API does not say (which models are speech to text, where the language goes, speed
  ranges, fixed voices), lists its models live, and the engine keeps spec facts and listing in memory only, with the
  provider's own status. Nothing about a remote model is written by hand or generated into the repository.
- **One set of capability interfaces** (`src/capability.rs`: `Stt`, `Tts`, `Vad`, `EndOfTurn`), outside the
  catalogues: a `LocalModel` and a `RemoteModel` (either is a catalogue's `LoadedModel`) hand out the same ones, and
  transcribing with Whisper or with OpenAI is the same call. Code that uses the engine works against them.
- **Keys come from the host, never from the engine or the catalogue.** The app keeps them (the OS keychain on
  desktop, the browser's storage on the web) and the host hands one over for each call (`Host::credentials`); the
  engine stores none, and sidevoice-core holds none. The call goes through the host's HTTP (`Host::http`).
- **The platform is injected** through `Host` (capabilities, storage, fetching, API calls, keys). The engine ships the host of each
  kind of build, chosen by the same aliases as the backends (`src/host/native.rs`, `NativeHost`; the page's,
  `src/web/host.rs`), and the interface stays replaceable: the tests bring fake hosts. Only a host touches the file system, the
  network or the browser; the rest of the engine goes through `Host`.
- **Backends are internal, optional and lazy.** Which exist in a build is a compile-time decision (the cfg aliases
  `web`, `native`, `apple_silicon` from `build.rs`); a backend file carries a single `#![cfg(alias)]` only when its
  library cannot compile elsewhere, and registers itself with `inventory`. A backend describes itself as data
  (`BackendSpec`: accelerators in order of preference, requirements as checks); the engine matches, ranks, selects
  and installs for every backend alike. Which models a backend runs is the catalogue's to say, and what it downloads
  is data too, never code. A backend's code only checks its accelerators for real when the default `probe()` is not
  enough, and loads a model. Loading a model installs and loads only the chosen build.
- **Everything is closed by default.** Each item gets the narrowest visibility that works: private, then
  `pub(super)` or `pub(crate)`, and `pub` only for what consumers of the crate actually need. The public API is
  deliberate: opening something later is cheap, closing it later is a breaking change. Backends, for instance, are
  crate-private; if plugins ever need to add backends, that part of the contract is opened then, on purpose.
- **Nothing heavy is linked into the app.** Engine libraries and models are downloaded on demand. Two exceptions, for
  now: the sherpa-onnx backend links the official `sherpa-onnx` crate statically (ONNX Runtime included), and the
  whisper.cpp backend links `whisper-rs` (whisper.cpp and ggml, compiled from source), each in native builds only, so
  neither downloads anything of its own. Making them load on demand is sidevoice-engine#33; no other backend links
  its engine. The `onnxruntime` backend links none: it runs on the ONNX Runtime already inside sherpa-onnx's
  libraries, through `ort` with `ort-sys`'s `disable-linking` (no runtime of its own, and `ort`'s ordinary
  initialization: the engine installs no process-wide API), so it adds only `ort`'s glue (about 0.1–0.2 MB, measured by
  the link-size job). When runtimes load on demand, it takes its runtime from there.

## Code layout

How the crate (`src/`) is laid out; `README.md` maps where each package is.

- **One package per concept.** `x.rs` is the module and holds its main type or trait, the interface at the module root
  (never `x/x.rs`: Clippy's `module_inception`); `x/` holds its parts, one small file per piece.
- **A file is one concept**, and it keeps that concept's closely related types together, even when other modules use
  them: as `std::io::Error` lives with `ErrorKind`, `Rejection` lives with `Reason`, and `Backend` with
  `BackendSpec`. A type gets its own file only when it is a concept of its own, not a part or a detail of another
  (`Accelerator`, `BuildEntry`, `Voice`). Split by cohesion and size, never one type per file: the module is the unit.
- **Names say what a thing is**: `requirement.rs` (what the machine must meet), not `checks.rs`;
  `loaded_model.rs`; `SttModel` and `TtsModel`, after the catalogue's `Capability`. Name by role, never by state:
  `WebStorage`, not `Unimplemented`; that something is not implemented yet, a stub or a placeholder is said in its
  docs and its `not-implemented` error code, not in its name.
- **Unit tests live in `x/tests.rs`** beside their module (`#[cfg(test)] mod tests;`), never inline. Test doubles
  shared between modules are in `src/test_support.rs`, compiled in test builds only. Tests that must run on wasm32 are
  unit tests: an rlib linked into an integration test loses its `inventory` registrations there
  (`src/backend/registry.rs`).
- **Platform conditions use the crate's aliases** (`web`, `native`, `apple_silicon`, from `build.rs`), except in a test
  that checks the aliases themselves. A crate-level module's condition goes on its declaration (`#[cfg(web)] mod web;`);
  a backend file carries its own `#![cfg]`.
- **Closed by default**, as above.
- **What is data stays data**: versions, URLs and digests live in the catalogue, not in code, and are validated strictly
  when read: an unknown or a missing key is an error.

## How work lands

- Pull request titles are [Conventional Commits](https://www.conventionalcommits.org) (CI checks them); a PR is
  squash-merged and its title becomes the commit, from which release notes are written (`RELEASING.md`).
- Commits are signed.
- Logic lives in `cargo xtask`, not in workflow YAML: a workflow sets up the machine and calls one `cargo xtask`
  command, which runs the same on a laptop.
- Third-party actions are pinned by commit SHA, with the version in a comment.
