# OMNAFK — IPC Contract (frontend ⇄ backend)

`src/index.html` is FINISHED and calls exactly the commands and listens to exactly the
events below. The backend must implement this contract precisely. Do not rename, do not
change shapes, do not edit the frontend except to fix a demonstrable wiring bug.

All commands are Tauri v2 `#[tauri::command]`s registered on the `flyout` window's app.
All payloads are JSON (serde). Times are integer seconds.

## State snapshot (the core type)

```jsonc
{
  "engine": "dormant" | "active" | "holding" | "suspended",
  "next_tick": 412 | null,          // seconds until next keepalive (null unless active)
  "games": [                         // every relevant visible window, games first
    {
      "title": "ELDEN RING™",
      "exe": "eldenring.exe",
      "wclass": "FLUX",             // Win32 window class, used with exe as identity
      "verdict": "game" | "ignored",     // detector's automatic call (§6.0)
      "overridden": false,               // true if the user pinned this verdict
      "effective": "game" | "ignored",   // override ?? verdict — what actually applies
      "gone": false,                     // closed <60s ago, still listed dimmed (§5.2)
      "uptime": 13632,                   // seconds kept alive this session
      "actions": 26                      // keepalives sent this session
    }
  ],
  "stats": { "kept": 13632, "actions": 26, "seen": 3 },   // session totals + games seen today
  "config": {
    "interval": 540,                  // seconds (30|60|120|300|540|840)
    "randomize": true,                // ±15% jitter (§5.1)
    "action": "Space tap" | "W tap" | "Camera nudge",
    "send_without_focus": true,       // PostMessage path; false = focus-flick (§11)
    "hold_while_playing": true,       // skip ticks if user touched game <60s ago
    "manual_mode": false,             // Targets AUTO/MANUAL segment (§5.2)
    "sensitivity": "Strict" | "Standard" | "Broad",
    "autostart": true,
    "show_on_launch": false,
    "remember_pin": true,
    "notifications": "All" | "Errors only" | "None",
    "hotkey": "CTRL+ALT+K",
    "github_repo": "OMNHZN/OMNAFK",
    "update_channel": "Stable" | "Prerelease",
    "check_updates_on_launch": false,
    "pinned": false                   // flyout pin state (persisted)
  }
}
```

## Commands (frontend → backend)

| Command | Args | Behavior |
|---|---|---|
| `get_state` | — | Return the full snapshot above. |
| `set_config` | `{ key: string, value: bool \| number \| string }` | Set one config field, persist immediately to `%APPDATA%\OMNAFK\config.json`, apply live (e.g. `interval` reschedules timers; `autostart` writes/removes the run key; `manual_mode` re-evaluates targets). Emit a fresh state event. |
| `cycle_override` | `{ exe, wclass }` | Cycle that identity's override: none → `game` → `ignored` → none (§5.2). Persist overrides. Emit state. |
| `rescan` | — | Force an immediate detection pass. Emit state. |
| `set_suspended` | `{ suspended: bool }` | Enter/leave SUSPENDED (§3.5). Persisted. Swap tray icon. Emit state. |
| `set_pinned` | `{ pinned: bool }` | Pin/unpin the flyout (§3.2): pinned ⇒ ignore blur-dismiss, allow drag; persist position if `remember_pin`. |
| `hide_flyout` | — | Hide the flyout window (Esc path). |
| `set_hotkey` | `{ hotkey: string }` | Re-register the global open-flyout shortcut; persist. Format: `CTRL+ALT+K`. |
| `reset_stats` | — | Zero session stats and per-game counters. Emit state. |
| `import_settings` | — | Open a file dialog, load JSON config (validate!), apply + persist. Emit state. |
| `export_settings` | — | Save dialog, write current config JSON. |
| `check_updates` | — | Check the configured GitHub Releases repo on the selected channel. Return current/latest version metadata, release URL, and the preferred installer asset URL when one is available. |
| `open_github` | — | Open the configured GitHub repository in the user's browser. |
| `open_github_releases` | — | Open the configured repository's Releases page. |
| `open_github_issue` | — | Open the configured repository's new issue page. |
| `open_github_url` | `{ url: string }` | Open a trusted `https://github.com/...` URL returned by update checking, such as the latest release or setup asset. |

## Events (backend → frontend)

| Event | Payload | Cadence |
|---|---|---|
| `omnafk://state` | full snapshot | Once per second while the flyout is visible, and immediately after any command or engine transition. (The frontend detects keepalive ticks by `stats.actions` increasing — it fires the pulse-line sweep itself; no separate tick event needed.) |

## Backend-only responsibilities (no frontend involvement)

- Tray icon + state swapping per §10.2 using `icons/sentinel-{dormant,active,suspended}.ico`
  (HOLDING = alternate active/dormant at 1 Hz). Left-click toggles flyout above the tray
  (§3 positioning, all four taskbar edges, clamped to work area). Right-click menu §6.2.
- Dismiss flyout on focus loss unless pinned; slide-up handled by simply showing the
  window (CSS animation would re-run; acceptable to show instantly).
- Detection loop every 5 s per §6.0; keepalive timers per game per §5.1/§11.
- First-run toast/notification per §7.4. Single instance. Suspend + overrides + pin
  position persisted in config.json.
