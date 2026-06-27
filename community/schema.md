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
