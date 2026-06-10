# OMNAFK — UI Design Specification v1.0

A universal anti-AFK utility for Windows. Keeps any game or application "alive" by sending periodic, configurable input to selected windows — without stealing focus from what the user is doing.

This document is the single source of truth for the UI. Anyone implementing OMNAFK (Win32, WinUI, Qt, Electron, Tauri, web mockup, etc.) should be able to reproduce the interface from this spec alone, pixel-for-pixel in intent if not in literal pixels.

**Design lineage:** Inspired by the layout language of AntiAFK-RBX v3.2 (compact single window, icon tab strip, label+control rows with inline help, status bar) — re-skinned as a pure-black AMOLED monochrome theme and generalized from Roblox-only to any process.

---

## 1. Design philosophy

1. **True AMOLED monochrome.** The background is pure `#000000`. There is no accent color. Hierarchy is expressed entirely through shades of gray, borders, and inversion (white-on-black flips to black-on-white for the most important element on screen).
2. **One window, one job.** No wizards, no modal dialogs except confirmations. Everything lives in a compact 420×640 window.
3. **It already knows.** OMNAFK is autonomous. It detects when a game is running, arms itself, and goes dormant when the game closes. There is no Start button anywhere in the product — the user customizes behavior; they never operate it. Every automatic decision is visible and overridable, but the default experience is: install, forget.
4. **Inversion = on.** Solid white fill with black content marks an enabled/active state (toggle tracks, override pills, the suspend control while suspended). Nothing is ever solid white for decoration.
5. **The app is a heartbeat.** When armed, a thin white "pulse" line at the top of the footer sweeps on every keepalive tick. This is the signature element — the user can tell at a glance, even from across the room, that OMNAFK is alive.
6. **Quiet by default.** No emoji in the UI (the original used many; OMNAFK uses none). Line icons only, 1.5px stroke, monochrome.

---

## 2. Design tokens

Implement these as named constants / CSS variables. Never hardcode raw values elsewhere.

### 2.1 Color (the entire palette — nothing else is allowed)

| Token | Hex | Usage |
|---|---|---|
| `--bg` | `#000000` | Window background, content background |
| `--surface` | `#0A0A0A` | Raised rows, cards, dropdown menus, footer |
| `--surface-2` | `#141414` | Hover state of rows/controls, pressed tab |
| `--border` | `#1F1F1F` | Default hairline borders, dividers |
| `--border-strong` | `#333333` | Borders of interactive controls (dropdowns, toggles, inputs) |
| `--text` | `#FFFFFF` | Primary text, active icons |
| `--text-dim` | `#9A9A9A` | Secondary text, labels, inactive tab icons |
| `--text-faint` | `#5C5C5C` | Disabled text, placeholder, fine print |
| `--invert-bg` | `#FFFFFF` | Fill of the primary (Start) button and active toggle knob track |
| `--invert-text` | `#000000` | Text/icon on inverted surfaces |

Rules:
- No color outside this table. No green "on" states, no red "stop" states, no blue links. "On" is expressed by inversion and brightness; "off" by dimness.
- Focus ring: 1px solid `--text` offset 2px (keyboard navigation must be visible).
- Shadows: none. Depth comes from borders only. (Pure AMOLED — shadows are invisible on `#000` anyway.)

### 2.2 Typography

| Role | Face | Size / weight | Usage |
|---|---|---|---|
| Display | `JetBrains Mono` (fallback: `Cascadia Code`, `Consolas`, monospace) | 15px / 700, letter-spacing 0.08em, uppercase | App name in title bar, Start/Stop button label |
| Body | `Inter` (fallback: `Segoe UI`, system-ui) | 13px / 400–500 | Setting labels, dropdown values, descriptions |
| Data | `JetBrains Mono` | 12px / 400 | Countdown timers, statistics numbers, process names, status bar text |

Rationale: monospace for anything machine-ish (timers, PIDs, process names) reinforces the "utility tool" character and makes numbers stop jittering as they update.

### 2.3 Spacing & shape

- Base unit: `4px`. Common values: 8, 12, 16.
- Window padding: 0 (content rows run edge-to-edge; inner padding 16px horizontal).
- Row height: 44px.
- Corner radius: `6px` on controls (dropdowns, buttons, toggles), `0px` on the window content rows, `10px` on the window itself (if the framework supports it).
- Hairline dividers (`--border`, 1px) between every settings row.

### 2.4 Iconography

- Style: outlined, 1.5px stroke, 18×18px viewbox, `currentColor`.
- Source suggestion: Lucide icon set (MIT) or hand-drawn SVG equivalents.
- Icons used: `home` (General), `crosshair` (Targets), `bar-chart-2` (Stats), `settings` (Settings), `info` (About), `minus` (minimize), `x` (close), `chevron-down` (dropdowns), `help-circle` (inline help), `refresh-cw` (rescan), `play` / `square` (start/stop glyphs inside the button, optional).

---

## 3. Window specification — tray flyout

OMNAFK is a **tray-first application**. There is no taskbar button and no conventional app window. The UI is a flyout panel that pops up anchored to the system tray, exactly like the Windows volume / network flyouts — open it, change something, click away, it's gone. The keepalive engine runs in the background regardless of whether the flyout is visible.

- Size: **380 × 560 px**, fixed. DPI-aware.
- Frameless, custom-drawn. Corner radius 10px on all corners (Windows 11 flyout convention).
- **Anchoring:** opens above the tray icon, bottom edge 12px above the taskbar, right-aligned to the icon. Handle all four taskbar positions (bottom/top/left/right) by anchoring to the nearest taskbar edge. Never overflow the work area — clamp to screen bounds.
- **Open:** left-click the tray icon, or the global hotkey (Settings §5.4). Animation: slide up 8px + fade in, 150ms ease-out. Reduced motion: instant.
- **Dismiss:** clicking anywhere outside, pressing Esc, or the flyout losing focus closes it (fade out 100ms) — *unless pinned*.
- **Pin:** a pin button in the header (see §3.2) toggles pinned mode. Pinned, the flyout becomes a normal always-on-top floating window the user can drag anywhere; it ignores focus loss and only closes via the pin-off or Esc. Pin state and dragged position persist.
- The engine state (armed/idle) is fully independent of flyout visibility. Dismissing the flyout never stops the keepalive.
- There is no "minimize" and no "close means tray" toast anymore — the app simply *is* the tray.

### 3.1 Vertical layout (top → bottom)

```
┌────────────────────────────────────────────┐
│ HEADER                          h: 40px    │  drag region only when pinned
├────────────────────────────────────────────┤
│ TAB STRIP                       h: 44px    │  5 icon tabs
├────────────────────────────────────────────┤
│                                            │
│ CONTENT AREA                    flexible   │  scrolls if needed
│ (active tab's rows)                        │
│                                            │
├────────────────────────────────────────────┤
│ STATUS BAR                      h: 36px    │  + pulse line on top edge
└────────────────────────────────────────────┘
```

There is deliberately **no primary action button**. The engine state is driven by detection (§6.0); the only manual control over the engine is the small Suspend control (§3.5) and per-target overrides (§5.2).

### 3.2 Header (40px)

- Left: 18×18 app glyph, then `OMNAFK` in Display type, then version `v1.0` in `--text-faint` Data type.
- Right: a single **pin button**, 30×30 hit target. Unpinned: outline pin icon in `--text-dim`. Pinned: filled (inverted) pin, and a thin 1px `--border-strong` outline appears around the whole flyout to signal "this is now a window."
- No minimize, no close — dismissal is click-away / Esc.
- The header is a drag region only while pinned (the flyout is position-locked to the tray otherwise).

### 3.3 Tab strip (44px)

- 5 equal-width icon tabs: **General · Targets · Stats · Settings · About**.
- Inactive: icon `--text-dim`, no label.
- Active: icon `--text`, label appears beside icon (13px, 500), and a 2px white underline spans the tab's width along the strip's bottom edge.
- Hover (inactive): background `--surface-2`.
- Keyboard: Left/Right arrows move between tabs when strip is focused.

### 3.4 Status bar (36px)

- Background `--surface`, top border `--border`. Sits at the very bottom of the flyout.
- Left: state text in Data type:
  - Dormant: `DORMANT — WATCHING FOR GAMES` in `--text-faint`
  - Active: `ACTIVE — ELDEN RING — NEXT TICK 04:12` in `--text-dim`, the countdown in `--text` (multiple games: `ACTIVE — 2 GAMES — …`)
  - Paused (user input): `ACTIVE — HOLDING (YOU'RE PLAYING)`
  - Suspended: `SUSPENDED — NOT WATCHING` in `--text-faint`
- Right: activity dot (hollow = dormant, solid = active, absent = suspended), then the Suspend control (§3.5).
- **Pulse line (signature):** a 1px line sits on the very top edge of the status bar. Idle: static `--border`. Armed: every time a keepalive action fires, a white segment sweeps left→right across it over 600ms, then fades. Respect `prefers-reduced-motion` / OS animation settings: replace the sweep with a single 200ms full-line flash.

### 3.5 Suspend control (in the status bar)

The only global manual control. A 24×24 icon button at the right end of the status bar.

- Normal: outline pause icon (two 1.5px bars) in `--text-dim`. Tooltip: `Suspend OMNAFK`.
- Suspended: the button inverts (solid white circle, black pause glyph) — the single loud element on screen, signalling "I have been silenced." Tooltip: `Resume watching`. The pulse line goes static; the tray icon dims (§6.2).
- Suspension is a manual override of everything: no detection, no ticks, until the user resumes. It persists across restarts.

---

## 4. Component library

### 4.1 Settings row
The universal building block of every tab.

```
┌──────────────────────────────────────────────────────┐
│ [icon 18px]  Label text                 [control] [?] │  44px
└──────────────────────────────────────────────────────┘
```
- Icon `--text-dim`; label Body 13px `--text`.
- Control is right-aligned; `?` help button sits 8px right of it.
- Hover anywhere on the row: background `--surface-2`.
- Divider `--border` under each row.

### 4.2 Dropdown
- Size: width fits content (min 120px), height 30px, radius 6px.
- Fill `--surface`, border `--border-strong`, value text Body `--text`, chevron `--text-dim`.
- Open menu: `--surface` panel, border `--border-strong`, items 32px tall, hover `--surface-2`, selected item shows a small white dot on its left.

### 4.3 Toggle
- Track 38×20px, radius full.
- Off: track `--surface`, border `--border-strong`, knob 14px `--text-dim`.
- On: track `--invert-bg`, knob `#000000`. (Inversion = on. No green.)
- Animation: knob slides 120ms ease-out.

### 4.4 Help button (`?`)
- 20×20px circle, border `--border-strong`, glyph `--text-faint`.
- Hover/click: shows a tooltip card (max-width 240px, `--surface`, border `--border-strong`, Body 12px `--text-dim`) explaining the setting in one or two sentences. Click outside or Esc dismisses.

### 4.5 Secondary button
- Height 32px, fill transparent, border `--border-strong`, label Body 13px `--text`. Hover: fill `--surface-2`. Used for Import/Export, Refresh, Reset stats.

### 4.6 Checkbox (Targets list)
- 16×16px, radius 4px, border `--border-strong`.
- Checked: fill `--invert-bg`, black checkmark.

### 4.7 List row (Targets list)
- Height 40px: checkbox · process icon placeholder (16px gray square with first letter of exe) · window title (Body, `--text`, truncate middle) · process name (Data 11px, `--text-faint`, e.g. `eldenring.exe`).

---

## 5. Screens (tab by tab)

### 5.1 General — the main controls

| Row | Label | Control | Default | Help text |
|---|---|---|---|---|
| 1 | Interval | Dropdown: 30 sec · 1 min · 2 min · 5 min · 9 min · 14 min · Custom… | 9 min | How often OMNAFK sends the keepalive action to each armed target. "Custom…" opens an inline numeric field (10–3600 sec). |
| 2 | Randomize timing | Toggle | On | Adds ±15% random jitter to the interval so actions don't fire on a perfectly regular clock. |
| 3 | Action | Dropdown: Space tap · W tap · Camera nudge (mouse) · Key sequence… · Per-target… | Space tap | What input is sent. "Key sequence…" lets you record up to 4 keys. "Per-target…" defers to each target's profile on the Targets tab. |
| 4 | Send without focus | Toggle | On | Posts input directly to the target window so your active work is never interrupted. If a game ignores background input, disable this to use brief focus-flick mode. |
| 5 | Pause while I'm active | Toggle | On | If you've touched the target window in the last 60 seconds, OMNAFK skips the tick — it never fights you for the controls. |

Below the rows: a hairline divider, then centered fine print (Data 11px, `--text-faint`): `Settings save automatically.`

### 5.2 Targets — detection, visible and overridable

This tab shows what OMNAFK currently sees and lets the user correct it. It is a window onto the detector, not a control panel that must be operated.

Top row (44px): segmented control, two segments:
- `AUTO` — **default.** OMNAFK decides per window using the detection heuristics (§6.0).
- `MANUAL` — detection off; only rows the user marks `Always` are kept alive. For users who want full control.

Below: the live window list (auto-rescans every 5 s while the tab is open; `Rescan` secondary button in the header row).

Each list row (40px): verdict pill · process glyph · window title · process name.
- **Verdict pill** (left, replaces the old checkbox): a 56px-wide rounded pill in Data 9px uppercase showing the detector's call and the user's override, cycling on click through three states:
  - `GAME` — solid white pill, black text (detected or forced; this window gets keepalives)
  - `IGNORED` — transparent pill, `--border-strong` border, `--text-faint` text (detected as non-game or forced off)
  - `AUTO` is not a visible state; rows the user has never touched show the detector's verdict with a small hollow dot before the word. Once clicked, the dot disappears (the verdict is now pinned) and a further click cycles GAME → IGNORED → back to auto (dot returns).
- Help (`?`) in the tab header explains the cycle in one sentence.
- Row hover reveals a `profile` ghost button → inline expander with Action override and Interval override dropdowns ("Use global" default).
- A row whose game closed stays listed for 60 s, dimmed, suffixed `(closed)`, then drops off. Pinned overrides persist forever by process name + window class, so a re-launched game is re-recognized instantly.
- Empty state: centered crosshair icon in `--text-faint`, text `Nothing running. OMNAFK is watching.`

Footer note row (fine print): `WATCHING — 1 GAME, 3 IGNORED` (Data 11px, `--text-faint`).

### 5.3 Stats

All numbers in Data type, large where noted.

- Row: `Session kept alive` → value right-aligned, 20px Data `--text` (e.g. `03:47:12`).
- Row: `Actions sent` → e.g. `26`.
- Row: `Longest unbroken streak` → e.g. `01:58:03`.
- Divider, then a per-target mini-table (only armed targets): columns Target (Body 12px) · Uptime (Data) · Actions (Data). Max 5 rows visible, scrolls.
- Bottom: secondary button `Reset statistics` (confirm inline: button label changes to `Click again to confirm` for 3 s).

### 5.4 Settings

| Row | Label | Control | Default |
|---|---|---|---|
| 1 | Start with Windows | Toggle | On (set by installer choice) |
| 2 | Show flyout on launch | Toggle | Off (launches silent to tray) |
| 3 | Detection sensitivity | Dropdown: Strict · Standard · Broad | Standard |
| 4 | Remember pinned position | Toggle | On |
| 5 | Global hotkey (open flyout) | Hotkey field: 30px, Data type, shows `CTRL+ALT+K`; click → "Press keys…" recording state | `Ctrl+Alt+K` |
| 6 | Notifications | Dropdown: All · Errors only · None | Errors only |

Divider, then a 2-column button row: `Import settings` · `Export settings` (secondary buttons, JSON file). Fine print under: `Config stored at %APPDATA%\OMNAFK\config.json`.

### 5.5 About

- Centered app glyph (32px), `OMNAFK` Display type, `v1.0.0` Data `--text-faint`.
- One-line description: `Awake when you aren't.` (Body, `--text-dim`.)
- Rows (each a full-width quiet link row, 40px, chevron-right on the right): `Check for updates` · `View on GitHub` · `Report a bug` · `License (MIT)`.
- Fine print at bottom: `Sending automated input may violate the terms of service of some games. Use at your own discretion.` (Data 11px, `--text-faint`.)

---

## 6. States & behavior

### 6.0 Game detection (the heart of the product)

OMNAFK continuously classifies visible top-level windows. A window is judged a **game** when it scores enough of these signals (exact weights are an implementation detail; the *signals* are the contract):

- Fullscreen or borderless-fullscreen window covering a monitor's work area
- Process has a swapchain / renders via DirectX, Vulkan, or OpenGL (loaded `d3d11.dll`, `d3d12.dll`, `vulkan-1.dll`, `opengl32.dll`…)
- Sustained GPU utilization attributable to the process
- Launched from a known game platform path (`steamapps\common`, Epic, Riot, Xbox, `Roblox`…) or by a launcher process
- Raw-input / DirectInput / XInput device registration
- Negative signals: browser, IDE, office, and media-player window classes are down-weighted

`Detection sensitivity` (Settings) shifts the threshold: **Strict** = only unambiguous games; **Broad** = anything that plausibly wants keepalives (emulators, remote-desktop sessions, idle browser games).

Detection runs every 5 s, is cheap (no polling of process memory), and every verdict is shown — and correctable — on the Targets tab. A user override always beats the detector.

### 6.1 App states
- **DORMANT** — no game detected. The engine sleeps; detection keeps watching. Pulse line static, tray icon outline.
- **ACTIVE** — at least one game detected (or forced `GAME`). Keepalive timers run per target. Status bar shows the countdown; pulse sweeps on every action fired. Entering ACTIVE is automatic and silent (optional notification under Settings → Notifications: All).
- **HOLDING (auto)** — active, but "Pause while I'm active" is suppressing ticks because the user touched the game recently. Activity dot blinks slowly (1 Hz, 50% duty).
- **SUSPENDED** — the user pressed the Suspend control or tray menu item. Nothing runs, nothing is watched. Survives restart.

### 6.2 Tray (the app's home)

- Tray icon: the Sentinel mark with state expressed through the eyes and opacity — see §10.2 for the full table (dormant: dimmed, eyes dark · active: full brightness, **eyes lit** · holding: eyes blinking · suspended: heavily faded). Monochrome only.
- **Left-click:** open/dismiss the flyout (toggle).
- **Right-click menu:** `Suspend` / `Resume` (contextual) · `Open OMNAFK` · `Quit OMNAFK`. Custom-drawn to spec tokens: `--surface` panel, `--border-strong` border, 32px items, Body 13px.
- Quit fully exits the engine. There is no other way to exit (no close button exists).
- Tooltip on hover: `OMNAFK — DORMANT` / `OMNAFK — ACTIVE · NEXT TICK 04:12` / `OMNAFK — SUSPENDED`.

### 6.3 Persistence
- Every control persists immediately on change (no Save button anywhere).
- Pinned/unpinned state and the pinned position are remembered between sessions.

---

## 7. Microcopy rules

1. Sentence case everywhere except: tab labels, the Start/Stop button, status-bar text, and section headers (uppercase Data type).
2. Buttons say what they do: `Rescan`, `Export settings`, never `OK` / `Submit`.
3. Errors state the fix: `Couldn't send input to eldenring.exe — it may be running as administrator. Restart OMNAFK as administrator to fix this.`
4. First launch after install: a single toast (bottom-right above tray, `--surface`, border, 5 s): `OMNAFK is in your tray. It wakes when a game does.` The tray icon flashes twice in sync.

---

## 8. Installer specification (OMNAFK Setup)

The installer is part of the product and follows the same tokens (§2) exactly — same palette, same JetBrains Mono / Inter pairing, same monochrome inversion language. It should feel like the flyout grew into a setup window.

### 8.1 Window

- Size: **560 × 380 px**, fixed, frameless, radius 10px, border 1px `--border`, centered on screen.
- Custom header (40px): glyph + `OMNAFK SETUP` (Display type) + version on the left; a close (×) button on the right (the installer is the one place a close button exists — it cancels setup with an inline confirm, see 8.5).
- Layout: a left rail (160px) listing the steps, content area on the right, footer (56px) with navigation.

```
┌──────────────────────────────────────────────────────┐
│ ◎ OMNAFK SETUP  v1.0                              ×  │ 40px
├───────────────┬──────────────────────────────────────┤
│  01 WELCOME   │                                      │
│  02 OPTIONS   │   step content                       │
│  03 INSTALL   │                                      │
│  04 DONE      │                                      │
├───────────────┴──────────────────────────────────────┤
│ fineprint                    [ BACK ]   [ NEXT ]     │ 56px
└──────────────────────────────────────────────────────┘
```

### 8.2 Step rail

- Items in Data type, 11px, letter-spacing .08em: `01 WELCOME`, `02 OPTIONS`, `03 INSTALL`, `04 DONE`.
- Future steps `--text-faint`; current step `--text` with a 2px white bar on the rail's right edge; completed steps `--text-dim` with the number replaced by a small check glyph.
- Numbers are justified here: installation genuinely is a sequence.
- Rail background `--surface`, right border `--border`.

### 8.3 Steps

**01 Welcome.** Large glyph (40px), `OMNAFK` display headline, one line: `Awake when you aren't.` Below, Data fineprint: version, size on disk (`~2 MB`), MIT license link. Primary footer button: `INSTALL →` (skips Options, uses defaults — express path) and a quiet text button `Customize` that goes to step 02 instead.

**02 Options.** Settings rows (§4.1, 40px here):
| Label | Control | Default |
|---|---|---|
| Install location | Path field (Data type) + `Browse…` secondary button | `%LOCALAPPDATA%\OMNAFK` |
| Start with Windows | Toggle | On |
| Desktop shortcut | Toggle | Off |
| Launch when finished | Toggle | On |
Fine print: `Per-user install. No administrator rights required.`

**03 Install.** No spinner, no marquee. A single full-width 2px progress track (`--border`) that fills with solid white left→right, percentage in Data 20px above it, and below it a Data 11px `--text-faint` live line of what's happening (`Copying omnafk.exe…`, `Registering tray startup…`, `Writing config…`). The pulse-line language from the app, reused. Footer buttons disabled during copy.

**04 Done.** Centered check-in-circle glyph (outline, 2px stroke), `INSTALLED` in Display type, fine print `OMNAFK is in your tray. It wakes when a game does.` Footer: single inverted button `FINISH` (launches the app if the toggle was on; the tray icon flashes per §7.4).

### 8.4 Installer behavior

- Engine: NSIS or Inno Setup with a fully custom-drawn page (both support embedded UI), or a self-contained Rust/C++ stub that performs the copy itself — the spec is engine-agnostic; what matters is the rendering matches §2 tokens.
- Single file `OMNAFK-Setup.exe`, code-signed if possible.
- Per-user by default (no UAC prompt). If the chosen path requires elevation, prompt then.
- Uninstaller: registered in Apps & Features; running it shows the same window style with steps `01 CONFIRM`, `02 REMOVE`, `03 DONE`, plus a `Keep my settings` toggle (default on).

### 8.5 Cancel confirm

Clicking × mid-setup swaps the footer for: `Cancel setup?` (Body, `--text-dim`) + secondary `Keep installing` + quiet destructive `Cancel setup` (still monochrome — destructive is expressed by being the *non*-default, never by red).

---

## 9. Accessibility & quality floor

- All interactive elements reachable by Tab; visible 1px white focus ring, 2px offset.
- Tooltips (`?` content) also available on focus + `F1`.
- Contrast: `--text-dim` (#9A9A9A) on `#000` ≈ 7.4:1; never place `--text-faint` text smaller than 11px.
- Respect OS reduced-motion: disable pulse sweep and knob slide animations (use instant state changes / single flash).
- All timers use monospace digits (or `font-variant-numeric: tabular-nums`) to avoid layout jitter.

---

## 10. App logo / icon — the Sentinel

The app mark is the **Sentinel**: a white hooded figure on transparent, face in shadow, eyes as narrow white slits (source asset: `OMNICO.png`, 1254×1254). It is the single piece of illustration in the entire product; everything else stays line-icon austere so the Sentinel carries all the character.

### 10.1 Asset preparation
- Cut the black background to true transparency (alpha from luminance works: the artwork is pure white-on-black). Interior blacks (hood shadow, mask) may remain transparent — on the product's dark surfaces this is invisible and keeps the file clean.
- Produce two master variants:
  - **`sentinel-dormant`** — base artwork, eyes as drawn (dark slits).
  - **`sentinel-active`** — identical, but the eye slits filled solid white with a 1–2px soft glow. At 16px this reads as two bright pixels — enough.
- ICO contains both variants at 16, 20, 24, 32, 48, 256. Ship a dark-on-transparent inverse for light-mode trays.
- Where the mark appears in UI: flyout header (18px), About tab (48px), installer header (18px) and Welcome step (56px), uninstaller, file icon of `omnafk.exe`.

### 10.2 Tray status language (the icon IS the status display)
The tray icon must communicate engine state at a glance, monochrome only:

| State | Icon | Read |
|---|---|---|
| **DORMANT** | `sentinel-dormant` at ~75% opacity | hooded, waiting |
| **ACTIVE** | `sentinel-active` at 100% — **the eyes light up** | it's awake |
| **HOLDING** | `sentinel-active`, eyes blink at 1 Hz | awake, deferring to you |
| **SUSPENDED** | `sentinel-dormant` at ~35% opacity | silenced |

- Swap via `Shell_NotifyIcon(NIM_MODIFY)`; pre-load all states, never regenerate at runtime.
- Optional per-tick flourish (Settings, off by default): eyes flash one frame brighter when an action fires — synchronized with the flyout's pulse-line sweep.
- Windows renders tray icons at 16–20px depending on DPI; verify both eye pixels survive at 16px on a 100% scale display before shipping.

---

## 11. Implementation notes (non-binding)

- **Language note:** this spec and the HTML mockups are design documents, not the app. The mockups are HTML/CSS/JS purely so they can be viewed in a browser. The recommended implementation stack for the real app is **C++ / Win32** with custom-drawn controls (matching AntiAFK-RBX's footprint: single small .exe, no runtime) — or **Rust + Tauri** if web rendering is acceptable. Avoid Electron if "lightweight" is a goal.
- Detection plumbing (Win32): `EnumWindows` + `GetWindowRect`/`MonitorFromWindow` for fullscreen tests; `EnumProcessModulesEx` or `CreateToolhelp32Snapshot` for graphics-DLL checks; `QueryFullProcessImageNameW` for install-path heuristics; GPU usage via `pdh.dll` counters (`GPU Engine` instance per PID). Score, threshold by sensitivity, cache verdicts per (exe name, window class).
- Tray flyout plumbing (Win32): `Shell_NotifyIcon` for the icon; on `WM_LBUTTONUP` get the icon rect via `Shell_NotifyIconGetRect`, position a borderless layered popup window (`WS_POPUP`, `WS_EX_TOOLWINDOW` so no taskbar button, `WS_EX_TOPMOST`) above it; dismiss on `WM_ACTIVATE` → `WA_INACTIVE` unless pinned.
- Keepalive without focus: `PostMessage(WM_KEYDOWN/WM_KEYUP)` to the target HWND; fall back to brief `SetForegroundWindow` + `SendInput` flick mode when a game's input loop ignores posted messages (this is the "Send without focus" toggle, §5.1 row 4).
- Window enumeration: `EnumWindows` filtered to visible, non-tool windows with a title; group by process.
- One process, single instance (named mutex); second launch focuses the existing window.

— End of spec —
