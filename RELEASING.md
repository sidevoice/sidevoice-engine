# Releasing

One version for the whole workspace, tagged `vX.Y.Z`. It lives in the workspace's `Cargo.toml` (and `Cargo.lock`);
release-please moves it (`release-please-config.json`: release type `rust`, `bump-minor-pre-major`, draft
releases). Never edit it by hand.

The engine reaches its consumers in two ways:

- **Native consumers** (the desktop app, later the core) depend on the crate at a git tag and compile them
  themselves: source, no binaries.
- **The web** gets `@sidevoice/engine` on npm: the wasm32 build of the crate as `wasm-bindgen --target web`
  emits it, with its `package.json`. Published for `vX.Y.Z` releases only ([npm](#npm)).

## What each act means

| Act | Who | What happens |
|---|---|---|
| Open / update a PR | anyone | `ci`: format and Clippy (Linux and macOS), the native tests on every target, the wasm32 tests in Node, and the npm package built and installed as a consumer installs it (`cargo xtask npm`, `npm-smoke`), publishing nothing. **PR title is a conventional commit**. |
| Squash-merge into `main` | reviewer | The PR title becomes the commit. `release` runs: the wasm32 tests, the npm package built and smoke-tested; then it attests the assets, attaches them to the **`nightly`** pre-release, reads them back, verifies them and publishes it. Never on npm. release-please opens or updates the **release PR** ("chore(main): release X.Y.Z"). |
| Merge the release PR | a maintainer | **This is the release.** release-please tags `vX.Y.Z` and creates a draft GitHub Release whose notes are that version's changelog; `release` runs from the tag, attaches and verifies the assets, and publishes the Release; then it publishes `@sidevoice/engine@X.Y.Z` to npm. |

Everything besides the GitHub steps is code in `xtask/` (`cargo xtask test-wasm | npm | npm-smoke | manifest |
publish | npm-publish`, each described at the top of `xtask/src/main.rs`), so it runs the same on a laptop:

- `cargo xtask test-wasm` runs the engine's tests compiled to wasm32 in Node.
- `cargo xtask npm` builds the crate for wasm32 in release mode, runs `wasm-bindgen --target web` on it,
  writes the `package.json` (version: the crate's), a README and the licence, and packs the package into
  `target/npm/sidevoice-engine-X.Y.Z.tgz`.
- `cargo xtask npm-smoke` installs that tarball into a temporary directory with `npm install`, as a consumer does,
  and in Node imports `@sidevoice/engine`, loads its wasm from `node_modules` and creates a `WebEngine` on a
  plain-object host: it must have exactly the web build's backends (`transformers-js`), which proves the packaged
  build kept the backends it registers.

Both wasm commands use the wasm-bindgen CLI at the version of the `wasm-bindgen` crate in `Cargo.lock`, pinned with
the digest of each host's release asset in `xtask/src/wasm_bindgen.rs`: one of that version on the `PATH` is used,
otherwise it is downloaded, checked and kept in `target/tools/`. npm steps run the npm CLI pinned in
`xtask/src/npm.rs` (`NPM_VERSION`), installed into `target/npm-cli` with a configuration of its own: no `.npmrc`,
no token from the environment.

A native consumer pins a release by its tag:

```toml
sidevoice-engine = { git = "https://github.com/sidevoice/sidevoice-engine", tag = "vX.Y.Z" }
```

## Assets

Every GitHub Release (and the nightly) carries:

- `sidevoice-engine-X.Y.Z.tgz`: the npm package exactly as `npm publish` sends it (on the nightly,
  `sidevoice-engine-nightly.tgz`, a fixed name whose download URL never changes; the version inside is the crate's).
- `SHA256SUMS`.
- `attestation.sigstore.json`: one SLSA provenance attestation whose subject is the tarball.

The signer is the workflow `release.yml` on `main`, for nightlies and releases alike. Verify an asset with:

```sh
gh attestation verify sidevoice-engine-0.2.0.tgz \
  --repo sidevoice/sidevoice-engine \
  --bundle attestation.sigstore.json \
  --cert-identity 'https://github.com/sidevoice/sidevoice-engine/.github/workflows/release.yml@refs/heads/main' \
  --deny-self-hosted-runners
```

The changelog is written from the squashed PR titles. To change it, edit `CHANGELOG.md` in the release PR right
before merging it: any later merge into `main` regenerates the PR. After the release, fix the notes on the
Release itself.

## npm

Every `vX.Y.Z` release (never the nightly) is published to npm as `@sidevoice/engine` at that version: dist-tag
`latest`, or `next` for a version with a `-` suffix (a release candidate).

Only the release workflow publishes, by npm's **trusted publishing** (OIDC) with provenance: no npm token exists.
The job `publish-npm` ("Publish to npm") in `release.yml` runs after the GitHub Release is published, for versioned
releases only, as one step, `cargo xtask npm-publish vX.Y.Z`:

1. It downloads the Release's tarball, `SHA256SUMS` and attestation and checks the tarball against both, and that it
   is `@sidevoice/engine` at that version: npm gets the bytes GitHub Releases has, nothing rebuilt.
2. It publishes that tarball with the pinned npm CLI (`npm publish --access public --provenance --tag latest|next`).
   A version already published with the same bytes is skipped, so a re-run carries on; with other bytes it fails:
   **npm versions are immutable** (a published version can never be replaced, only deprecated), so a bad release is
   fixed by the next version.

### Before the first versioned release

The operator sets this up once on npmjs.com, before the first release PR is merged; until then the `publish-npm` job
fails and nothing reaches npm (the GitHub Release is published regardless):

1. **The scope and the name.** `@sidevoice/engine` lives in the `@sidevoice` organisation, which must allow its
   members to create public packages. A trusted publisher is configured on the package's settings page, which only
   exists once the package does: publish it once by hand, from an account in the org, as a public `0.0.0`
   placeholder (`npm publish --access public`), so the name is ours.
2. **The trusted publisher.** `@sidevoice/engine` → Settings → Trusted publisher → GitHub Actions: organisation
   `sidevoice`, repository `sidevoice-engine`, no environment, and the workflow filename npm checks. **npm checks
   the workflow that starts the run, not one it calls**: a version is released by `release-please.yml`, which calls
   `release.yml` (`workflow_call`), so the filename to enter is **`release-please.yml`**. The job's error names the
   filename it saw when it does not match.
3. **No tokens.** In the package's access settings, require 2FA and disallow tokens: only trusted publishing
   remains.

Every workflow on the way grants `id-token: write`, and the job sets up Node.js 24 (trusted publishing needs 22.14 or
later).

## Which version comes next

`fix:` → patch, `feat:` → minor. While the version is 0.x a breaking change (`feat!:` or a `BREAKING CHANGE:`
footer) bumps the minor, not the major. `docs:`, `chore:`, `ci:`, `test:`, `refactor:` alone make no release.

## A release candidate, or any explicit version

Put the footer as the **last line of a PR's description** (the squash commit takes the description as its body):

```
Release-As: 0.2.0-rc.1
```

The release PR then proposes exactly that version. A version with a `-` suffix is published as a **pre-release and
never as latest**, on GitHub and on npm (`next`). The next candidate is `Release-As: 0.2.0-rc.2`; the final one is
`Release-As: 0.2.0` (say it: after a candidate, do not leave the next version to the computation). With nothing
else to merge, a PR with one empty commit (`git commit --allow-empty`) carries the footer.

## Nightly

Every green `release` run on `main` moves the tag `nightly` to that commit and replaces every asset of the one
`nightly` pre-release. Its notes give the commit. It is a snapshot, not a version: never latest, never on npm, and
release-please ignores the tag. Pin a `vX.Y.Z` release, never `nightly`.

Build artifacts on Actions runs are kept 7 days, for debugging only. Download from Releases.

## When something fails

- A release build or its verification fails: the Release stays a draft, its tag in place. Fix forward if needed,
  then re-run the failed jobs of that `release-please` run. Nothing is published until every check passed.
- A `nightly` run fails: the previous snapshot stays. The next green push replaces it.
- The npm job fails: the GitHub Release is already published and stays. Fix the cause (usually the trusted
  publisher settings: the error names them) and re-run that job; a version already published with the same bytes
  is skipped.
- A release run is never cancelled half-way; nightlies queue behind each other.

## What this needs from the repository settings

- Settings → Actions → General → **Allow GitHub Actions to create and approve pull requests**: without it
  release-please cannot open its PR.
- Squash merging, with the PR title as the commit message.
- `ci` and **PR title is a conventional commit** run on every PR; release-please's own PR gets both through a
  dispatched run (its pushes start no workflow by themselves). Make both required in a ruleset to enforce them.
- On npmjs.com, the scope and the trusted publisher of `@sidevoice/engine` ([Before the first versioned
  release](#before-the-first-versioned-release)).
