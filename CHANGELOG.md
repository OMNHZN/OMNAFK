# Changelog

## v0.1.10

- Made Sightline the default opening tab so detected windows are visible first.
- Added zero-setup game hints for common titles, including Roblox, GTA V, Minecraft, Fortnite, Valorant, and CS2.
- Lowered the adaptive learning threshold to 20 samples and switched the default keepalive action to W tap.
- Added automatic elevation prompts for administrator-run games, with a Settings toggle to turn that behavior off.
- Reorganized Keepalive, Stats, and Settings details so daily controls stay easier to scan.
- Refreshed community profile data and kept shared detection hints available by default.

## v0.1.9

- Changed the default keepalive delivery to real keyboard and mouse input for better game compatibility.
- Renamed the legacy background-message option to Background-only mode.
- Added a one-time migration from the old background delivery default while keeping the expert toggle persistent afterward.
- Updated fallback wording around camera nudge and mouse wiggle recovery.

## v0.1.8

- Added opt-in community game profiles for shared detection hints and keepalive settings.
- Kept community profile changes behind the user toggle and saved auto-applied profiles immediately.
- Tightened shared-profile safeguards so remote data cannot disable quiet background sending or rewrite monitor preferences.
- Added clearer community documentation and status details for target rows.

## v0.1.7

- Reorganized the General tab around setup, timing, action, and safety controls.
- Collapsed deeper timing, recovery, monitor placement, maintenance, and about details so daily controls stay easier to scan.
- Separated Settings into clearer startup, detection, hotkey, notification, app mode, interface, update, and maintenance sections.
- Refreshed the About tab so update and support actions are easier to find.

## v0.1.6

- Added quick presets for common keepalive setups.
- Added keepalive health warnings with session fallback options.
- Added per-target adaptive controls, success tracking, and immediate monitor move controls.
- Improved game detection with GPU activity and an always-mark process list.
- Added headless tray mode and restart-as-administrator support for elevated targets.
- Updated the website FAQ with admin, fallback, and always-mark guidance.

## v0.1.5

- Added clearer target details with detection reasons, adaptive action progress, monitor placement status, and holding state.
- Improved the tray menu with live state, target counts, and next-tick status.
- Added a quieter first-run setup area for choosing core defaults.
- Refined activity log wording for marked targets, held ticks, resumed ticks, and sent actions.

## v0.1.4

- Added monitor placement for marked games, including a global target monitor and per-target overrides.
- Added placement styles for preserving size, maximizing, filling the work area, or filling the full monitor.
- Added a skip-while-playing guard for monitor moves so active windows are not pulled while you are using them.
- Made keepalive holding more conservative: recent user input now holds sends even when the foreground window check is not exact.
- Improved release notes so each update is easier to read.

## v0.1.3

- Added adaptive keepalive actions that can learn a small per-game key profile from safe movement inputs.
- Added per-target reset controls for learned actions.
- Kept release builds on the stable GitHub Releases channel.

## v0.1.2

- Refreshed the app presentation, website, and release page around the OMNAFK visual identity.
- Added the custom setup experience and GitHub update connection.
- Improved Settings with update checks, bug reporting, and diagnostics.
