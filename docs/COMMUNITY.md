# Community intelligence

Shared game profiles for OMNAFK. When enabled, the app fetches `community/manifest.json` from the connected GitHub repository (same repo as updates) and applies shared knowledge without requiring an app release. The feature is enabled by default and can be turned off from Keepalive settings.

## Manifest

Hosted at `https://raw.githubusercontent.com/{owner}/{repo}/main/community/manifest.json`.

| Section | Purpose |
|---------|---------|
| `games` | Per-exe keepalive profiles, fallback order, monitor hints, confidence |
| `detection` | Supplemental known/negative exes and path patterns |
| `ingest_url` | Optional POST endpoint for anonymized contribution uploads |

Profiles auto-apply when `confidence ≥ 0.7` and `reports ≥ 30`, and only if the user has no existing per-target profile.

## Contributions

When enabled, OMNAFK queues shared stats locally (`%APPDATA%\OMNAFK\community_queue.json`):

- Keepalive attempts/successes per exe and action
- Monitor placement outcomes
- Stability events

Rows flush to `ingest_url` when set and enough samples exist (≥ 20 attempts per action). Without an ingest endpoint, data stays on disk only. Reports can include executable names, selected actions, success counts, send mode, monitor placement outcomes, and the top learned keys.

Each install gets a random `community_client_id` — never tied to Windows identity.

## Settings

- **Community intelligence** — master toggle (`community_intelligence`)
- Uses `github_repo` for manifest source (Settings → Updates)
- Dismiss per-exe via `community_dismissed_exes` (future UI)

## Cache

Manifest is cached at `%APPDATA%\OMNAFK\community_cache.json` and refreshed every 6 hours while enabled.
