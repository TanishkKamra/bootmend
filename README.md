# rigscan

A Windows desktop app that checks a PC (hardware, health, security, software) and explains, in plain words, what is worth fixing. It is written for people who aren't technical.

rigscan only reads until you press a button. Every change explains itself first, and every fix can be undone from History. Nothing is sent anywhere, except the connection test, and that one asks first.

## What it does

- **Overview and Suggestions:** a check of security, stability, speed and hardware, with suggestions sorted by what matters most and the steps to follow.
- **This PC:** device details, an upgrade advisor (for example, a matching memory stick), and, on Windows 10, whether the PC can move to Windows 11.
- **Storage:** drive health, wear and free space.
- **Free up space:** measures caches and leftovers first, then clears only what you tick. Files an app has open are skipped.
- **Network:** adapters, Wi-Fi, what accepts connections from the network, and a connection test (router, DNS, internet delay, download and upload speed) that uses Cloudflare's public speed test.
- **Security:** Windows Security, firewall, updates, System Restore, and which apps used the camera, microphone and location.
- **Apps & extensions:** installed apps, outdated ones (updated through winget), and browser extensions with a link to each one's own page.
- **Startup & background:** on/off switches for apps that open at sign-in, plus services and scheduled tasks from other makers.
- **Crashes:** blue screens and app crashes, with what each stop code usually means.
- **What's running:** live processor, memory and drive use, grouped by app.
- **Changes and History:** what changed between checks, every change rigscan made with an Undo button, and charts of free space, suggestions, startup apps and battery health over time.
- **Regular check-ups (optional):** after the first check, rigscan offers to check again daily, weekly or monthly. A check-up runs quietly at the hour you pick (noon unless you change it), or as soon as the PC is on after that, and shows a notification only when something new needs a look. It keeps up to 100 saved checks.
- **Save a report:** HTML or PDF, with personal details hidden unless you choose to show them.

Some details (Windows Security's skip list, crash files, some drive health data) need administrator rights. The app offers to restart as administrator and works without it.

## Setup

Needs Windows 10 or 11 and Rust (MSVC toolchain).

```
cargo build --release
```

The app is `target\release\rigscan.exe`. It's a single file with no installer.

## Run

```
rigscan                                  Open the app
rigscan --report <folder> [--pdf] [--show-personal]
                                         Run a check without a window and save a report
rigscan --screenshots <folder> [--theme light|dark] [--size WxH]
                                         Save a picture of every page (for testing)
```

`--scheduled` is what the regular check-up task runs. You don't need to use it yourself.

Saved checks live in `%LOCALAPPDATA%\rigscan\snapshots\<PC name>\`.

## Test

```
cargo test
cargo clippy --all-targets
```

Tests that change the PC (cleaning, Task Scheduler, notifications, the live connection test) are marked `#[ignore]`. Run them on purpose with `cargo test -- --ignored`.

## Design

See [DESIGN.md](DESIGN.md) for the palette, type, spacing, components and the reasoning behind them.
