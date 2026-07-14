# Changelog

## v0.1.19

- Refined the flyout header so navigation, version, and pin controls fit in one cleaner bar.
- Moved Sightline list actions into a compact actions menu with safer confirmation for bulk changes.
- Grouped Smart presence settings behind a simple master control with a Tune option for advanced layers.
- Collapsed lower-frequency Settings groups so startup, detection, hotkeys, and updates stay easier to scan.
- Switched holding tray status to a steady half-awake icon instead of the open-eye state.
- Sent engine notices through native Windows notifications so important events stay visible in Action Center.

## v0.1.18

- Added presence sensing for menu/lobby vs in-session states using safe local signals.
- Added Sightline presence details so targets can show when OMNAFK thinks a game is in session, at a menu, or being held.
- Added community manifest presence rules for GTA V and Minecraft variants.
- Made Hold while I'm playing respect the foreground game window so input in other apps does not starve background keepalives.
- Added polite focus-flick deferral so background ticks wait for a quiet input moment.
- Reduced tray icon repaint flicker while keeping active/holding eyes steady.

## v0.1.17

- Added periodic stable update checks for long-running tray sessions.
- Kept automatic update installs enabled even when launch notifications are turned off.
- Migrated users on the old default update prompt to automatic updates while preserving quieter/manual choices.
- Guarded update installs so the manual button and automatic checks cannot launch duplicate installers.

## v0.1.16

- Added starred Sightline targets so important games can stay pinned near the top of the target list.
- Added a first-run walkthrough, with a replay option from About.
- Switched closed-flyout notices to native Windows notifications while keeping in-flyout action buttons.
- Made stable updates automatic by default on launch when OMNAFK is idle.
- Cleaned up the removed tray-toast window and refreshed the IPC/docs language around notifications.

## v0.1.15

- Added an **Always ignore exes** rule list for reliable false-positive cleanup.
- Ignored exe rules now flow through detection explanations, saved config, and IPC.
- Expanded default desktop-app filtering for common chat, media, creative, editor, and terminal apps.
- Kept the custom interval field visible while entering a custom value.
- Moved Hold time next to Hold while I'm playing and added more hold-window choices.
- Made pending update install safer by launching setup before stopping the keepalive engine.

## v0.1.14

- Added an **Automatic** update prompt mode that downloads, verifies, and launches stable updates on app launch when OMNAFK is not actively keeping a game awake.
- Improved tray toast placement so notices anchor from the real taskbar/tray area instead of a guessed screen corner.

## v0.1.13

- Added optional virtual gamepad keepalives through ViGEmBus, including Xbox 360 and DualShock 4 nudge modes.
- Added controller activity sensing so Hold while I'm playing also respects gamepad input.
- Added audio activity as an extra detection signal for active games.
- Added per-game detection sensitivity overrides, title-based mark/ignore rules, and a **Why?** explanation for Sightline targets.
- Added opt-in action rotation, adaptive interval warm-up, failure-aware backoff, and multi-instance keepalive support.
- Added a pre-AFK **Test all** check for active targets.
- Added profile sharing for community profile suggestions.
- Added day scope for quiet hours and monitor placement capture in user presets.
- Added a high-contrast interface theme.
- Added script/Stream Deck style controls through relaunch flags such as `--suspend`, `--resume`, `--toggle-suspend`, `--snooze`, and `--rescan`.
- Added optional ntfy/Discord away alerts for paused keepalives and errors.
- Improved detection so game-store launchers are ignored while the actual game window can still be marked.
- Setup can now optionally install the ViGEmBus driver during interactive installs.

## v0.1.12

- Added the in-app update flow: OMNAFK can download the release installer, verify the optional checksum file, launch setup, and return to the tray after updating.
- Added silent install, uninstall, update, reinstall, and downgrade handling to the custom setup.
- Added release checksums for `OMNAFK-Setup.exe`.
- Replaced the native tray menu with a live OMNAFK-styled tray menu and added branded tray/in-app notices.
- Added saved user presets and per-target behavior overrides for hold behavior, hold windows, background-only delivery, and fallback.
- Added richer community profile details in Sightline, including recommendation reasons, confidence split, verified dates, supported variants, apply/dismiss choices, and simple feedback buttons.
- Fixed early sign-in startup so Start with Windows does not fail when Windows has only been awake for a few seconds.
- Improved administrator relaunch handling during tray-only startup.
- Tightened the custom tray menu layout while keeping live status details visible.
- Removed the update-channel selector so updates stay on the stable release channel.

## v0.1.11

- Fixed play-aware holding so OMNAFK's own keepalive input is not mistaken for real player input.
- Made SendInput keepalive actions use paced press and release events for steadier behavior in more games.
- Kept adaptive learning focused on real user input after an OMNAFK tick.
- Split the bottom status bar into state text and next-tick time so active targets read more cleanly.
- Made Start with Windows use one quoted Windows startup entry from setup, Settings, and app launch.

## v0.1.10

- Made Sightline the default opening tab so detected windows are visible first.
- Added zero-setup game hints for common titles, including GTA V, Minecraft, Fortnite, Valorant, CS2, and other platform games.
- Renamed the old **Roblox** quick preset to **Long interval (Space)** and reordered presets by behavior, not game title.
- Updated README and docs copy to describe OMNAFK as a universal game keepalive tool.
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
