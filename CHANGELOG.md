# Changelog

All notable changes to Developer Black Box are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`; pre-releases as `-alpha.N`, `-beta.N` or `-rc.N`).

## [Unreleased]

### Added
- System health events from the Windows Event Log `System` channel (#69), read without administrator rights from a **fixed list** of
  provider + ID pairs. Only the category, the event ID and one optional number are stored; the message text, service names, paths, user
  and computer names are discarded on the spot (the closed schema has nowhere to keep them). Each event is created only by the Privacy
  Guard, through a dedicated door (`admit_health`) that the regular activity door cannot reach. The list (the IDs follow Microsoft's public
  documentation; **they were not yet checked on a real Windows machine**, only against synthetic XML):

  | Category | Provider and ID | Stored number |
  |---|---|---|
  | Unexpected shutdown | Kernel-Power 41; EventLog 6008 | bug check code (41 only) |
  | Blue screen | WER-SystemErrorReporting 1001 | stop code |
  | Hardware error | WHEA-Logger 1, 17, 18, 19, 20, 47 | none |
  | Display driver reset | Display 4101 | none |
  | Disk error | disk 7, 11, 51, 153 | none |
  | File system error | Ntfs 55, 98, 140 | none |
  | Service stopped unexpectedly | Service Control Manager 7031, 7034 | crash count (never the service name) |
  | Update install failed | WindowsUpdateClient 20 | error code (never the update title) |

  Behaviour: recorded even during a privacy block (they do not depend on the app in front), but a manual pause, shutdown, the start
  (no observation yet) and restricted test mode always win. What Windows logged while the app was paused is never recorded afterwards;
  the only exception is the interval in which the app was **closed** after a run that ended while recording (capped at 7 days), so an
  unexpected shutdown logged on the next boot is not lost. The source is read at most every 30 s, incrementally with a watermark kept
  encrypted in the settings; repeated identical events within 60 s are stored once and one read stores at most 50 events. If the channel
  cannot be read the source is simply "unavailable", with no error. The Activity tab lists the new events in both languages.

## [0.4.0] - 2026-10-02

### Fixed
- Protected applications that are Windows (UWP) apps, such as Calculator or Settings, never paused the recording: for them the window in front
  belongs to the Windows window host (ApplicationFrameHost.exe), so the program name never matched the list. The app now finds the real app
  behind the host (the child window of class Windows.UI.Core.CoreWindow, from another process) and uses its name. If the real app cannot be
  identified, or two different apps are found, the program in front is treated as unknown and recording is suspended, never as the host. Only
  window classes and process ids are read, never window titles.

### Added
- Clearer privacy rules (#67). The Privacy tab now says plainly that **protected applications pause everything** while they are in front,
  while **exclusion rules leave out only that program**, in both languages and in a short "what is new" step. Adding a broad host such as
  msedgewebview2.exe to the protected list shows a notice explaining that it pauses recording whenever any app built on it is in front. The
  Overview has a new "Privacy rules in effect" card with the number of protected applications, each exclusion rule, and how many times each
  rule left something out since the app opened (starts, crashes, hangs and started programs). The counter is one number per rule, kept only in
  memory, reset when the app closes and dropped when the rule is removed; it never stores names, titles or content.
- The program picker now lists the Microsoft Store (MSIX/AppX) apps installed for you, even when they are not running, with their friendly
  name (for example WhatsApp for whatsapp.root.exe). The list is read locally and in memory only: the per-user package list in the registry
  and each package's manifest file (a small read-only file, size-limited, from a WindowsApps folder), with no network and no extra
  permission. Only executable names are kept, validated like every other name, never a folder path or account data (#64).
- Exclusion rules can now also leave out the programs an excluded program starts: a new checkbox, "Also leave out the programs it starts", on
  each rule that excludes the whole program. Helper processes of apps built on WebView2 or Electron (such as the msedgewebview2.exe processes
  of the WhatsApp desktop app) are then never recorded (start, CPU and memory, end), nor are their own children, while a program with the same
  name that something else started is still recorded. The tree is recomputed every cycle from the full process list, a child only counts if it
  started after its parent (so a reused process id never inherits the exclusion), and a child stays excluded even if its parent exits. A new
  or removed rule takes effect on the next cycle with the programs already running, and exports apply the rule again to older data. It only
  applies to a full exclusion, never to one that still records something. Crash and hang records only carry the program name, so they cannot be
  tied to a tree.
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
