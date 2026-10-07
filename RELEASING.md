# Releasing

One version for the whole workspace, tagged `vX.Y.Z`. It lives in the workspace's `Cargo.toml` (and `Cargo.lock`);
release-please moves it (`release-please-config.json`: release type `rust`, `bump-minor-pre-major`, draft
releases). Never edit it by hand.

The engine is distributed as source: native consumers (the desktop app, later the core) depend on the crates at a
git tag and compile them themselves. The web will get `@sidevoice/engine` on npm; that is **not wired yet** (see
[npm](#npm-pending)).

## What each act means

| Act | Who | What happens |
|---|---|---|
| Open / update a PR | anyone | `ci` runs, publishing nothing. **PR title is a conventional commit**. |
| Squash-merge into `main` | reviewer | The PR title becomes the commit. release-please opens or updates the **release PR** ("chore(main): release X.Y.Z"). |
| Merge the release PR | a maintainer | **This is the release.** release-please tags `vX.Y.Z` and creates a draft GitHub Release whose notes are that version's changelog. |

Everything besides the GitHub steps is code in `xtask/` (`cargo xtask <command>`), so it runs the same on a laptop.

A native consumer pins a release by its tag:

```toml
sidevoice-engine = { git = "https://github.com/sidevoice/sidevoice-engine", tag = "vX.Y.Z" }
```

The changelog is written from the squashed PR titles. To change it, edit `CHANGELOG.md` in the release PR right
before merging it: any later merge into `main` regenerates the PR. After the release, fix the notes on the
Release itself.

## npm (pending)

Not wired yet: nothing is published to npm today. What it will be, as in sidevoice-connector:

- `@sidevoice/engine`: the WebAssembly build of `crates/engine-web` and the engine's JavaScript backends, built by
  `cargo xtask` from the tagged source.
- Published only by the release workflow, for `vX.Y.Z` releases (never anything else), by npm's **trusted
  publishing** (OIDC) with provenance: no npm token exists.

## Which version comes next

`fix:` → patch, `feat:` → minor. While the version is 0.x a breaking change (`feat!:` or a `BREAKING CHANGE:`
footer) bumps the minor, not the major. `docs:`, `chore:`, `ci:`, `test:`, `refactor:` alone make no release.

## A release candidate, or any explicit version

Put the footer as the **last line of a PR's description** (the squash commit takes the description as its body):

```
Release-As: 0.2.0-rc.1
```

The release PR then proposes exactly that version. A version with a `-` suffix is published as a **pre-release and
never as latest**. The next candidate is `Release-As: 0.2.0-rc.2`; the final one is `Release-As: 0.2.0` (say it:
after a candidate, do not leave the next version to the computation). With nothing else to merge, a PR with one
empty commit (`git commit --allow-empty`) carries the footer.

## What this needs from the repository settings

- Settings → Actions → General → **Allow GitHub Actions to create and approve pull requests**: without it
  release-please cannot open its PR.
- Squash merging, with the PR title as the commit message.
- `ci` and **PR title is a conventional commit** run on every PR. Make both required in a ruleset to enforce them.
