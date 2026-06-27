# Contributing

Thanks for helping improve OMNAFK. Keep changes focused, test them locally, and use the issue templates when reporting behavior.

## Local Checks

Run these before opening a pull request:

```powershell
.\scripts\validate-community.ps1
cd src-tauri
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo deny check
```

## Community Profiles

Community profiles live in `community/`. The app downloads `community/manifest.json`; profile files in `community/profiles/` explain the reasoning behind each entry.

Profile changes should:

- Use lowercase executable names.
- Keep actions conservative.
- Include confidence and report counts.
- Explain limitations clearly.
- Pass `.\scripts\validate-community.ps1`.

## UI Changes

OMNAFK is a tray-first app. Keep screens compact, quiet, and easy to scan. Avoid adding explanatory blocks inside the app when a clear label, tooltip, or user-guide entry will do.

## Release Notes

Write release notes for users, not for implementation history. Include what changed, why it matters, and anything users should check after updating.
