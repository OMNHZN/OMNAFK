# Releasing OMNAFK

OMNAFK is wired for GitHub Releases at `OMNHZN/OMNAFK`.

## One-time GitHub setup

1. Create the `OMNAFK` repository under the `OMNHZN` GitHub account.
2. Push this repository to `origin`.
3. In the GitHub repository settings, keep Issues and Actions enabled.
4. Set the social preview image to `assets/github/social-preview.png`.
5. Enable Discussions if you want users to share game-specific settings, profile suggestions, and setup questions outside the issue tracker.

## Publish an update

1. Update the version in `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.
2. Commit the version bump.
3. Tag the release:

```powershell
git tag v0.1.20
git push origin main
git push origin v0.1.20
```

The `Release` workflow builds the custom `dist/OMNAFK-Setup.exe` executable and
attaches it to the GitHub release. If `docs/releases/<tag>.md` exists, those
notes become the GitHub release body. OMNAFK's Settings tab checks that release
feed and opens the newest installer asset when an update is available.

For a local release build:

```powershell
.\scripts\build-custom-installer.ps1
```

The build writes `dist\OMNAFK-Setup.exe`. Release builds also attach `OMNAFK-Setup.exe.sha256` for integrity verification.

## Silent install and uninstall

The custom setup supports unattended runs (no UI):

```powershell
# Fresh install or in-place update (uses registered folder when updating)
.\dist\OMNAFK-Setup.exe --silent

# Uninstall from the registered copy (keeps settings in %APPDATA%\OMNAFK)
& "$env:LOCALAPPDATA\OMNAFK\omnafk-setup.exe" --uninstall --silent
```

Re-running `OMNAFK-Setup.exe` when OMNAFK is already installed opens an **update** flow when the setup version is newer, **reinstall** when versions match, or **downgrade** when the setup version is older.

Additional setup flags:

```powershell
# Override install folder (fresh install or silent)
.\dist\OMNAFK-Setup.exe --silent --install-dir "D:\Apps\OMNAFK"

# Skip Start-with-Windows registration during silent install
.\dist\OMNAFK-Setup.exe --silent --no-autostart

# Desktop shortcut toggles
.\dist\OMNAFK-Setup.exe --silent --desktop-shortcut
.\dist\OMNAFK-Setup.exe --silent --no-desktop-shortcut

# Uninstall and remove settings
& "$env:LOCALAPPDATA\OMNAFK\omnafk-setup.exe" --uninstall --silent --no-keep-settings

# Confirm rolling back to an older build
.\dist\OMNAFK-Setup.exe --silent --allow-downgrade
```

Setup writes a plain-text log to `%APPDATA%\OMNAFK\install.log`. When OMNAFK updates itself from Settings, it downloads the release installer, verifies the optional `.sha256` sidecar, launches setup, and exits so the new build can replace the running app.

## GitHub presentation

- Keep public screenshots from the real desktop app.
- Keep local mockups and workspace files out of the public repository.
- Keep README links focused on download, website, guide, troubleshooting, and bug reports.
- Keep only the newest GitHub Pages deployment. The `Pages Cleanup` workflow runs after successful Pages deploys and can also be run manually.

## Bug reports

The About tab's `Report a bug` row opens:

```text
https://github.com/OMNHZN/OMNAFK/issues/new?template=bug_report.yml
```

