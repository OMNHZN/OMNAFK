# Releasing OMNAFK

OMNAFK is wired for GitHub Releases at `OMNHZN/OMNAFK`.

## One-time GitHub setup

1. Create the `OMNAFK` repository under the `OMNHZN` GitHub account.
2. Push this repository to `origin`.
3. In the GitHub repository settings, keep Issues and Actions enabled.

## Publish an update

1. Update the version in `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.
2. Commit the version bump.
3. Tag the release:

```powershell
git tag v0.1.4
git push origin main
git push origin v0.1.4
```

The `Release` workflow builds the custom `dist/OMNAFK-Setup.exe` executable and
attaches it to the GitHub release. If `docs/releases/<tag>.md` exists, those
notes become the GitHub release body. OMNAFK's Settings tab checks that release
feed and opens the newest installer asset when an update is available.

For a local release build:

```powershell
.\scripts\build-custom-installer.ps1
```

## Bug reports

The About tab's `Report a bug` row opens:

```text
https://github.com/OMNHZN/OMNAFK/issues/new?template=bug_report.yml
```
