# Changelog

All notable changes to Developer Black Box are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`; pre-releases as `-alpha.N`, `-beta.N` or `-rc.N`).

## [Unreleased]

### Fixed
- In-app update download: GitHub answers 404 to release downloads requested with `Accept: application/octet-stream`,
  so a signed release was reported as unsigned. Found by testing against the real release; `0.1.1-rc.1` has this bug.

## [0.1.1-rc.1] - 2026-09-30

### Added
- English and Portuguese (Brazil) interface, switchable at any time from the app window; the tray menu,
  tooltip and installer follow the language too.
- The app version is shown next to the title.
- Release installers are signed (Ed25519) over version + SHA-256 with a key that stays on the maintainer's machine; the app embeds the trusted public key and will only
  accept an update that verifies (groundwork for in-app updates; see `RELEASING.md`).
- Optional update check (Privacy tab): off by default, one HTTPS request to this project's GitHub Releases that tells
  you a newer version exists. "Check now" works even when it is off.
- In-app update: "Download update" fetches the installer, checks its SHA-256 and Ed25519 signature (bound to the version)
  and discards it if anything is off; only "Install and restart" runs it (progress only, then the app reopens).
- Single source of truth for the version (`scripts/version.mjs`), checked in tests and before every release.
- `CHANGELOG.md` is now the source of the GitHub Release notes; `RELEASING.md` documents the release procedure.

## [0.1.0] - 2026-09-30

### Added
- First public release: local recorder of technical process events with a Privacy Guard, tray indicator
  (green/red), incident detection, encrypted storage, re-filtered export and a Windows installer.
