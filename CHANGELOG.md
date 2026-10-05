# Changelog

All notable changes to Developer Black Box are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`; pre-releases as `-alpha.N`, `-beta.N` or `-rc.N`).

## [Unreleased]

### Added
- Incident detail: **Export with a password**. The same export document is encrypted with a password you choose at that moment (8+ characters,
  typed twice): AES-256-GCM, key from Argon2id (64 MiB, 3 passes), random salt and nonce, format and costs authenticated. The readable text never
  touches the disk and the file is `incident-<id>-<time>.protected.json`. There is no recovery: the app does not keep the password anywhere. A
  wrong password and a damaged file give the same error; costs read from a file are bounded. The plain export keeps working and is still labelled
  as not encrypted. New dependency: `argon2` (RustCrypto, no network code); the key derivation is checked against the Argon2 reference
  implementation. (#109)

### Tests
- Rust code is now formatted with `rustfmt` (`rustfmt.toml`: 140 columns, short lists on one line, to stay close to the existing style) and CI
  fails if `cargo fmt --all -- --check` finds a difference. The formatting commit changes no logic: all 527 tests pass before and after. (#108)
- Interface tests in a real browser (`npm run build && npm run test:ui`, also in CI): the welcome guide (steps, skipping, Esc, keyboard focus kept
  inside, the smaller buttons), the System health tab, the Privacy tab (switches, unsaved-changes bar, Apply and Discard) and the language switch,
  against a simulated backend with made-up data only. Dev dependency added: `playwright-core` (it uses the Chrome already installed; nothing is
  downloaded). (#108)
- The two `apps::` tests that failed on Linux now pass (they built folder paths with `\`); the timing checks of the Windows program listing use
  the fastest of three runs, so a load spike no longer fails them while a real slowdown still does. (#108)

### Added
- Privacy tab: individual switches for the Windows health events, the machine inventory and the power and battery sources (all on by default),
  next to the existing one for the performance counters. Turning a source off stops it at once and what happened while it was off is never
  read afterwards: the event log drops its pending interval (even across a restart), the inventory forgets its last known values so turning
  it on again starts a fresh baseline instead of recording what changed meanwhile, and the power source records the state of that moment.
  A setting that cannot be read back counts as off. The System health tab shows a switched-off source as "off". (#105)
- Optional notification when an incident opens by itself (Privacy tab, **off by default**). It carries only the type of the incident from a
  fixed text, never a program name or recorded data. It is not shown for the manual capture, waits while a browser or password manager is in
  front, is dropped on a manual pause, when the switch is turned off or after 30 minutes, and clicking it does not open the app. New
  dependency in the app: `tauri-plugin-notification` (no network library). (#106)
- Incident detail: **Copy summary**, a short text for a bug report (type, time, app version, Windows build, the nearest machine health events and
  the last performance sample before the incident). It is built from the same export document, with today's privacy rules applied again, never
  from the recorded data directly: no notes, no user or computer names, no paths or message texts, and the program name only if the export
  would include it. The text is copied and also shown, read-only, for you to check; nothing is written to disk or sent. (#107)

### Changed
- The welcome guide is shorter: the first-run tour went from 8 steps to 6 (the "starts paused" step now sits with the recording light, and
  "language and updates" and "you are ready" became one closing step) and each step is one or two paragraphs, keeping the same facts. The
  "What's new" text of 0.5.0 went from three paragraphs to two.
- The buttons of the welcome guide and of the "What's new" window (Skip, Back, Next) are smaller and lighter. The rest of the app keeps its buttons.

## [0.5.0] - 2026-10-03

### Changed
- System health, checked on a real Windows 11 laptop: the Event Log, inventory, power and performance counter sources returned real values
  (counts matched Windows' own tools for disk 11 and Windows Update 20), and in the running app health data kept being recorded during a
  privacy block, stopped with the manual pause and with the telemetry switch off, the System health tab listed the four sources, and an incident
  export carried the health numbers. The README now says exactly what was and was not checked: the IDs for shutdowns, blue screens, WHEA,
  display driver resets, NTFS and services did not occur on that machine and are covered only by synthetic XML.

### Added
- Documentation of the system health feature (#76). The README (English and Portuguese) has a new **System health** section: what each
  source reads and how, the full fixed list of Event Log IDs with the number kept for each, what is **never** recorded, when it records
  (also during a privacy block; a manual pause always wins, with the one explicit exception for the interval in which the app was closed),
  how to turn it off (the performance counters have a switch in the Privacy tab; the other sources are controlled by the pause and by
  deleting the activity, and have no switch of their own), what it cannot see **without administrator rights** (disk SMART/NVMe, TPM, the
  WHEA Operational channel, per-core temperature, fans and voltages, battery wear) and why libraries that load a kernel driver will not be
  used, what has **not** been verified on a real Windows yet, and the measured storage cost. The "What it does" list now points to it, and
  the Limitations section mentions the same limits. New README tests keep the documentation in step with the code: every rule of the fixed
  Event Log list must appear in the tables in both languages (adding a rule without documenting it fails), the telemetry switch must be named
  exactly as in the Privacy tab, and the limits and the "not verified yet" notice must stay.
- Tests and synthetic fixtures for the machine health data (#75). `tests/fixtures/health/` holds, all invented, one Windows event XML for
  each rule of the fixed Event Log list (plus events that must be ignored), performance counter readings (healthy, throttled, values in the
  wrong unit, nothing available, several GPU engines, not-a-number) and inventory readings, each with the numbers it must become; the tests
  require exactly one fixture per rule, so removing or adding a rule without its fixture fails. A schema test walks **every** variant of
  the health events (categories, inventory items, power sources, the four event types) and checks that only numbers, nulls and the closed
  names are stored, with no path, name, address or GUID in any text, and it stops compiling when a variant is added without updating it.
  A matrix test checks each health event type in each of 8 Guard states (recording, privacy block by protected app, locked session,
  detector failure, manual pause, shutdown, start, restricted test mode) and that the regular door never admits them. Engine tests check
  that with every source unavailable nothing breaks, nothing is recorded and no incident is raised, and that with the manual pause on no
  source is even read. `npm run check:repo` now also runs a fixture hygiene check that rejects values that look real (computer or user
  names without a synthetic marker, emails, MAC addresses, non-zero GUIDs and SIDs, private IP addresses, serial numbers). Mutation testing
  of these rules (breaking each on purpose and confirming a test fails) was done and reported in the pull requests.
- The incident export now carries the **machine health around the incident** (#74), in a new `health` section; the export format is now
  `developer-blackbox-export/2` (documented in the README, "Export format"). It holds the Windows health events, inventory changes,
  power records and performance samples whose timestamp falls inside the incident window: from the start of the evidence window to its
  end and, for blue screens, unexpected shutdowns, hardware errors and throttling, from where the condition began (the incident summary
  now ends with that time, up to 8 days back), so the event that caused an incident is in its own export even when the app only read it
  after a long gap. Nothing outside the window is exported. **Every health row is checked again at export time** against closed sets
  (exact keys, fixed category and item lists, numbers within range; anything else is dropped and counted, never quoted), so no free-text
  field can reach the file. **Today's rules apply**: performance samples are left out if the counters are turned off now. Notes are still
  never exported, application events keep the same exclusion and protection rules as before (health rows do not count in
  `droppedEvents`), at most 5,000 health rows are exported (the closest to the incident, with `truncated: true` otherwise), and the
  interface still warns that the file is **not encrypted**, now also mentioning the health data.
- Automatic health incidents and the "System health" tab (#73). **Incidents** now open on their own for a **blue screen** (Kernel-Power 41
  with a non-zero stop code, or the stop code event), an **unexpected shutdown** (power loss or freeze, with no stop code), a **hardware error**
  (WHEA) and **sustained thermal throttling** (see #70: about 5 minutes of passive limit below 100% with the CPU at 70% or more). They are
  created only from events the Privacy Guard already admitted, state only a code and numbers (no app name, no message text), and carry the
  same evidence as the other incidents: the window before is preserved at once and the window after when it ends. The events of one bad
  shutdown (for example Kernel-Power 41 and the blue screen event, logged together on the next boot) open **one** incident, hardware errors
  are condensed to one per 5 minutes and throttling to one per half hour. An incident about an event read when the app starts again is
  anchored at the detection time, so its evidence is whatever the app recorded around that moment. **The tab** shows, for each source (Windows
  event log, inventory and changes, power and battery, performance counters), its state: OK, Attention (devices with a driver problem, battery
  at 10% or less while unplugged, sustained throttling), Unavailable (this machine does not offer it or the last reading failed: **never** an
  alarm), Waiting (not read yet), Paused (manual pause) or Off (counters turned off). It also lists the latest performance sample, a 7-day
  timeline of health events, power changes and sleep, the inventory changes, and what is **not** monitored and what is **never** recorded.
  Texts in both languages, a "what is new" step for 0.5.0 and a synthetic screenshot in the README (made with an invented backend, no data from a
  real machine). It only shows what was already recorded under the closed schema. Not checked on a real Windows machine: only the portable logic
  (incident rules, source states) is tested here and by the CI.
- System performance telemetry (#70): about one sample every 30 seconds of **system-wide counters**, read through the Windows
  performance counters (PDH) with the **English counter names** (`PdhAddEnglishCounter`, so it works the same on a Portuguese Windows),
  without administrator rights. One sample is a single record of plain numbers: hottest thermal zone (kelvin) and the lowest passive
  limit (%), total CPU load, CPU performance (%) and frequency (MHz), commit use (%), available memory (MB), page faults per second,
  disk latency (microseconds) and busy time (%), network errors and 3D GPU use (%). Network errors are **one aggregated total** (the delta
  of received plus outbound errors across all adapters since the previous sample); adapter names, SSIDs and IPs are never read into the
  record, and for the GPU only the 3D engine total is kept (the per-process instance text is thrown away). Every value is validated for
  range and unit (for example the temperature must be 200 to 450 K, so a value in tenths of a kelvin is rejected rather than "fixed"); a
  counter that is missing or invalid is simply null ("unavailable"), and a sample with no counter at all is not stored. It is **on by
  default**, with a switch in the Privacy tab ("Record system performance counters"); turning it off stops reading the counters at once,
  and a value that cannot be read back from the settings means off. It follows the same gates as the other health records (recorded
  during a privacy block; manual pause, shutdown, start and restricted test mode win). **Storage cost, measured** with the real
  recorder and 2,880 pseudo-random synthetic samples (one day at 30 s): about **337 bytes per sample** while the journal is still open
  (about 0.97 MB per day of samples before it is sealed) and about **40 bytes per sample once sealed** (about 0.115 MB per day). The
  existing storage limit and retention apply as usual; a test fills the recorder with two days of samples and checks that the oldest are
  dropped first and the budget holds. A simple **throttling detector** (passive limit below 100% while CPU load is 70% or more, for 10
  samples in a row, about 5 minutes; any sample missing either number restarts the count) is exposed for the automatic incidents of #73.
  The Windows side was only compiled for Windows and tested with synthetic readings: whether the wildcard counters (thermal zones,
  network, GPU) return what is expected on a real machine is **not checked yet**.
- Power and battery (#72), without administrator rights. `GetSystemPowerStatus` gives the power source (plugged in or on battery) and the
  charge percentage; the raw bytes are converted by a pure function, and any value outside the documented ranges becomes "unknown",
  never an invented number. A reading is recorded the first time, whenever the power source changes, and when the charge moved 5
  percentage points or more since the last recorded one (read every minute), so a day on battery stays a handful of records. A computer
  **without a battery** (or whose battery cannot be identified) has this source marked "unavailable" and records nothing. Only two states
  and a percentage are stored: no location, network, battery name, manufacturer or serial number. Same gates as the other health events
  (recorded during a privacy block; manual pause, shutdown, start and restricted test mode win); after a pause the current state is
  recorded again. **Sleep and resume** come from the Windows Event Log (`Kernel-Power` 42 "entering sleep", with the target state 3 or 4,
  and 107 "resumed"), through the same fixed list, watermark and "no reconstruction" rules as the other Event Log health events. These two
  IDs and the meaning of the state number follow Microsoft's public documentation and were **not checked on a real Windows machine**.
  **Battery wear (design capacity x full charge) was not implemented**: this environment cannot prove that Windows exposes it without
  administrator rights, and the issue says not to implement it in that case. The
  Activity tab shows the new records in both languages.
- System inventory and changes (#71): the app reads, without administrator rights, the BIOS version and date, the firmware type
  (UEFI or legacy), Secure Boot, the Windows build (with revision) and the **count and codes** of present devices with a driver problem
  code (a device you disabled yourself, code 22, is not a failure and is not counted). It records **only changes** between two readings,
  as `previous -> new`, never the readings themselves. Everything is a number: dotted versions are packed into one number, and a version
  that is not numeric (for example `F.12`) is stored only as a 52-bit hash, enough to say "it changed" but not what it was. The reference
  (last known value of each item, numbers only) is kept encrypted in the settings. The first reading of an item is only a reference, an
  item that cannot be read is never a change, and a reference that cannot be parsed is treated as a first reading. Read at start and every
  10 minutes, with the same gates as the health events (recorded during a privacy block; manual pause, shutdown, start and restricted test
  mode win). What the source reads: BIOS and Windows version values from the registry (HKLM, readable by regular users), `GetFirmwareType`,
  the Secure Boot state value, and only the problem code of each present device through Windows' device configuration API. Serial numbers,
  UUIDs, computer name, manufacturer, machine model and device names or instance IDs are never read. The optional machine model was **not**
  implemented (it would identify the machine and was not asked for). Not yet checked on a real Windows machine: only compiled for Windows and
  tested with synthetic readings. The Activity tab shows the changes in both languages. If a change happens while the app is closed, it is
  recorded when the app reads again (the time shown is the detection time).
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
