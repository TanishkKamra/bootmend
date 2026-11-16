# Bootmend

A Windows desktop app that checks a PC (hardware, health, security, software) and explains, in plain words, what is worth fixing. It is written for people who aren't technical.

Bootmend only reads until you press a button, and every change explains itself first. Changes to Windows settings can be undone from History. Clearing space and updating apps can't be undone, and Bootmend says so before it runs them. It only goes online when you press one of four buttons (check for app updates, update an app, read crash files, test my connection), and each one asks first.

## What it does

- **Overview and Suggestions:** a check of security, stability, speed and hardware, with suggestions sorted by what matters most and the steps to follow. [docs/levels.md](docs/levels.md) explains how each one is ranked.
- **This PC:** device details, an upgrade advisor (for example, a matching memory stick), and, on Windows 10, whether the PC can move to Windows 11.
- **Storage:** drive health, wear and free space. When a failing drive holds Documents, Desktop or Pictures, Bootmend offers to copy them to a USB drive.
- **Free up space:** measures caches and leftovers first, then clears only what you tick. Files an app has open are skipped.
- **Network:** adapters, Wi-Fi, what accepts connections from the network, and a connection test (router, DNS, internet delay, download and upload speed) that uses Cloudflare's public speed test.
- **Security:** Windows Security, firewall, updates, System Restore, and which apps used the camera, microphone and location.
- **Apps & extensions:** installed apps, outdated ones (updated through winget), and browser extensions with a link to each one's own page.
- **Startup & background:** on/off switches for apps that open at sign-in, plus services and scheduled tasks from other makers.
- **Crashes:** blue screens and app crashes, with what each stop code usually means. It can repair damaged Windows files (DISM, then System File Checker) and, when blue screens began after a driver update, go back to the driver before it.
- **What's running:** live processor, memory and drive use, grouped by app.
- **Changes and History:** what changed between checks, every change Bootmend made (with an Undo button where Windows allows one), the restore points Windows has saved, and charts of free space, suggestions, startup apps and battery health over time.
- **Regular check-ups (optional):** after the first check, Bootmend offers to check again daily, weekly or monthly. A check-up runs quietly at the hour you pick (noon unless you change it), or as soon as the PC is on after that, and shows a notification only when something new needs a look. It keeps up to 100 saved checks.
- **Save a report:** HTML or PDF, with personal details hidden unless you choose to show them.
- **Looks like Windows 11:** follows your light or dark mode and transparency effects, and can take your Windows accent colour (Settings > Appearance).
- **Screen readers:** headings, buttons, switches and progress bars are named for Narrator and other screen readers.

Some details (Windows Security's skip list, crash files, some drive health data) need administrator rights. The app offers to restart as administrator and works without it.

## Setup

Needs Windows 10 or 11 and Rust (MSVC toolchain).

```
cargo build --release
```

The app is `target\release\bootmend.exe`. It's a single file with no installer.

## Run

```
bootmend                                 Open the app
bootmend --report <folder> [--pdf] [--show-personal]
                                         Run a check without a window and save a report
bootmend --screenshots <folder> [--theme light|dark] [--size WxH]
                                         Save a picture of every page (for testing)
```

`--scheduled` is what the regular check-up task runs. You don't need to use it yourself.

Saved checks live in `%LOCALAPPDATA%\Bootmend\snapshots\<PC name>\`.

Everything the app does is written to a log, one file per day, in a `logs` folder beside `bootmend.exe` (or `%LOCALAPPDATA%\Bootmend\logs` when that folder can't be written to). It records:

- each check step and how long it took
- every program the app runs, with its exit code
- fixes, undos, scans and schedule changes
- page visits, settings changes and every message shown
- crashes, with where they happened

The Windows username, home folder and PC name are replaced with placeholders. App, file and extension names stay in, so look through a log before sharing it. Logs older than 14 days are deleted, and a day's file stops growing at 5 MB. Settings has an "Open the log" link.

Bootmend used to be called rigscan. The first time it runs, it moves the old `%LOCALAPPDATA%\rigscan` folder (saved checks, settings and undo history) to the new name, and moves a regular check-up set up under the old name over too.

## Test

```
cargo test
cargo clippy --all-targets
```

Tests that change the PC (cleaning, Task Scheduler, notifications, the live connection test) are marked `#[ignore]`. Run them on purpose with `cargo test -- --ignored`.

## Design

See [DESIGN.md](DESIGN.md) for the palette, type, spacing, components and the reasoning behind them.
