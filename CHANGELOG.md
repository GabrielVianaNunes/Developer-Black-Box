# Changelog

All notable changes to Developer Black Box are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`; pre-releases as `-alpha.N`, `-beta.N` or `-rc.N`).

## [Unreleased]

### Added
- English and Portuguese (Brazil) interface, switchable at any time from the app window; the tray menu,
  tooltip and installer follow the language too.
- The app version is shown next to the title.
- Single source of truth for the version (`scripts/version.mjs`), checked in tests and before every release.
- `CHANGELOG.md` is now the source of the GitHub Release notes; `RELEASING.md` documents the release procedure.

## [0.1.0] - 2026-09-30

### Added
- First public release: local recorder of technical process events with a Privacy Guard, tray indicator
  (green/red), incident detection, encrypted storage, re-filtered export and a Windows installer.
