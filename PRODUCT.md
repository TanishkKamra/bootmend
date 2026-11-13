# Product

<!-- impeccable:product-schema 1 -->

## Platform

windows

Native Windows 10 and 11 desktop app (Rust, eframe/egui 0.31), one exe with no installer. Not a web page: there is no browser, no hosted font, no CSS.

## Users

Two people, told apart by asking on first run:

- **The main user is not technical.** They are on their own or a family PC, open Bootmend when the PC feels slow or now and then to check it, fix what it suggests, and close it.
- **The helper is the techy family member or friend.** They install it for someone, set up regular check-ups, and want the detail behind each finding.

Bootmend asks the person first, then shows the version they should see: a simple one for the main user and a detailed one for the helper. Either can switch later.

## Product Purpose

Bootmend checks a Windows PC (hardware, health, security, software), then turns the result into a short plan, and does the safe fixes for you. Success means two equal things:

- A non-technical person goes from "is my PC OK?" to fixed without calling anyone or reading anything twice.
- Nobody mistakes it for scareware or a "PC optimizer" upsell when they first see it.

## Positioning

- **Does the work, safely.** It fixes things for you. Every change explains itself first, and every fix can be undone from History.
- **Honest, nothing to sell.** No scare scores, no upsell, nothing sent anywhere without asking.
- **Plain words.** Each finding says what was found, why it matters and what to do.
- **Feels part of Windows.** It looks and behaves like Windows' own apps, so it is trusted on sight.

## Operating Context

- Opened often and briefly, on laptops and desktops, in light or dark mode as Windows is set.
- Regular check-ups run from Task Scheduler without a window. A Windows notification appears only when something new needs a look.
- The GUI always starts as administrator (UAC prompt); scheduled and headless runs stay unelevated, and the app keeps working if UAC is declined.

## Capabilities and Constraints

- **Five places:**
  - Home, a plan to work through.
  - Tune-up.
  - My PC, with tabs for Hardware, Storage, Network, Security, Apps, Crashes and Live.
  - History.
  - Settings.
- **Safe fixes:** clearing temp files and caches only. "Fix the N safe ones for me" runs these without asking.
- **Every other fix asks first,** with a "Don't ask again" checkbox. It never covers deleting things or going online, and Settings can turn asking back on.
- **Guided help:** for anything Bootmend can't do itself, it opens the exact Windows page, shows a short checklist, re-checks automatically, and ticks the item off.
- **Reports** save as HTML or PDF, with personal details hidden by default.
- **Platform limits:**
  - egui draws everything itself, so system fonts are loaded from `%WINDIR%\Fonts`.
  - Mica needs DWM and a transparent window.

## Brand Commitments

- **Name:** Bootmend ("boot" + "mend"). It is "Bootmend" in UI text, and `bootmend` for the exe, CLI and protocol.
- **Voice:**
  - The app never says "I"; it names the outcome.
  - Not flattering and not persuasive: no praise for the user, no pushing, no urgency it can't back up.
  - No "please", "successfully", "simply" or "just".
- **Look:** Windows 11 native, with Mica and Segoe UI Variable.
- **No logo or app icon exists yet.**

## Evidence on Hand

- The app's own check results on the user's PC. The screenshot mode `bootmend --screenshots <dir>` renders every page.
- **There are no testimonials, user counts, reviews or press, and none may be invented.**

## Product Principles

1. **Do it for them, then show what changed.** Plans and buttons, not dashboards to read.
2. **Earn trust by restraint.** It only reads until you press, explains before it acts, can undo everything, and is never alarmist.
3. **Plain words over completeness.** Detail is there for the helper, never in the main user's way.
4. **Native first.** When Windows already has a pattern for something, use it.

## Accessibility & Inclusion

WCAG 2.2 AA contrast in both themes, full keyboard use with a visible focus ring, and colour never carrying meaning alone. Many users are older or less confident with PCs, so targets stay generous and words stay plain.
