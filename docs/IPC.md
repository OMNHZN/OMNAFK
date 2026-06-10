# OMNAFK — IPC Contract (frontend ⇄ backend)

`src/index.html` calls exactly the commands and listens to exactly the events below.
The backend must implement this contract precisely.

All commands are Tauri v2 `#[tauri::command]`s registered on the flyout window's app.
All payloads are JSON (serde). Times are integer seconds.

## State snapshot (the core type)

```jsonc
{
  "engine": "dormant" | "active" | "holding" | "suspended",
  "next_tick": 412 | null,
  "error": "Couldn't send input to game.exe — …" | null,
  "games": [
    {
      "title": "ELDEN RING™",
      "exe": "eldenring.exe",
      "wclass": "FLUX",
      "verdict": "game" | "ignored",
      "overridden": false,
      "effective": "game" | "ignored",
      "gone": false,
      "uptime": 13632,
      "actions": 26,
      "profile": {
        "action": "W tap" | null,
        "interval": 60 | null,
        "key_sequence": ["SPACE", "W"]
      }
    }
  ],
  "stats": {
    "kept": 13632,
    "actions": 26,
    "seen": 3,
    "longest_streak": 7083
  },
  "config": {
    "interval": 540,
    "randomize": true,
    "action": "Space tap" | "W tap" | "Camera nudge" | "Key sequence…" | "Per-target…",
    "key_sequence": ["SPACE"],
    "send_without_focus": true,
    "hold_while_playing": true,
    "manual_mode": false,
    "sensitivity": "Strict" | "Standard" | "Broad",
    "autostart": true,
    "show_on_launch": false,
    "remember_pin": true,
    "notifications": "All" | "Errors only" | "None",
    "hotkey": "CTRL+ALT+K",
    "github_repo": "OMNHZN/OMNAFK",
    "update_channel": "Stable" | "Prerelease",
    "check_updates_on_launch": false,
    "pinned": false,
    "last_tab": "general" | "targets" | "stats" | "settings" | "about",
    "settings_updates_collapsed": false
  }
}
```

`interval` accepts any integer **10–3600** (preset labels in the UI map to these values).
`profiles` and `overrides` persist in `config.json` and round-trip through import/export.

## Commands (frontend → backend)

| Command | Args | Behavior |
|---|---|---|
| `get_state` | — | Return the full snapshot above. |
| `set_config` | `{ key, value }` | Set one config field, persist immediately, apply live, emit state. Supported keys: all `config` fields above plus `key_sequence` (string array). |
| `cycle_override` | `{ exe, wclass }` | Cycle override: none → game → ignored → none. Persist. Emit state. |
| `set_target_profile` | `{ exe, wclass, action?, interval?, key_sequence? }` | Set per-target profile overrides. `action: null` or omit clears action. `interval: null` clears interval. Emit state. |
| `rescan` | — | Force immediate detection pass. Emit state. |
| `set_suspended` | `{ suspended }` | Enter/leave SUSPENDED. Persist. Emit state. |
| `set_pinned` | `{ pinned }` | Pin/unpin flyout. Persist position when `remember_pin`. |
| `hide_flyout` | — | Hide flyout (Esc). |
| `set_hotkey` | `{ hotkey }` | Re-register global shortcut. Persist. |
| `reset_stats` | — | Zero session stats, streak, and per-game counters. Emit state. |
| `import_settings` | — | File dialog → load JSON config (includes overrides + profiles). Emit state. |
| `export_settings` | — | Save dialog → write full config JSON. |
| `check_updates` | — | Check GitHub Releases on selected channel. |
| `open_github` | — | Open configured repository. |
| `open_github_releases` | — | Open Releases page. |
| `open_github_issue` | — | Open new issue page. |
| `open_github_url` | `{ url }` | Open trusted GitHub HTTPS URL. |

## Events (backend → frontend)

| Event | Payload | Cadence |
|---|---|---|
| `omnafk://state` | full snapshot | Once per second while flyout visible, and after any command or engine transition. Pulse line fires when `stats.actions` increases. |

## Backend-only responsibilities

- Tray icon state swapping (§10.2), flyout positioning, right-click menu, detection loop, keepalive timers using per-target resolved options, first-run notification, single instance.
