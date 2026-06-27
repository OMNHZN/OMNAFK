# Gamepad keepalives

OMNAFK can keep **controller-gated** games awake — games that ignore synthetic
keyboard and mouse input but read a controller via XInput. Two independent
pieces make this work:

## Sense (always on, no setup)

Hold-while-playing now watches the four XInput controller slots as well as the
keyboard and mouse. `GetLastInputInfo` (the Windows "last input" clock) never
sees gamepad input, so without this a controller-only player would have
keepalives fired mid-game. With it, OMNAFK holds its ticks whenever a pad shows
a button, trigger, or stick beyond its deadzone — or any state change since the
last poll — within your hold window.

Sense requires nothing extra and sends no input.

## Send (opt-in, needs ViGEmBus)

Select the **Gamepad nudge** action (globally on the Keepalive tab, or per
target in Sightline) to send a brief virtual left-stick flick each tick. This
uses a virtual Xbox 360 controller created through the
[ViGEmBus](https://github.com/nefarius/ViGEmBus/releases) kernel driver.

Pick the virtual controller type with the **Gamepad type** setting (Expert
section): **Xbox 360** (read via XInput, the default and best fit for most PC
games) or **DualShock 4** (for games that only accept PlayStation/DirectInput
pads). A DS4 is invisible to XInput, so the DS4 nudge never trips OMNAFK's own
controller-activity sense. Switching type re-plugs the virtual pad on the next
nudge.

- The virtual pad is created **lazily** — only the first time a gamepad nudge
  actually runs, never just because the driver is installed.
- OMNAFK records which XInput slot its own virtual pad occupies and excludes it
  from the sense probe, so its nudges are not mistaken for you playing.
- If ViGEmBus is not installed, the first gamepad nudge fails with a message
  pointing at the ViGEmBus releases page; install it once and restart the game.

### Is the kernel driver safe for my games?

ViGEmBus is a legitimately signed driver used by Steam Input, DS4Windows, and
reWASD, so the driver itself does not break games. The caveat is anti-cheat:
aggressive kernel anti-cheats — most notably Vanguard (Valorant) — can detect
virtual controllers, the same way they can detect synthetic keyboard/mouse
input. Gamepad nudge is off by default and only ever runs for a target you
explicitly set it on. As with all of OMNAFK's keepalives, sending synthetic
input may violate some games' terms of service; use it at your discretion.

## Installing the driver from OMNAFK Setup

`OMNAFK-Setup.exe` bundles the ViGEmBus installer and offers an optional
**Gamepad driver (ViGEmBus)** toggle on the install options screen (off by
default). When enabled, setup extracts the bundled installer and runs it
silently (`/quiet /norestart`); the ViGEmBus bundle elevates itself, so a UAC
prompt may appear, and it is a no-op when a current-or-newer driver is already
present. Unattended/silent OMNAFK installs never add the driver — use the
interactive installer or install ViGEmBus separately.

You can always install ViGEmBus yourself from the releases link above; the
in-app guided prompt also appears the first time a gamepad nudge runs without
the driver present.

### How the bundling works (maintainers)

The driver is **not** committed-then-embedded automatically — it is wired
through the same gzip-embed path as `omnafk.exe`:

- `vendor/ViGEmBus_Setup_x64.exe` is the redistributable.
- `scripts/build-custom-installer.ps1` sets `OMNAFK_VIGEM_EXE` to that file
  (or honors a caller-provided override) before building `omnafk-setup`.
- `build.rs` gzip-embeds it behind the `omnafk_embed_vigem` cfg; `setup.rs`
  unpacks and runs it during install when the user opts in.

To update the bundled driver, drop a newer `ViGEmBus_Setup_x64.exe` into
`vendor/` (or point `OMNAFK_VIGEM_EXE` elsewhere) and rebuild. Builds without
the file simply ship without the toggle; gamepad send still works via the
guided install.
