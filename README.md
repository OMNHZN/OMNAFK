<p align="center">
  <img src="assets/github/logo.png" width="104" alt="OMNAFK logo">
</p>

<h1 align="center">OMNAFK</h1>

<p align="center">
  <strong>Awake when you aren't.</strong><br>
  A tray-first Windows utility that detects games automatically and sends quiet keepalive input when they need it.
</p>

<p align="center">
  <a href="https://github.com/OMNHZN/OMNAFK/releases/latest"><strong>Download for Windows</strong></a>
  ·
  <a href="https://omnhzn.github.io/OMNAFK/">Website</a>
  ·
  <a href="https://github.com/OMNHZN/OMNAFK/issues/new?template=bug_report.yml">Report a bug</a>
</p>

<p align="center">
  <a href="https://github.com/OMNHZN/OMNAFK/releases">
    <img alt="GitHub release" src="https://img.shields.io/github/v/release/OMNHZN/OMNAFK?include_prereleases&label=release&style=flat-square&color=111111">
  </a>
  <a href="https://github.com/OMNHZN/OMNAFK/actions/workflows/release.yml">
    <img alt="Release workflow" src="https://img.shields.io/github/actions/workflow/status/OMNHZN/OMNAFK/release.yml?label=build&style=flat-square&color=111111">
  </a>
  <img alt="Windows 10 and 11" src="https://img.shields.io/badge/windows-10%20%7C%2011-111111?style=flat-square">
  <a href="LICENSE">
    <img alt="MIT license" src="https://img.shields.io/badge/license-MIT-111111?style=flat-square">
  </a>
</p>

<table>
  <tr>
    <td width="50%">
      <img src="assets/github/flyout.png" alt="OMNAFK tray flyout">
      <br>
      <sub>Tray flyout</sub>
    </td>
    <td width="50%">
      <img src="assets/github/setup.png" alt="OMNAFK custom setup">
      <br>
      <sub>Custom setup</sub>
    </td>
  </tr>
</table>

## Why OMNAFK

OMNAFK watches your visible windows, decides which ones look like games, and keeps them awake without asking you to babysit another control panel. There is no arm button and no start button. It runs from the tray, wakes when a game does, and goes dormant when there is nothing to do.

## What You Get

| Feature | Behavior |
| --- | --- |
| Automatic detection | Finds fullscreen, borderless, and game-platform windows without manual arming. |
| Quiet keepalives | Uses `PostMessage` by default so it avoids stealing focus. |
| Play-aware timing | Skips ticks while you are actively playing. |
| Persistent overrides | Marks any detected target as `GAME` or `IGNORED` and remembers it. |
| Save-on-change settings | Writes settings immediately to `%APPDATA%\OMNAFK\config.json`. |
| GitHub updates | Checks GitHub Releases and opens the bug-report form from Settings. |

## Install

Download `OMNAFK-Setup.exe` from the [latest release](https://github.com/OMNHZN/OMNAFK/releases/latest), run it, and leave **Start with Windows** enabled if you want OMNAFK to live quietly in the tray.

## Updates And Feedback

The app is wired to [OMNHZN/OMNAFK](https://github.com/OMNHZN/OMNAFK). The Settings tab can check GitHub Releases, open the latest download, open the repository, and start a structured bug report.

Publishing a new version tag, such as `v0.1.1`, builds and attaches the custom setup executable to GitHub Releases.

<details>
<summary>Build from source</summary>

```powershell
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
cargo tauri build
cd ..
.\scripts\build-custom-installer.ps1
```

Local installer builds are written to `dist\OMNAFK-Setup.exe`.

</details>

<details>
<summary>Release checklist</summary>

```powershell
git tag v0.1.1
git push origin main
git push origin v0.1.1
```

GitHub Actions builds the Windows installer and publishes it to GitHub Releases. See [docs/RELEASING.md](docs/RELEASING.md) for the full release checklist.

</details>

## Safety Note

Sending automated input may violate the terms of service of some games or platforms. Use OMNAFK at your own discretion.

## License

MIT. See [LICENSE](LICENSE).
