# Community Profiles

OMNAFK can use shared game profiles when **Community intelligence** is enabled in Settings. The feature is opt-in. Local marks, local learned keys, and local settings always remain the user's source of truth.

## Source

The app reads the official manifest from:

```text
https://raw.githubusercontent.com/OMNHZN/OMNAFK/main/community/manifest.json
```

The update repository setting does not change the community manifest source. This keeps profiles predictable and avoids untrusted profile feeds.

## What Profiles Can Do

| Section | Purpose |
| --- | --- |
| `games` | Per-exe keepalive action, timing, fallback order, monitor hints, confidence, and report count |
| `detection` | Supplemental known game executables, path patterns, and negative matches |
| `ingest_url` | Optional trusted endpoint for queued contribution uploads |

Profiles can auto-apply only when confidence is at least `0.70`, reports are at least `30`, the user has enabled Community intelligence, and the target does not already have a local profile.

## Contributions

When Community intelligence is enabled, OMNAFK may queue shared stats locally at:

```text
%APPDATA%\OMNAFK\community_queue.json
```

Queued rows can include executable names, selected actions, success counts, send mode, monitor placement outcomes, and learned movement keys. Rows are uploaded only when `ingest_url` is set to a trusted endpoint and there are enough samples. Without an ingest endpoint, the queue stays local.

Each install gets a random community client ID after the feature is enabled. It is not tied to a Windows account.

## Profile Files

Human-readable profile notes live in `community/profiles/`. The app consumes `community/manifest.json`; the profile files explain why a profile exists, what was verified, and what limits are known.

Run validation before publishing profile changes:

```powershell
.\scripts\validate-community.ps1
```

## Cache

The manifest cache is stored at:

```text
%APPDATA%\OMNAFK\community_cache.json
```

OMNAFK refreshes the cache every 6 hours while Community intelligence is enabled.
