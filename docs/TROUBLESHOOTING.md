# Troubleshooting

Use this page when OMNAFK is installed but something does not behave the way you expected.

## Game Not Detected

Open the game window, then open OMNAFK and go to **Sightline**.

1. Change the filter to show visible windows.
2. Press **Rescan**.
3. If the game appears, mark it as a game.
4. If a normal app appears, ignore it.

OMNAFK remembers both game and ignored choices.

If the game still does not appear, report it with the **Game detection** issue template and include the game name, executable name, window title, and whether it is fullscreen, borderless, or windowed.

## Keepalive Not Working

First check whether the game is running as administrator. Windows can block normal input from a non-admin app into an admin game.

If OMNAFK offers to restart as administrator, approve the prompt. You can turn this behavior off in Settings if you prefer to handle it manually.

If the target is marked but keepalives still fail:

1. Try a different **What to send** action in **Keepalive**.
2. Keep **Softer retry** enabled.
3. Leave **Don't interrupt me** enabled so OMNAFK moves the next send forward while you are actively playing.
4. Check the target details in Sightline for warnings.

For games where movement keys cancel an emote or move the character, open the target details in Sightline and turn on **Gentle only**.

## Start With Windows

Open **Settings**, turn **Start with Windows** off, then turn it back on. This rewrites the Windows startup entry.

If it still does not launch after sign-in, reinstall the latest setup build and leave **Start with Windows** enabled during setup.

## Tray Icon Or Menu Missing

Windows can briefly delay tray icons during sign-in.

1. Wait a few seconds after logging in.
2. Open the hidden tray icons area.
3. Launch OMNAFK manually once if needed.

If the app is running but the tray icon never appears, report it with the **Bug report** template.

## Updates Or Setup Fail

Download the latest `OMNAFK-Setup.exe` from GitHub Releases and run it again. Setup keeps the existing install folder and user settings.

If the in-app updater reports a checksum or download problem, use the browser download from the release page instead.

## Useful Report Details

When opening an issue, include:

- OMNAFK version
- Windows version
- Game or app name
- Executable name if visible
- What tab or setting you were using
- Screenshot when it helps explain the problem
