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
machine) depend on the crate and compile it themselves. The web gets a WebAssembly build, to be published on npm as
`@sidevoice/engine` (not published yet: [`RELEASING.md`](RELEASING.md)).

The platform is injected: a `Host` gives the engine the machine's capabilities, its storage and a way to fetch
files (`BrowserHost` in the browser, `NativeHost` on desktop). The backends that run models are internal to the
engine and optional: which exist in a build is decided when it is compiled, whether they work on this machine when
it runs. Engine libraries and models are downloaded when they are needed, never linked into the app.

## Status

A skeleton: the interfaces, discovery of the backends a build has and their lazy loading, with stub backends. No
model runs yet.

## Layout

```
crates/engine/         the crate sidevoice-engine: traits, data types, Engine, the catalogue, backends/
crates/engine-native/  the crate sidevoice-engine-native: NativeHost, the native entry point
crates/engine-web/     the crate sidevoice-engine-web: the wasm-bindgen entry point (WebEngine, JsHost),
                       compiled to the npm package
xtask/                 build tooling (`cargo xtask`)
```

## Build and test

You need the Rust toolchain pinned in the repository.
```sh
cargo test --locked
```

## Contributing

Issues and pull requests are welcome. Read [`AGENTS.md`](AGENTS.md) first: it holds the rules for code, texts and
tests, for people and coding agents alike. Pull request titles follow
[Conventional Commits](https://www.conventionalcommits.org) (CI checks them) and become the squashed commit, from
which release notes are written ([`RELEASING.md`](RELEASING.md)).

## Licence

[Apache-2.0](LICENSE). The Sidevoice name and logo are trademarks: forks are welcome under their own name — see
[`TRADEMARKS.md`](TRADEMARKS.md).
