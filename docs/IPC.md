# OMNAFK — IPC Contract (frontend ⇄ backend)

`src/index.html` calls exactly the commands and listens to exactly the events below.
The backend must implement this contract precisely.

All commands are Tauri v2 `#[tauri::command]`s registered on the flyout window's app.
All payloads are JSON (serde). Times are integer seconds.

## State snapshot (the core type)

```jsonc
{
  "version": "0.1.2",
  "engine": "dormant" | "active" | "holding" | "suspended",
  "next_tick": 412 | null,
  "error": "Couldn't send input to game.exe — …" | null,
  "paused_reason": "QUIET HOURS" | "ON BATTERY" | "SESSION LOCKED" | "WAITING FOR IDLE" | "SAFETY CAP REACHED" | null,
  "snooze_remaining": 1740 | null,        // seconds left on an active snooze
  "log": [                                 // newest first, capped at 50 entries
    { "at": "14:02:11", "kind": "action" | "target" | "engine" | "error", "text": "Sent Space tap to eldenring.exe" }
  ],
  "update": {
    "repo": "OMNHZN/OMNAFK",
    "channel": "Stable",
    "current_version": "0.1.2",
    "latest_version": "0.1.7",
    "latest_tag": "v0.1.7",
    "title": "OMNAFK v0.1.7",
    "url": "https://github.com/OMNHZN/OMNAFK/releases/tag/v0.1.7",
    "published_at": "2026-06-10T20:00:00Z",
    "prerelease": false,
    "update_available": true,
    "asset_name": "OMNAFK-Setup.exe",
    "asset_url": "https://github.com/OMNHZN/OMNAFK/releases/download/v0.1.7/OMNAFK-Setup.exe",
    "notes_excerpt": "Short release notes excerpt."
  } | null,
  "games": [
    {
      "title": "ELDEN RING™",
      "exe": "eldenring.exe",
      "wclass": "FLUX",
      "verdict": "game" | "ignored",
      "overridden": false,
      "effective": "game" | "ignored",
      "gone": false,
      "paused": false,                     // per-target pause (config.paused)
      "uptime": 13632,
      "actions": 26,
      "score": 5,                          // detection score vs threshold
      "threshold": 4,
      "facts": {                           // detection facts behind the verdict
        "fullscreen": true, "borderless": false, "gfx_dll": true,
        "platform_path": true, "known_game": false, "negative_class": false,
        "elevated": false | true | null
      },
      "next_tick": 92 | null,              // per-target seconds to next keepalive
      "last_action_secs": 448 | null,      // seconds since last keepalive attempt
      "last_action_ok": true | false | null,
      "elevated_mismatch": false,          // target is admin but OMNAFK isn't
      "profile": {
        "action": "W tap" | null,
        "interval": 60 | null,
        "key_sequence": ["SPACE", "W"]
      }
    }
  ],
  "stats": {
    "kept": 13632,                         // session
    "actions": 26,                         // session
    "seen": 3,
    "current_streak": 3120,
    "longest_streak": 7083,
    "lifetime_kept": 86400,                // persisted across restarts
    "lifetime_actions": 412,
    "actions_by_type": { "Space tap": 20, "Mouse wiggle": 6 },
    "daily": [ { "date": "2026-06-10", "seen": 2, "actions": 26, "kept": 13632 } ],
    "lifetime_games": [ { "identity": "eldenring.exe\u001fFLUX", "title": "ELDEN RING™", "kept": 86400, "actions": 412 } ]
  },
  "config": {
    "interval": 540,
    "randomize": true,
    "jitter_pct": 5 | 15 | 30,
    "action": "Space tap" | "W tap" | "Camera nudge" | "Mouse wiggle" | "Scroll tick" | "Right click" | "Key sequence…" | "Per-target…",
    "key_sequence": ["SPACE"],
    "send_without_focus": true,
    "hold_while_playing": true,
    "hold_window_secs": 30 | 60 | 300,
    "idle_threshold_mins": 0 | 2 | 5 | 10 | 30,   // 0 = off
    "pause_on_battery": false,
    "pause_when_locked": false,
    "max_session_hours": 0 | 4 | 8 | 12 | 24,     // 0 = off
    "max_session_actions": 0 | 100 | 500 | 1000 | 5000,
    "quiet_hours_enabled": false,
    "quiet_start": "23:00",
    "quiet_end": "07:00",
    "manual_mode": false,
    "sensitivity": "Strict" | "Standard" | "Broad",
    "autostart": true,
    "show_on_launch": false,
    "remember_pin": true,
    "notifications": "All" | "Errors only" | "None",
    "hotkey": "CTRL+ALT+K",
    "suspend_hotkey": "CTRL+ALT+S" | "",          // empty string = no suspend hotkey
    "github_repo": "OMNHZN/OMNAFK",
    "update_channel": "Stable",
    "check_updates_on_launch": false,
    "ignored_update_tag": "v0.1.7" | null,
    "pinned": false,
    "last_tab": "general" | "targets" | "stats" | "settings" | "about",
    "settings_interface_collapsed": true,
    "settings_updates_collapsed": false,
    "general_advanced_collapsed": true,
    "target_view": "All" | "Clean" | "Games only",
    "target_sort": "Status" | "Name",
    "target_density": "Compact" | "Comfortable",
    "tab_label_mode": "Active only" | "Always" | "Icons only",
    "version_display": "Title + About" | "About only" | "Hidden",
    "safety_note_display": "Compact" | "Full" | "Hidden",
    "update_prompt_mode": "Card + toast" | "Card only" | "Manual only",
    "accent": "Mono" | "Ice" | "Ember" | "Acid" | "Violet",
    "file_logging": false,
    "tour_done": false,
    "armed_overrides": [ { "exe": "eldenring.exe", "wclass": "FLUX" } ]  // derived: overrides pinned to "game"
  }
}
```

`interval` accepts any integer **10–3600** (preset labels in the UI map to these values).
`quiet_start` / `quiet_end` are `HH:MM` 24-hour strings; the window may wrap midnight.
`profiles`, `overrides`, and `paused` persist in `config.json` and round-trip through import/export.
Lifetime statistics persist separately in `stats.json` next to the config.

## Commands (frontend → backend)

| Command | Args | Behavior |
|---|---|---|
| `get_state` | — | Return the full snapshot above. |
| `set_config` | `{ key, value }` | Set one config field, persist immediately, apply live, emit state. Supported keys: all `config` fields above (except derived `armed_overrides`) plus `key_sequence` (string array). |
| `cycle_override` | `{ exe, wclass }` | Cycle override: none → game → ignored → none. Persist. Emit state. |
| `set_override` | `{ exe, wclass, verdict }` | Set an override directly (`"game"`, `"ignored"`, or `null` to clear). Persist. Emit state. |
| `clear_overrides` | — | Remove every manual override. Persist. Emit state. |
| `pause_target` | `{ exe, wclass, paused }` | Pause/resume keepalives for one target without changing its verdict. Persist. Emit state. |
| `test_target` | `{ exe, wclass }` | Send the resolved keepalive action to that window right now. Emit state. |
| `set_target_profile` | `{ exe, wclass, action?, interval?, key_sequence? }` | Set per-target profile overrides. `action: null` or omit clears action. `interval: null` clears interval. Emit state. |
| `rescan` | — | Force immediate detection pass. Emit state. |
| `set_suspended` | `{ suspended }` | Enter/leave SUSPENDED. Persist. Emit state. |
| `snooze` | `{ minutes }` | Suspend for N minutes, auto-resume after (0 cancels an active snooze). Emit state. |
| `set_pinned` | `{ pinned }` | Pin/unpin flyout. Persist position when `remember_pin`. |
| `hide_flyout` | — | Hide flyout (Esc). |
| `set_hotkey` | `{ hotkey }` | Re-register global shortcut. Persist. |
| `reset_stats` | — | Zero session stats, streak, and per-game counters. Emit state. |
| `export_stats` | — | Save dialog → write session + lifetime statistics as CSV. |
| `import_settings` | — | File dialog → load JSON config (includes overrides + profiles). Emit state. |
| `export_settings` | — | Save dialog → write full config JSON. |
| `reset_settings` | — | Restore every config field to defaults (overrides/profiles cleared). Emit state. |
| `open_config_dir` | — | Open the config folder in Explorer. |
| `open_log_file` | — | Open the activity log file (errors if `file_logging` never wrote one). |
| `diagnostics` | — | Return a plain-text diagnostics report (version, OS, engine state, config summary, targets). |
| `check_updates` | — | Check stable GitHub Releases. |
| `get_changelog` | — | Return release notes for the latest few releases: `[{ tag, title, published_at, body }]`. |
| `ignore_update` | `{ tag }` | Hide and remember the current update tag until a newer tag appears. |
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
- Gating: keepalives hold (engine reports `paused_reason`) during quiet hours, on battery, while the session is locked, until the idle threshold passes, or once a session safety cap is hit.
- Suspend hotkey toggles SUSPENDED globally; snooze timers resume the engine automatically.
- Windows toast notices for armed/lost targets and errors, honoring the `notifications` setting.
