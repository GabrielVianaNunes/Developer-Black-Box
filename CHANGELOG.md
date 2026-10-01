# Changelog

All notable changes to Developer Black Box are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`; pre-releases as `-alpha.N`, `-beta.N` or `-rc.N`).

## [Unreleased]

### Added
- Exclusion rules by event type: for each excluded program you now choose what is still recorded (start and end,
  CPU and memory, crashes and hangs). By default nothing is, so adding a program still leaves it out entirely; ticking a
  box is a deliberate choice to record more. CPU and memory need the start and end (the start carries the program name).
  Existing exclusions keep meaning "everything", exports re-apply the rules per event type, and unreadable rule data is
  treated as a full exclusion.

### Changed
- The program search also reads the Start Menu shortcuts, so many more programs show up (about 65% more on a typical PC) with the
  names you know, such as "Android Studio" or "LibreOffice Writer". Shortcuts are parsed locally by a minimal reader (no
  Windows shell calls, nothing resolved or executed); only the program name and the shortcut name are used, in memory.
  Uninstaller shortcuts are left out, and shortcuts that open a generic host (a control panel, a script) show the program
  name instead of what they open.

## [0.2.0] - 2026-09-30

### Changed
- "Protected applications" and "Exclusion rules" no longer take free text: type to search the programs installed or running on
  this PC and pick one from the list, or use "Browse for a file…" to choose an `.exe` (only its name is kept). The list
  is built locally in memory, never stored or sent.

## [0.1.2] - 2026-09-30

### Fixed
- "Open with Windows" failed on a Windows account whose `Run` registry key did not exist yet ("file not found"); the key is now created when needed.
  Found by the pull-request CI on a fresh GitHub runner.

## [0.1.1] - 2026-09-30

### Added
- English and Portuguese (Brazil) interface, switchable at any time from the app window; the tray menu,
  tooltip and installer follow the language too.
- The app version is shown next to the title.
- Optional update check (Privacy tab): off by default, one HTTPS request to this project's GitHub Releases that tells
  you a newer version exists. "Check now" works even when it is off.
- In-app update: "Download update" fetches the installer, checks its SHA-256 and Ed25519 signature (bound to the version)
  and discards it if anything is off; only "Install and restart" runs it (progress only, then the app reopens).
- Release installers are signed (Ed25519) over version + SHA-256 with a key that stays on the maintainer's machine;
  the app embeds the trusted public key and will only accept an update that verifies.
- Single source of truth for the version (`scripts/version.mjs`), checked in tests and before every release;
  `CHANGELOG.md` is the source of the GitHub Release notes and `RELEASING.md` documents the release procedure.
- Tests run on every pull request (GitHub Actions).

### Fixed
- In-app update download: GitHub answers 404 to release downloads requested with `Accept: application/octet-stream`,
  so a signed release was reported as unsigned. Found by testing against the real release (only `0.1.1-rc.1` was affected).

## [0.1.1-rc.1] - 2026-09-30

Pre-release used to test the release pipeline. Superseded by 0.1.1, which contains everything in it plus the fix above.

## [0.1.0] - 2026-09-30

### Added
- First public release: local recorder of technical process events with a Privacy Guard, tray indicator
  (green/red), incident detection, encrypted storage, re-filtered export and a Windows installer.
