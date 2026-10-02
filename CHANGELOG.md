# Changelog

All notable changes to Developer Black Box are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`; pre-releases as `-alpha.N`, `-beta.N` or `-rc.N`).

## [Unreleased]

### Added
- Exclusion rules can now also leave out the programs an excluded program starts: a new checkbox, "Also leave out the programs it starts", on
  each rule that excludes the whole program. Helper processes of apps built on WebView2 or Electron (such as the msedgewebview2.exe processes
  of the WhatsApp desktop app) are then never recorded (start, CPU and memory, end), nor are their own children, while a program with the same
  name that something else started is still recorded. The tree is recomputed every cycle from the full process list, a child only counts if it
  started after its parent (so a reused process id never inherits the exclusion), and a child stays excluded even if its parent exits. A new
  or removed rule takes effect on the next cycle with the programs already running, and exports apply the rule again to older data. It only
  applies to a full exclusion, never to one that still records something. Crash and hang records only carry the program name, so they cannot be
  tied to a tree.

### Added
- "Detect the app in front" in the Privacy tab: click, bring the program you want to the front during a 5-second countdown, and the app shows
  the program name it sees (the same name the privacy rules compare), with buttons to add it to the protected applications or to the
  exclusion rules. It only reads the executable name, never the window title, and saves and sends nothing; the buttons only change the
  draft, which still needs Apply. It warns about shared system programs (such as msedgewebview2.exe) and never offers Developer Black Box itself.

## [0.3.7] - 2026-10-02

### Fixed
- Privacy changes could be lost without any warning: the lists of protected programs and exclusion rules are only a draft until you click
  Apply, the Apply button sits at the bottom of the Privacy tab, and switching tabs silently discarded the draft, so a program could look
  protected or excluded without being so. The draft now survives tab changes, and a bar at the top, visible on every tab, says there are
  unsaved privacy changes and lets you Apply or Discard them right there.

## [0.3.6] - 2026-10-02

### Fixed
- Updating from 0.3.3 or earlier could keep showing the old app icon (the cube with a gray light beside it) on the taskbar button
  and on shortcuts: Windows keeps that icon in the memory of File Explorer, and no Windows notification renews it; only restarting
  File Explorer does. When the installer detects an update from one of those versions it now asks, once, whether to restart File
  Explorer (the taskbar and open File Explorer windows close for a few seconds; nothing is lost). Answering No, or a silent
  install, restarts nothing; the new icon then appears after restarting Windows.

## [0.3.5] - 2026-10-01

### Fixed
- After updating from an older version, Windows could keep showing the old app icon (the cube with a gray light beside it) on the
  taskbar button and on shortcuts, because it caches executable icons. On the first start of each version the app now asks
  Windows, with its official "icons changed" notification and before any window exists, to read the icons again. Nothing is
  changed besides a mark with the version number, stored encrypted with the other settings.

## [0.3.4] - 2026-10-01

### Fixed
- The light on the app's taskbar button follows the state again (green recording, red not). Windows shows the executable's
  or the shortcut's icon on that button and keeps it in a cache, so the gray light drawn into the old icon never changed. The
  app and executable icon is now only the cube (no light) and the color is a small badge on the button, which Windows always
  honors; the tray icon next to the clock is unchanged. The installer also asks Windows to refresh its icons after installing
  or updating, so the old cached icon is not kept. Taskbar button: cube plus a small colored dot at its corner, not beside it.

## [0.3.3] - 2026-10-01

### Fixed
- The small light on the app's taskbar button now follows the state (green recording, red not) when the program is opened
  from a shortcut. Windows ties a shortcut-started program to the shortcut and shows the shortcut's fixed icon (gray light)
  on the button; the program now detects that it was started from a shortcut and reopens itself once, directly, so the
  button follows the window icon. If reopening fails it simply keeps running as before.

## [0.3.2] - 2026-10-01

### Fixed
- Removed the large colored badge that 0.3.1 drew on the app's taskbar button. It was not what was wanted: the light is the
  small dot beside the cube, which already changes color with the state in the tray icon and in the window icon. When the
  program is opened from a shortcut, Windows still shows the shortcut's own icon (gray light) on the taskbar button; that
  case is not fixed yet.

## [0.3.1] - 2026-10-01

### Fixed
- The light (green when recording, red when not) now also shows on the app's button in the taskbar. When the program was opened
  from a shortcut, Windows kept the shortcut's fixed icon on that button (with a gray light) and ignored the icon the app
  changes; the button now carries a small colored badge that follows the state and is re-applied when the window comes back
  from the tray. The tray icon next to the clock already worked.

## [0.3.0] - 2026-10-01

### Added
- Welcome guide: new users get a short step-by-step tour on first launch (what the app is, the green/red light, why it starts
  paused, what it never records, incidents, exclusion rules, updates and language). Every step shows an example with made-up
  data that cannot be clicked, and nothing is recorded or changed. It can always be skipped (button or Esc) and reopened with
  the ? button at the top. People who already used the app are not forced through it. The guide text lives inside the app,
  in English and Portuguese, and nothing is fetched from the internet.
- Exclusion rules by event type: for each excluded program you now choose what is still recorded (start and end,
  CPU and memory, crashes and hangs). By default nothing is, so adding a program still leaves it out entirely; ticking a
  box is a deliberate choice to record more. CPU and memory need the start and end (the start carries the program name).
  Existing exclusions keep meaning "everything", exports re-apply the rules per event type, and unreadable rule data is
  treated as a full exclusion.
- What's new after an update: the first time you open the app after an update that changes how it is used, a short
  step-by-step summary shows what is new (same inert examples with made-up data as the guide; skippable with the button or
  Esc). It appears once per update, never over the first-run tour, and its text is built into the app in English and
  Portuguese: nothing is downloaded. In the Privacy tab you can turn it off or show it again. Only two flags and the
  last version seen are stored, encrypted with the other settings.

### Changed
- The program search also reads the Start Menu shortcuts, so many more programs show up (about 65% more on a typical PC) with the
  names you know, such as "Android Studio" or "LibreOffice Writer". Shortcuts are parsed locally by a minimal reader (no
  Windows shell calls, nothing resolved or executed); only the program name and the shortcut name are used, in memory.
  Uninstaller shortcuts are left out, and shortcuts that open a generic host (a control panel, a script) show the program
  name instead of what they open.
- The installer refuses install folders that are known not to work, before copying anything: Program Files (this installer
  has no administrator rights) and folders directly inside AppData\Local or AppData\Roaming (on one PC Windows showed the
  generic icon there). The suggested folder (AppData\Local\Programs\Developer Black Box) stays the default and the folder
  page is unchanged. Updates of an existing installation are never refused, so nobody is left without updates.

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
