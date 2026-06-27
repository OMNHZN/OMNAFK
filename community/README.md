# OMNAFK Community Profiles

This folder holds the shared profile feed used by OMNAFK when Community intelligence is enabled.

- `manifest.json` is the file the app downloads.
- `profiles/` contains human-readable notes for each profile.
- `schema.md` explains the fields and review rules.

Community profiles are optional. They should improve detection and defaults without overriding a user's local choices.

## Review Rules

- Keep executable names lowercase.
- Use conservative actions that are unlikely to disrupt gameplay.
- Require enough evidence before raising confidence.
- Explain limitations instead of hiding them.
- Run validation before publishing:

```powershell
.\scripts\validate-community.ps1
```
