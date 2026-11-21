# Bootmend

A Windows desktop app that checks a PC (hardware, health, security, software) and explains, in plain words, what is worth fixing. It is written for people who aren't technical.

Bootmend only reads until you press a button, and every change explains itself first. Changes to Windows settings can be undone from History. Clearing space and updating apps can't be undone, and Bootmend says so before it runs them. It goes online on its own for one thing: once a day it asks GitHub whether a newer Bootmend is out, and Settings can turn that off. Anything else online (checking for app updates, updating an app, reading crash files, testing the connection, updating Bootmend) waits for a button press and asks first.

## Download

Bootmend 0.1.0 is an early version, so expect a few rough edges. Download `bootmend.exe` from the [latest release](https://github.com/TanishkKamra/bootmend/releases/latest). It's a single file with no installer, for Windows 10 and 11 (64-bit).

Bootmend isn't code-signed yet, so the first time you open it Windows may say "Windows protected your PC". Click **More info**, then **Run anyway**. To check your copy is the real one, compare its SHA-256 checksum with the one on the release page. In PowerShell, in the folder you saved it to:

```
Get-FileHash .\bootmend.exe -Algorithm SHA256
```

After that, Bootmend offers new versions itself. It installs one only if the download is signed with Bootmend's release key, and your saved checks and settings stay as they are.

## What it does

- **Overview and Suggestions:** a check of security, stability, speed and hardware, with suggestions sorted by what matters most and the steps to follow. [docs/levels.md](docs/levels.md) explains how each one is ranked.
- **This PC:** device details, an upgrade advisor (for example, a matching memory stick), and, on Windows 10, whether the PC can move to Windows 11.
- **Storage:** drive health, wear and free space. When a failing drive holds Documents, Desktop or Pictures, Bootmend offers to copy them to a USB drive.
- **Free up space:** measures caches and leftovers first, then clears only what you tick. Files an app has open are skipped.
- **Network:** adapters, Wi-Fi, what accepts connections from the network, and a connection test that times the router, DNS lookups and Microsoft's website, to show where any delay is.
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

The app is `target\release\bootmend.exe`. It's a single file with no installer, and it doesn't need the Visual C++ Redistributable.

After changing dependencies, run `python tools/third_party_notices.py` to refresh `THIRD-PARTY-NOTICES.txt`, which is built into the app.

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

History is kept in `%LOCALAPPDATA%\Bootmend\history.json`. A change that removes settings saves a copy of them first, under `HKCU\SOFTWARE\Bootmend\Changes` for your own settings and `HKLM\SOFTWARE\Bootmend\Changes` for settings that apply to everyone on the PC. A change made as administrator also leaves a record there, and Undo checks it first, so editing `history.json` can't change what an undo does. Deleting Bootmend's data in Settings removes these too; the `HKLM` ones only go when Bootmend is running as administrator.

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

## Release

Releases are built and signed on GitHub, never on a personal PC.

1. Raise `version` in `Cargo.toml`, run `cargo build` so `Cargo.lock` follows, and merge to `main`.
2. Tag that commit and push the tag, for example `git tag v0.1.1` and `git push origin v0.1.1`.
3. The Release workflow tests and builds `bootmend.exe`. Approve the signing job when GitHub asks; it signs the exe, then opens a draft release with the exe, its signature and its SHA-256.
4. Add the notes and publish the draft. Installed copies offer the update at their next daily check.

The build runs on Windows and never sees the signing key. A separate Linux job signs the exe with Ubuntu's `minisign`, then checks the signature against `PUBLIC_KEY` in `src/update.rs` before drafting the release. The secret key is `BOOTMEND_SIGNING_KEY` in the `release` environment, which only `v*` tags can use, and only once the maintainer approves the run. Running the workflow by hand is a rehearsal: it builds and signs with a throwaway key, and makes no release.

## Design

See [DESIGN.md](DESIGN.md) for the palette, type, spacing, components and the reasoning behind them.

## Contributing

Found a problem? Use **Report a problem** in Bootmend, or the [bug form](https://github.com/TanishkKamra/bootmend/issues/new?template=bug_report.yml). Fixes are welcome too: see [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

Bootmend is free software under the GNU General Public License v3.0 or later. It comes with no warranty: see [LICENSE](LICENSE). Third-party licences are in [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt).

Windows, OneDrive and other product names are trademarks of their owners. Bootmend isn't made or endorsed by Microsoft or any other company it mentions.
