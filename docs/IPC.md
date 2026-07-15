# OMNAFK — IPC Contract (frontend ⇄ backend)

`src/index.html` calls exactly the commands and listens to exactly the events below.
The backend must implement this contract precisely.

All commands are Tauri v2 `#[tauri::command]`s registered on the flyout window's app.
All payloads are JSON (serde). Times are integer seconds.

## State snapshot (the core type)

```jsonc
{
  "version": "0.1.20",
  "engine": "dormant" | "active" | "holding" | "suspended",
  "next_tick": 412 | null,
  "error": "Couldn't send input to game.exe — …" | null,
  "paused_reason": "QUIET HOURS" | "ON BATTERY" | "SESSION LOCKED" | "WAITING FOR IDLE" | "SAFETY CAP REACHED" | null,
  "snooze_remaining": 1740 | null,        // seconds left on an active snooze
  "community_last_error": "Couldn't fetch manifest" | null,  // when community intelligence is on
  "log": [                                 // newest first, capped at 50 entries
    { "at": "14:02:11", "kind": "action" | "target" | "engine" | "error", "text": "Sent Space tap to eldenring.exe" }
  ],
  "update": {
    "repo": "OMNHZN/OMNAFK",
    "current_version": "0.1.20",
    "latest_version": "0.1.20",
    "latest_tag": "v0.1.20",
    "title": "OMNAFK v0.1.20",
    "url": "https://github.com/OMNHZN/OMNAFK/releases/tag/v0.1.20",
    "published_at": "2026-06-10T20:00:00Z",
    "prerelease": false,
    "update_available": true,
    "asset_name": "OMNAFK-Setup.exe",
    "asset_url": "https://github.com/OMNHZN/OMNAFK/releases/download/v0.1.20/OMNAFK-Setup.exe",
    "notes_excerpt": "Short release notes excerpt.",
    "release_notes": "Longer release notes body for setup welcome screens."
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
        "elevated": false | true | null,
        "gpu_active": false,
        "gpu_usage": 42 | null,              // GPU utilization % from PDH, null when unknown
        "audio_active": false                // process is rendering audio (WASAPI)
      },
      "next_tick": 92 | null,              // per-target seconds to next keepalive
      "last_action_secs": 448 | null,      // seconds since last keepalive attempt
      "last_action_ok": true | false | null,
      "elevated_mismatch": false,          // target is admin but OMNAFK isn't
      "learned": {                         // adaptive input profile, null until first sample
        "samples": 132,
        "needed": 50,                      // samples required before activation
        "active": true,                    // keepalives currently draw from this profile
        "top": [ { "key": "W", "pct": 61 }, { "key": "SPACE", "pct": 22 } ]
      } | null,
      "health_warning": "Keepalive failing (3x) — using focus flick" | null,
      "consecutive_failures": 0,
      "success_rate": 92 | null,
      "primary_keepalive": true,
      "next_action": "Adaptive (W/SPACE)" | "Mouse wiggle (this game)" | "MOUSE WIGGLE · GENTLE" | null,
      "community": {
        "label": "Community · Space tap · 95%",
        "name": "Roblox",
        "family": "Roblox",
        "action": "Space tap",
        "interval": 540,
        "confidence": 0.95,
        "detection_confidence": 0.97,
        "action_confidence": 0.94,
        "monitor_confidence": 0.88,
        "reports": 100,
        "degraded": null,
        "status": "stable",
        "verified": "2026-06-17",
        "variants": ["Roblox Player", "Bloxstrap"],
        "reason": "Common Roblox desktop player executable with stable keepalive behavior.",
        "limitations": ["Some experiences can react differently to repeated input."],
        "top_keys": ["SPACE", "W"],
        "monitor_note": "Borderless fullscreen works best for monitor placement.",
        "applied": true
      } | null,
      "menu_hint": {                       // legacy heuristic hint; also surfaced in presence.sources
        "confidence": 78,                  // 0-100
        "reason": "GPU load 7% vs 72% gameplay peak, title back to launch state"
      } | null,
      "presence": {                        // layered in-game vs menu (log, screen, memory, heuristic)
        "state": "in_game" | "likely_menu" | "unknown",
        "confidence": 88,                  // 0-100 stable confidence after debounce
        "reason": "screen: frame variance 0.142",
        "hold_keepalives": false,          // true when respect_presence + high-confidence menu
        "sources": [
          { "layer": "screen", "state": "in_game", "confidence": 88, "detail": "frame variance 0.142" }
        ]
      } | null,
      "profile": {
        "action": "W tap" | null,
        "interval": 60 | null,
        "key_sequence": ["SPACE", "W"],
        "monitor": "Use global" | "Don't move" | null,
        "adaptive": true | false | null,
        "hold_while_playing": true | false | null,
        "hold_window_secs": 30 | 60 | 300 | null,
        "send_without_focus": true | false | null,
        "auto_fallback": true | false | null,
        "safe_actions": true | null
      },
      "monitor": {
        "target": "Monitor 2 (1920×1080)" | null,
        "status": "On target" | "Moved" | "Waiting (active)" | "Monitor disconnected" | "Move failed" | null
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
    "suspected_kicks": { "Space tap": 1 },
    "daily": [ { "date": "2026-06-10", "seen": 2, "actions": 26, "kept": 13632 } ],
    "lifetime_games": [ { "identity": "eldenring.exe\u001fFLUX", "title": "ELDEN RING™", "kept": 86400, "actions": 412 } ]
  },
  "config": {
    "interval": 540,
    "randomize": true,
    "jitter_pct": 5 | 15 | 30,
    "action": "Space tap" | "W tap" | "Camera nudge" | "Mouse wiggle" | "Scroll tick" | "Right click" | "Gamepad nudge" | "Key sequence…" | "Per-target…",
    "adaptive_actions": true,
    "auto_fallback": true,
    "adaptive_min_samples": 20 | 50 | 100 | 200,
    "adaptive_learn_sequences": true,
    "adaptive_learn_actions": true,
    "adaptive_interval": true,            // warm-up ease-in: faster first few ticks, then relax
    "burst_detection": true,
    "keep_all_instances": false,          // multi-boxing: keep every window of a game awake
    "rotate_actions": false,              // cycle W tap / Space tap / camera nudge each tick
    "gamepad_kind": "Xbox 360" | "DualShock 4", // virtual pad type for the Gamepad nudge action
    "headless": false,
    "community_intelligence": false,
    "presence_log_enabled": true,           // manifest log tail (requires cached manifest rules)
    "presence_screen_enabled": true,        // local window thumbnail variance
    "presence_memory_enabled": false,       // manifest memory reads (expert opt-in; at own risk)
    "respect_presence": true,               // hold keepalives on high-confidence menu/lobby
    "always_mark_exes": ["mygame.exe"],
    "always_ignore_exes": ["zoom.exe"],
    "mark_title_contains": ["my game"],   // title substrings (lowercased) that force-mark
    "ignore_title_contains": ["launcher"],// title substrings (lowercased) that force-ignore
    "monitor_placement": false,
    "monitor_device": null,
    "monitor_when": "Always" | "On launch",
    "monitor_style": "Preserve size" | "Maximize" | "Fill work area" | "Fill monitor",
    "monitor_skip_active": true,
    "monitor_skip_active_secs": 3 | 5 | 10 | 30,
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
    "quiet_days": "Every day" | "Weekdays" | "Weekends",
    "manual_mode": false,
    "sensitivity": "Strict" | "Standard" | "Broad",
    "autostart": true,
    "autostart_status": "ok" | "missing" | "mismatch" | "disabled",
    "user_presets": ["My GTA setup", "Long AFK"],
    "show_on_launch": false,
    "remember_pin": true,
    "notifications": "All" | "Errors only" | "None",
    "remote_alerts": false,
    "ntfy_topic": "",
    "discord_webhook": "",
    "hotkey": "CTRL+ALT+K",
    "suspend_hotkey": "CTRL+ALT+S" | "",          // empty string = no suspend hotkey
    "github_repo": "OMNHZN/OMNAFK",
    "check_updates_on_launch": true,
    "ignored_update_tag": "v0.1.20" | null,
    "pinned": false,
    "last_tab": "general" | "targets" | "stats" | "settings" | "about",
    "settings_interface_collapsed": true,
    "settings_updates_collapsed": false,
    "general_advanced_collapsed": true,
    "target_view": "All" | "Clean" | "Games only",
    "target_sort": "Status" | "Name",
    "favorite_targets": ["game.exeWindowClass"],
    "target_density": "Compact" | "Comfortable",
    "tab_label_mode": "Active only" | "Always" | "Icons only",
    "theme": "Dark" | "High contrast",
    "version_display": "Title + About" | "About only" | "Hidden",
    "safety_note_display": "Compact" | "Full" | "Hidden",
    "update_prompt_mode": "Automatic" | "Card + toast" | "Card only" | "Manual only",
    "file_logging": false,
    "tour_done": false,
    "armed_overrides": [ { "exe": "eldenring.exe", "wclass": "FLUX" } ]  // derived: overrides pinned to "game"
  }
}
```

`interval` accepts any integer **10–3600** (preset labels in the UI map to these values).
`quiet_start` / `quiet_end` are `HH:MM` 24-hour strings; the window may wrap midnight. `quiet_days` scopes the window to `Every day`, `Weekdays`, or `Weekends`, evaluated for the current local day (a wrapping window ends when the new day no longer matches).
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
| `test_all_targets` | — | Fire a test keepalive at every active target. Returns `{ tested, ok, failed }` and emits state. |
| `explain_detection` | `{ exe, wclass }` | Explain a window's verdict. Returns `{ title, exe, score, threshold, sensitivity, score_verdict, effective, factors[], reason }`. Read-only, computed on demand (no state emit). |
| `test_alert` | — | Send a test away-from-keyboard alert to the configured ntfy/Discord channels. Returns a summary string or an error. No state emit. |
| `reset_learning` | `{ exe, wclass }` | Wipe the adaptive input profile learned for that target. Persist. Emit state. |
| `list_monitors` | — | Return connected displays: `[{ device, label, primary, width, height }]`. |
| `set_target_profile` | `{ exe, wclass, action?, interval?, key_sequence?, monitor?, adaptive?, hold_while_playing?, hold_window_secs?, send_without_focus?, auto_fallback?, sensitivity?, safe_actions? }` | Set per-target profile overrides. `action: null` or omit clears action. `interval: null` clears interval. `monitor: null` or `"Use global"` uses global monitor rule; `"Don't move"` skips this target; otherwise pass a `device` string from `list_monitors`. `adaptive`, `hold_while_playing`, `send_without_focus`, and `auto_fallback`: `null` uses global; `true`/`false` overrides per target. `hold_window_secs`: `null` uses global; otherwise 10–3600 seconds. `sensitivity`: `null` or `"Use global"` uses global detection sensitivity; otherwise `"Strict"`, `"Standard"`, or `"Broad"` overrides the auto-detect threshold for this target only. `safe_actions: true` caps the target to pointer-based keepalives; `null` clears the cap. Emit state. |
| `move_target` | `{ exe, wclass }` | Move one tracked window onto its configured monitor immediately. Emit state. |
| `apply_preset` | `{ name }` | Apply a named preset (`Walking simulator`, `Long interval (Space)`, `Camera AFK`, `Mouse only`). Legacy alias `Roblox` maps to Long interval (Space). Persist. Emit state. |
| `save_user_preset` | `{ name }` | Snapshot current global keepalive settings — including monitor placement (on/off, style, timing, skip-while-active) — into a user-named preset (max 10). Persist. Emit state. |
| `apply_user_preset` | `{ name }` | Apply a saved user preset. Persist. Emit state. |
| `delete_user_preset` | `{ name }` | Remove a saved user preset. Persist. Emit state. |
| `dismiss_community_profile` | `{ exe }` | Add exe to `community_dismissed_exes` so community auto-apply stops for that game. Persist. Emit state. |
| `apply_community_profile` | `{ exe, wclass }` | Apply the shared profile to that target, update global community hints, persist, and emit state. |
| `community_feedback` | `{ exe, feedback }` | Queue simple community profile feedback. `feedback` is `worked`, `wrong_game`, or `wrong_action`. Emit state. |
| `share_community_profile` | `{ exe, game, action?, interval?, notes? }` | Open a prefilled `community_profile.yml` GitHub issue (via the configured `github_repo`) so the user can contribute a working profile. Fields are percent-encoded; the app version is appended automatically. Returns nothing; opens the browser. |
| `list_presets` | — | Return available built-in preset names. |
| `restart_as_admin` | — | Relaunch OMNAFK elevated via UAC, then exit the current instance. |
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
| `check_updates` | — | Start a background GitHub Releases check. Returns `{ started: bool }` immediately; emits `omnafk://update-check` with `{ kind: "result", check }` or `{ kind: "error", message }` when finished. |
| `run_app_update` | — | Download the pending setup installer, verify SHA256 when available, launch it, and exit OMNAFK. |
| `get_changelog` | — | Return release notes for the latest few releases: `[{ tag, title, published_at, body }]`. |
| `ignore_update` | `{ tag }` | Hide and remember the current update tag until a newer tag appears. |
| `open_github` | — | Open configured repository. |
| `open_github_releases` | — | Open Releases page. |
| `open_github_issue` | — | Open new issue page. |
| `open_github_url` | `{ url }` | Open trusted GitHub HTTPS URL. |
| `get_tray_menu_state` | — | Return tray menu summary lines for the custom tray menu window. |
| `tray_menu_action` | `{ action }` | Run a tray menu action (`open`, `settings`, `updates`, `bug`, `toggle_suspend`, `quit`). |
| `hide_tray_menu` | — | Hide the custom tray menu window. |
| `toast_action` | `{ action }` | Run an in-flyout toast action (`open_flyout`, `restart_admin`). |

## Events (backend → frontend)

| Event | Payload | Cadence |
|---|---|---|
| `omnafk://state` | full snapshot | Once per second while flyout visible, and after any command or engine transition. Pulse line fires when `stats.actions` increases. |
| `omnafk://toast` | `{ text, kind: "info" \| "error" \| "success", action?: "open_flyout" \| "restart_admin", duration_ms }` | When a notice is delivered while the flyout (or tray toast window) is visible. Errors use `duration_ms: 0` (sticky until dismissed). |
| `omnafk://open-settings` | `"settings"` \| `"updates"` | When the tray menu opens Settings or Updates. |
| `omnafk://tray-menu-state` | `{ state, next, targets, suspend_label }` | Once per second while the tray menu is visible. |

## Backend-only responsibilities

- Tray icon state swapping (§10.2), flyout positioning, custom tray menu, toast delivery (in-app when the flyout is open, native Windows notification when closed), detection loop, keepalive timers using per-target resolved options, first-run walkthrough, single instance.
- Gating: keepalives hold (engine reports `paused_reason`) during quiet hours, on battery, while the session is locked, until the idle threshold passes, or once a session safety cap is hit.
- Suspend hotkey toggles SUSPENDED globally; snooze timers resume the engine automatically.
- Notices for armed/lost targets, first keepalive, and errors, honoring the `notifications` setting. The flyout renders action buttons while open; otherwise notices go through Windows notification history.

