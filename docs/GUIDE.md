# OMNAFK User Guide

OMNAFK is a tray-first Windows app. It watches visible windows, marks likely games, and sends quiet keepalive input only when a marked game needs it.

## Install

1. Download `OMNAFK-Setup.exe` from the latest release.
2. Run the installer.
3. Leave **Start with Windows** enabled if you want OMNAFK to live in the tray.
4. Launch OMNAFK once after install.

## First Run

Open a game and give OMNAFK a moment to scan visible windows. The tray flyout shows whether it is dormant, watching, or active.

If a game is not marked automatically:

1. Open **Sightline**.
2. Change the filter to show visible windows.
3. Mark the game as a target.
4. Leave other apps ignored.

## Sightline

Sightline is the target list. It shows what OMNAFK can see, why something was marked, and whether a target is being watched.

Use the filter to switch between marked, ignored, hidden, and all visible windows. Local choices are saved immediately.

## Keepalive

The General tab controls how OMNAFK keeps a marked game awake.

| Setting | What it changes |
| --- | --- |
| Interval | How often OMNAFK may send a keepalive while idle |
| Randomize timing | Adds small timing variation |
| Action | Default keepalive action |
| Adaptive action | Learns movement keys and prefers what fits the game |
| Send without focus | Sends input without bringing the game forward when supported |
| Hold while I'm playing | Skips keepalives while you are actively using the PC |

Adaptive action starts with the selected default and improves after enough local samples. It stays per game.

## Monitor Placement

Monitor placement can move marked games to a preferred display. Borderless fullscreen usually works best. Per-target overrides are saved with the target profile.

## Updates

The Settings tab can check stable GitHub releases, download the newest setup file, and open the release page.

## Community Profiles

Community intelligence is optional. When enabled, OMNAFK can use shared detection hints and proven settings from the official manifest. Local target marks and local settings always win.

## Troubleshooting

If a game is not detected, open Sightline, switch the filter to show all visible windows, and mark the game manually.

If keepalives do not work in a game running as administrator, let OMNAFK restart elevated when prompted.

If Start with Windows does not work, open Settings, turn it off, turn it on again, and restart Windows.

For repeatable issues, open a GitHub issue and include the OMNAFK version, Windows version, the game executable name, and a screenshot if one helps.
