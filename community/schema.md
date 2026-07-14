# Community Profile Schema

`community/manifest.json` is intentionally small enough for the app to parse quickly. Profile files can carry more explanation for review.

## Manifest Fields

| Field | Required | Notes |
| --- | --- | --- |
| `version` | Yes | Positive integer schema version |
| `updated` | Yes | `YYYY-MM-DD` date |
| `ingest_url` | Yes | `null` unless a trusted endpoint is active |
| `games` | Yes | Object keyed by lowercase executable name |
| `detection` | Yes | Known executables, path hints, and negative matches |

## Game Fields

| Field | Required | Notes |
| --- | --- | --- |
| `action` | Yes | One of `Space tap`, `W tap`, `Camera nudge`, `Mouse wiggle`, `Scroll tick`, `Right click` |
| `interval` | Yes | Seconds between keepalive opportunities |
| `confidence` | Yes | Number from `0` to `1` |
| `detection_confidence` | No | Detection match confidence from `0` to `1` |
| `action_confidence` | No | Keepalive action confidence from `0` to `1` |
| `monitor_confidence` | No | Monitor placement confidence from `0` to `1` |
| `reports` | Yes | Count of supporting reports or verified samples |
| `status` | Yes | `stable`, `watch`, or `degraded` |
| `verified` | No | `YYYY-MM-DD` date shown in the app |
| `variants` | No | Supported launcher or install variants |
| `fallback_order` | No | `FocusFlick`, `CameraNudge`, or `Normal` |
| `monitor_style` | No | Short monitor placement hint |
| `monitor_note` | No | Plain-language placement note |
| `presence` | No | Layered in-game vs menu detection rules (see below) |

## Presence (optional)

Optional `presence` object on a game entry drives log tailing, screen sampling, and memory reads. Heuristic GPU/title detection always runs locally; manifest layers add higher-trust signals.

```json
"presence": {
  "log": {
    "paths": ["%APPDATA%\\.minecraft\\logs\\latest.log"],
    "poll_secs": 3,
    "in_game": ["joined the game"],
    "menu": ["main menu", "disconnecting"]
  },
  "screen": {
    "sample_w": 96,
    "sample_h": 54,
    "interval_secs": 8,
    "variance_max_menu": 0.018,
    "variance_min_game": 0.045
  },
  "memory": {
    "reads": [{
      "module": "game.exe",
      "offset": 0,
      "signature": "48 8B ?? 05",
      "offset_from_match": 4,
      "size": 4,
      "in_game_values": [1],
      "menu_values": [0]
    }]
  }
}
```

| Sub-field | Notes |
| --- | --- |
| `log.paths` | File or glob; `%ENV%` expansion supported |
| `log.in_game` / `log.menu` | Case-insensitive substrings; most recent match wins |
| `screen` | Local thumbnail variance; defaults apply when omitted |
| `memory.reads` | Expert-only; fixed offset or signature scan |

## Profile Files

Each file in `community/profiles/` should include:

- `exe`
- `display_name`
- `recommended_action`
- `recommended_interval`
- `confidence`
- `status`
- `verified`
- `notes`

The validator checks that each profile points to a game in `manifest.json`.
