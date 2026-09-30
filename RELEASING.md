# Releasing

Versions follow [Semantic Versioning](https://semver.org/) and the branches follow Git Flow:
`feature/*` and `bugfix/*` branch from `develop`, `release/x.y.z` branches from `develop` and goes to `main`,
`hotfix/*` branches from `main`. Every change reaches `develop` or `main` through a pull request.

The version lives only in the root `Cargo.toml` (`[workspace.package]`). Never edit it by hand: the scripts
below keep `Cargo.lock`, `package.json` and `package-lock.json` in sync, and `npm test` fails on any drift.

## Regular release

1. Make sure `develop` is green and `## [Unreleased]` in `CHANGELOG.md` describes what changed.
2. Start the release branch from `develop`:
   ```bash
   git checkout develop && git pull
   git checkout -b release/0.2.0
   ```
3. Set the version and date the changelog (both refuse invalid input and empty releases):
   ```bash
   node scripts/version.mjs set 0.2.0        # or: node scripts/version.mjs bump minor
   node scripts/changelog.mjs promote 0.2.0
   npm test && cargo test --workspace && npm run check:repo
   git commit -am "Release 0.2.0"
   ```
   Only release fixes go on this branch from now on.
4. Open a pull request `release/0.2.0` -> `main` and merge it.
5. Tag the merge commit on `main`. The tag must be `v` + the version, or the workflow fails on purpose:
   ```bash
   git checkout main && git pull
   git tag v0.2.0 && git push origin v0.2.0
   ```
   The **Release** workflow then checks the tag against the version and the changelog, runs the tests,
   builds the installer, and publishes the GitHub Release with the changelog section as its notes.
   Pre-release versions (`0.2.0-rc.1`) are published as pre-releases.
6. Back-merge into `develop` with a pull request `main` -> `develop`, then delete `release/0.2.0`.

## Hotfix

Branch `hotfix/0.2.1` from `main`, fix, then follow steps 3-6 (use `bump patch`); the back-merge goes to `develop`.

## Checks you can run any time

```bash
node scripts/version.mjs                 # current version
node scripts/version.mjs check v0.2.0    # version files consistent, tag matches, changelog section exists
node scripts/changelog.mjs notes 0.2.0   # the text that becomes the Release notes
```
