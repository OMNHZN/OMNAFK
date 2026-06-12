# Changelog

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
