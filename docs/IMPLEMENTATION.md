# OMNAFK — Implementation Plan (Tauri v2 / Rust)

> STATUS: the frontend (Phase 3's UI port), all icons, and tauri.conf.json are already
> complete in this repo. This file remains the acceptance reference.

Phases are sequential. A phase is done when every acceptance item passes on a real
Windows machine. Do not begin a phase before the previous one is accepted.

Spec references (§) point into `docs/OMNAFK-UI-SPEC.md`.

---

## Phase 0 — Repo & toolchain

Scaffold: `cargo tauri init` (app name `omnafk`, plain HTML frontend in `src/`),
workspace with a second binary target `omnafk-engine` for headless runs.
Pin Rust stable; add `windows`, `serde`, `serde_json`, `tracing`, `tracing-subscriber`.

**Accept:** `cargo tauri dev` opens a blank window; `cargo run --bin omnafk-engine`
prints a heartbeat log line; CI-able `cargo test` passes (zero tests is fine).

## Phase 1 — Headless engine (the risky part first)

Modules: `detector.rs`, `keepalive.rs`, `config.rs`.

- Detection per §6.0: every 5s, enumerate visible top-level windows
  (`EnumWindows`, skip tool windows / untitled), gather `WindowFacts`
  (rect vs monitor work area, loaded graphics DLLs via module snapshot,
  exe path, GPU usage via PDH `GPU Engine` counters), score with a pure
  function, threshold by sensitivity (Strict/Standard/Broad).
- Keepalive per §11: per-game timer (interval + ±15% jitter when enabled),
  default action Space tap via `PostMessage(WM_KEYDOWN/WM_KEYUP)`;
  focus-flick fallback behind a config flag. "Hold while playing": skip the
  tick if the target was foreground or received user input in the last 60s
  (`GetForegroundWindow` + `GetLastInputInfo` heuristic).
- Log every verdict and every tick at `info`.

**Accept:** with Roblox running, engine logs `GAME` for it and `IGNORED` for the
browser/IDE, ticks fire on schedule, Roblox's 20-minute idle kick does not occur
over a 30-minute unattended run. Score function has unit tests covering: fullscreen
game, borderless game, browser, video player, Steam-path exe. PostMessage path
verified on Roblox; focus-flick verified on at least one game that ignores
PostMessage.

## Phase 2 — Tray + flyout shell

`tray.rs`, `flyout.rs`. Tauri tray with the Sentinel icon states (§10.2: dormant /
active "eyes lit" / holding blink / suspended faded — pre-rendered ICOs in
`src-tauri/icons/`, swapped at runtime). Frameless 380×560 always-on-top window,
no taskbar button, positioned per §3 (anchored above tray, all four taskbar edges,
clamped to work area), slide-up 150ms, dismiss on blur/Esc, pin per §3.2.
Right-click menu per §6.2: Suspend/Resume · Open OMNAFK · Quit OMNAFK.
Single instance (tauri-plugin-single-instance). Suspend state persists (§3.5).

**Accept:** icon reflects engine state live; flyout opens/dismisses/pins correctly
with the taskbar on each screen edge and on a second monitor; second launch focuses
the existing instance; quit only via tray menu.

## Phase 3 — Port the mockup frontend

Copy markup+CSS from `design/omnafk-mockup.html` into the Tauri frontend.
Delete all demo JS (fake taskbar, fake detection, fake timers). Wire:

- `invoke` commands: get/set every General & Settings control, override pill cycle,
  rescan, suspend/resume, reset stats, import/export config.
- An event stream (`omnafk://state`) pushing status-bar text, countdown, pulse ticks,
  window list, and stats once per second — the UI renders only what the engine emits.
- Pulse line fires on real ticks; reduced-motion per §3.4/§9.

**Accept:** every control round-trips to config.json and survives restart; Targets
tab mirrors reality within 5s; UI is pixel-faithful to the mockup side-by-side;
keyboard focus rings work (§9).

## Phase 4 — Persistence, overrides, stats

Override pinning by (exe name, window class) so relaunched games are re-recognized
(§5.2); closed-game 60s linger; stats per §5.3 with daily "games seen"; import/export
JSON; autostart registry key behind the Settings toggle (tauri-plugin-autostart);
global hotkey to open the flyout (tauri-plugin-global-shortcut).

**Accept:** pin Firefox as GAME, restart app + Firefox, it is kept alive with no
user action; stats survive restart; export → wipe → import restores everything.

## Phase 5 — Installer + updates

v1 ships a custom single-file setup stub at `dist/OMNAFK-Setup.exe`. The stub renders
the §8 installer UI, embeds `omnafk.exe`, performs a per-user copy to
`%LOCALAPPDATA%\OMNAFK`, writes config/startup/uninstall registration, creates
shortcuts, and runs the same visual language for uninstall with "Keep my settings".
The Tauri NSIS bundle remains available as a fallback artifact.

Settings connects OMNAFK to a GitHub Releases repository (`OMNHZN/OMNAFK` by default,
or any `owner/repo` / GitHub URL), checks stable releases, opens the
repo/releases/new-issue pages, and can notify on launch when a new release is
available.

**Accept:** clean VM: install → tray appears → first-run toast (§7.4) → game detected
→ uninstall leaves no files (except settings when kept).

## Phase 6 — Polish & test matrix

- Hand-check 16px ICO: both eye pixels visible at 100% DPI (§10.2).
- Test matrix: fullscreen / borderless / windowed; multi-monitor; taskbar on each
  edge; game running elevated (expect the §7.3-style actionable error); laptop on
  battery (detection loop must stay cheap — target <0.5% CPU dormant).
- Notifications per Settings level; reduced-motion audit; tab/focus audit.
- README in the project's own voice; screenshots from the real app, not mockups.

**Accept:** 24-hour soak run: no handle/RAM growth, dormant CPU <0.5%, no missed
detections in the matrix.
