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
   Then **sign the Release on your machine** (see "Release signing" below): `node scripts/sign-release.mjs release v0.2.0`.
6. Back-merge into `develop` with a pull request `main` -> `develop`, then delete `release/0.2.0`.

## Hotfix

Branch `hotfix/0.2.1` from `main`, fix, then follow steps 3-6 (use `bump patch`); the back-merge goes to `develop`.

## Checks you can run any time

```bash
node scripts/version.mjs                 # current version
node scripts/version.mjs check v0.2.0    # version files consistent, tag matches, changelog section exists
node scripts/changelog.mjs notes 0.2.0   # the text that becomes the Release notes
```

## Release signing (Ed25519), done on the maintainer's machine

Every installer is signed, and the app only accepts an update whose signature matches one of the public keys
embedded in it (`TRUSTED_PUBLIC_KEYS` in `crates/bb-update/src/verify.rs`). The signature covers the **version and
the installer's SHA-256**, so a tampered file, or an old genuine installer presented as a newer version, is rejected.

**The private key never leaves the maintainer's computer and is never in this repository.** CI only builds and
publishes the installer; the maintainer signs it locally afterwards. Until then the Release has no `.sig` and apps
refuse to install it (fail closed).

### After the workflow publishes the Release (step 5 above)

```bash
node scripts/sign-release.mjs release v0.2.0
```

The command downloads the installer from the Release, checks it against `SHA256SUMS.txt`, shows its SHA-256 and waits
for a typed `yes`, signs it with the local key, verifies the result against the keys embedded in the app, and uploads
**only** the `.sig` file. It refuses an installer that does not match its checksum, a key the app does not trust, and
a release that is already signed (`--resign` replaces the signature).

### Where the key is

The script reads the key location from `signing.local.json` at the repository root: a local file that is listed in
`.gitignore` and rejected by the repository safety checks, so it cannot be committed. Its shape is
`{ "keyFile": "<absolute path outside the repository>" }`. `--key-file <file>` overrides it. The scripts refuse to
create or use a private key inside the repository.
