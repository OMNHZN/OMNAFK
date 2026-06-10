<p align="center">
  <img src="assets/github/logo.png" width="118" alt="OMNAFK logo">
</p>

<h1 align="center">OMNAFK</h1>

<p align="center">
  <strong>Awake when you aren't.</strong><br>
  A tray-first Windows utility that detects running games and sends quiet keepalive input when they need it.
</p>

<p align="center">
  <a href="https://github.com/OMNHZN/OMNAFK/releases">
    <img alt="GitHub release" src="https://img.shields.io/github/v/release/OMNHZN/OMNAFK?include_prereleases&label=release&style=for-the-badge&color=ffffff&labelColor=111111">
  </a>
  <a href="https://github.com/OMNHZN/OMNAFK/actions/workflows/release.yml">
    <img alt="Release workflow" src="https://img.shields.io/github/actions/workflow/status/OMNHZN/OMNAFK/release.yml?style=for-the-badge&label=release%20build&labelColor=111111">
  </a>
  <img alt="Windows" src="https://img.shields.io/badge/windows-10%20%7C%2011-ffffff?style=for-the-badge&labelColor=111111">
  <a href="LICENSE">
    <img alt="License" src="https://img.shields.io/badge/license-MIT-ffffff?style=for-the-badge&labelColor=111111">
  </a>
</p>

<p align="center">
  <a href="https://github.com/OMNHZN/OMNAFK/releases/latest"><strong>Download setup</strong></a>
  ·
  <a href="https://omnhzn.github.io/OMNAFK/">Website</a>
  ·
  <a href="https://github.com/OMNHZN/OMNAFK/issues/new?template=bug_report.yml">Report a bug</a>
</p>

<table>
  <tr>
    <td width="50%">
      <img src="assets/github/flyout.png" alt="OMNAFK tray flyout">
    </td>
    <td width="50%">
      <img src="assets/github/setup.png" alt="OMNAFK custom setup">
    </td>
  </tr>
</table>

## What It Does

OMNAFK watches your visible windows, decides which ones look like games, and keeps them awake without asking you to babysit another control panel. There is no arm button and no start button. It runs from the tray, wakes when a game does, and goes dormant when there is nothing to do.

- Detects fullscreen, borderless, and game-platform windows.
- Sends keepalive input with `PostMessage` by default, avoiding focus theft.
- Skips ticks while you are actively playing.
- Lets you override any detected window as `GAME` or `IGNORED`.
- Saves settings immediately to `%APPDATA%\OMNAFK\config.json`.
- Checks GitHub Releases for future updates from Settings.
- Opens the GitHub bug-report form directly from the app.

## Install

Download the latest `OMNAFK-Setup.exe` from [Releases](https://github.com/OMNHZN/OMNAFK/releases/latest), run it, and leave **Start with Windows** enabled if you want OMNAFK to live quietly in the tray.

The custom setup executable is also produced locally at:

```text
dist\OMNAFK-Setup.exe
```

## Updates And Bug Reports

The app is wired to this repository by default:

```text
OMNHZN/OMNAFK
```

The Settings tab can check GitHub Releases, open the Releases page, open the repository, and open a structured bug report. Publishing a new tag such as `v0.1.1` runs the release workflow and uploads the custom setup executable for installed copies to find.

## Build

```powershell
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
cargo tauri build
cd ..
.\scripts\build-custom-installer.ps1
```

## Release

```powershell
git tag v0.1.1
git push origin main
git push origin v0.1.1
```

GitHub Actions builds the Windows installer and publishes it to GitHub Releases. See [docs/RELEASING.md](docs/RELEASING.md) for the full release checklist.

## Safety Note

Sending automated input may violate the terms of service of some games or platforms. Use OMNAFK at your own discretion.

## License

MIT. See [LICENSE](LICENSE).
