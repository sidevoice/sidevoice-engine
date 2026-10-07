<!-- Header: .github/assets/readme-header*.svg, from the Sidevoice brand's banner. Badges: shieldcn
     (https://shieldcn.dev), each a light/dark pair so the row follows the reader's GitHub theme. -->
<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/readme-header-on-dark.svg" />
  <img alt="Sidevoice — Give your coding agent a voice. Keep the conversation." src=".github/assets/readme-header.svg" width="750" />
</picture>

<p>
  <a href="https://github.com/sidevoice/sidevoice-engine/actions/workflows/ci.yml"><picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/ci/sidevoice/sidevoice-engine.svg?variant=secondary&size=sm&workflow=ci.yml&branch=main&mode=dark" /><img alt="CI status" src="https://shieldcn.dev/github/ci/sidevoice/sidevoice-engine.svg?variant=secondary&size=sm&workflow=ci.yml&branch=main&mode=light" /></picture></a>
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
files; each platform implements its own (the browser, the desktop app). The backends that run models are internal
to the engine and optional: which exist in a build is decided when it is compiled, whether they work on this machine
when it runs. Engine libraries and models are downloaded when they are needed, never linked into the app.

## Status

A skeleton: the interfaces, discovery of the backends a build has and their lazy loading, with stub backends. No
model runs yet.

## Layout

```
src/            the crate sidevoice-engine, one package per concept (`x.rs` is the package, `x/` its parts):
  lib.rs          the front door: declares the packages, exports the public API
  host.rs         Host, Storage, Fetcher: the platform contract; host/capabilities.rs, what a host reports
  catalog.rs      CatalogSource, the merged catalogue; catalog/model.rs, models and their builds
  backend.rs      Backend: the interface every backend implements; backend/: downloads (backends.json read),
                  requirement, registry, loaded_model (what load returns), implementations/ (one file per backend)
  resolver.rs     the funnel; resolver/offer.rs, what it returns
  install.rs      the installer
  engine.rs       Engine: puts it together; engine/lifecycle.rs, a build's state
  web.rs          the bridge to JavaScript, only in the wasm32 build (the npm package): WebEngine; web/host.rs,
                  the JavaScript host (JsHost) as the engine sees it
  maybe_send.rs   Send/Sync in native builds only
backends.json   what each backend downloads per platform, compiled in; digests written by `cargo xtask pin-backends`
build.rs        the three cfg aliases: web, native, apple_silicon
npm/            the npm package's package.json and README, filled in by `cargo xtask npm`
xtask/          build tooling (`cargo xtask`), a package of its own
```

## Build and test

You need Rust 1.98.1 (the version `.github/actions/setup` installs). The native tests build and check this
platform's backends:

```sh
cargo test --locked
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

## Contributing

Issues and pull requests are welcome. Read [`AGENTS.md`](AGENTS.md) first: it holds the rules for code, texts and
tests, for people and coding agents alike. Pull request titles follow
[Conventional Commits](https://www.conventionalcommits.org) (CI checks them) and become the squashed commit, from
which release notes are written ([`RELEASING.md`](RELEASING.md)).

## Licence

[Apache-2.0](LICENSE). The Sidevoice name and logo are trademarks: forks are welcome under their own name — see
[`TRADEMARKS.md`](TRADEMARKS.md).
