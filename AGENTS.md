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

- **The engine has no remote providers.** Those stay in sidevoice-core; to the core, the device is one more provider.
- **The platform is injected** through `Host` (capabilities, storage, fetching), implemented by each platform outside
  this repository; here there are only the fake hosts of the tests. The engine does not reach for the file system,
  the network or the browser by itself.
- **Backends are internal, optional and lazy.** Which exist in a build is a compile-time decision (the cfg aliases
  `web`, `native`, `apple_silicon` from `build.rs`); a backend file carries a single `#![cfg(alias)]` only when its
  library cannot compile elsewhere, and registers itself with `inventory`. A backend describes itself as data
  (`BackendSpec`: accelerators in order of preference, requirements as checks); the engine matches, ranks, selects
  and installs for every backend alike. Which models a backend runs is the catalogue's to say, and what it downloads
  is data too, never code. A backend's code only checks its accelerators for real when the default `probe()` is not
  enough, and loads a model. Preparing a model installs and loads only the selected build.
- **Everything is closed by default.** Each item gets the narrowest visibility that works: private, then
  `pub(super)` or `pub(crate)`, and `pub` only for what consumers of the crate actually need. The public API is
  deliberate: opening something later is cheap, closing it later is a breaking change. Backends, for instance, are
  crate-private; if plugins ever need to add backends, that part of the contract is opened then, on purpose.
- **Nothing heavy is linked into the app.** Engine libraries and models are downloaded on demand.

## How work lands

- Pull request titles are [Conventional Commits](https://www.conventionalcommits.org) (CI checks them); a PR is
  squash-merged and its title becomes the commit, from which release notes are written (`RELEASING.md`).
- Commits are signed.
- Logic lives in `cargo xtask`, not in workflow YAML: a workflow sets up the machine and calls one `cargo xtask`
  command, which runs the same on a laptop.
- Third-party actions are pinned by commit SHA, with the version in a comment.
