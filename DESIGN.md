# Bootmend design

Bootmend is a **Use** surface: a native Windows utility that people open to answer one question ("is my PC OK, and what should I do?"), then close. Speed, consistency and plain words matter more than personality.

## Principles

1. **Trust before anything.** It reads until you press a button. Every change explains itself in a confirm dialog. Changes to Windows settings can be undone from History; clearing space and app updates can't, and the dialog says so. No alarm colours for things that aren't urgent, no fake scores, and nothing to buy.
2. **Native, not themed.** Segoe UI, Segoe Fluent Icons, and switches and spacing that match Windows Settings. It follows the Windows light or dark setting.
3. **Plain words.** Each suggestion says what was found, why it matters and what to do, in that order. No jargon without an explanation next to it.
4. **One loud thing per view.** One filled (accent) button per card at most. Long lists use switches or quiet secondary buttons.
5. **Colour never works alone.** Every status pairs a colour with an icon or a word ("Needs attention", "On", "Off").

## Tokens (`src/gui/theme.rs`)

### Colour

| Token | Light | Dark | Job |
|---|---|---|---|
| bg | `#F6F7F9` | `#16181C` | Window canvas |
| surface | `#FFFFFF` | `#1F2227` | Cards |
| surface2 | `#EEF0F3` | `#2A2E35` | Inactive controls, sidebar (light) |
| sidebar | `#EEF0F3` | `#1A1D21` | Sidebar |
| ink | `#1B1F24` | `#E8EAED` | Text |
| muted | `#4A5361` | `#A9B0BB` | Secondary text |
| line | `#DADFE6` | `#2E333A` | Card borders and dividers (decorative) |
| control | `#8A93A0` | `#7D8590` | Control borders, switch edge (3:1) |
| accent | `#1F56C4` | `#8AB0FF` | The one "press here" colour, links, switch on |
| on_accent | `#FFFFFF` | `#0B1220` | Text on accent |
| accent_soft | `#DCE6F8` | `#26324A` | Selected and hover fills |
| red / red_bg | `#B42318` / `#FDECEA` | `#FF9C94` / `#3A1D1B` | "Do first" and failures only |
| amber / amber_bg | `#8A4B00` / `#FFF4E0` | `#FFC46B` / `#3A2C14` | "Worth doing", trade-offs |
| blue / blue_bg | `#2B5797` / `#E8F0FB` | `#9EC2FF` / `#1C2A40` | "Nice to have", info notes |
| green / green_bg | `#1E7B3C` / `#E6F4EA` | `#7BD88F` / `#17301F` | "Looks good", done |

Contrast (checked 2026-11-12 with `design-playbook/scripts/contrast.mjs`). All text pairs pass 4.5:1 and all control shapes pass 3:1.

- **Text, light theme:**
  - ink on surface: 16.56:1
  - muted on bg, surface and surface2: 7.25, 7.77 and 6.81:1
  - on_accent on accent: 6.59:1
  - the lowest pair is green on green_bg, at 4.68:1
- **Text, dark theme:**
  - ink on surface: 13.24:1
  - muted on bg, surface and surface2: 8.14, 7.30 and 6.24:1
  - on_accent on accent: 8.68:1
  - every state colour on its tint is 5.94:1 or higher
- **Control shapes:**
  - control on surface: 3.11:1 in light and 4.28:1 in dark
  - accent on bg: 6.15:1 in light and 8.24:1 in dark

### Type

Segoe UI (regular and semibold), Consolas for raw values, and Segoe Fluent Icons. All are loaded from `%WINDIR%\Fonts`, with egui's own fonts as the fallback.

| Style | Size | Use |
|---|---|---|
| Small | 12.5 | Captions, timestamps, file paths |
| Body / Button | 14.5 | Everything else |
| title | 16 semibold | Card titles (one suggestion, one upgrade) |
| h2 | 18 semibold | Section headings |
| Heading | 26 semibold | Page title |
| big | 30 semibold | The one number on a card (space to free, stability score) |

Body is 14.5 px, not the 16 px a website would use. That matches Windows' own 14 px body text in Settings and Fluent apps, at the same viewing distance.

### Spacing, radius, size

- **Spacing scale:** S1 4, S2 8, S3 12, S4 16, S5 24, S6 32. Space inside a group is smaller than the space between groups.
- **Radius:**
  - cards and notes 8
  - buttons and inputs 6
  - pills 10
  - dialogs 10
  - checkbox 3
  - switch fully round
- **Controls:**
  - buttons are 32 px tall
  - the default interact height is 30 px
  - the switch is 40×20 px
  - the checkbox is 18 px

## Components

| Component | States | When to use |
|---|---|---|
| Primary button (`widgets::primary`) | rest, hover, pressed, disabled with hover reason | The one main action of a card: a fix, "Free up space", "Every week" |
| Secondary button (`secondary`, `icon_button`) | rest, hover, pressed, disabled with hover reason | Everything else, including a per-row action in long lists |
| Switch (`widgets::switch`) | off, on, hover (knob grows), pressed, focus ring, disabled off, disabled on | Something that is simply on or off, such as startup apps. The "On"/"Off" word sits beside it |
| Checkbox (cleanup rows) | unticked, ticked, disabled with reason | Picking several items before one action |
| Pill (`pill`, `level_pill`) | static | Priority ("Do first", "Worth doing") and short states |
| Note (`widgets::note`) | info, warning | A tinted strip with an icon and words; never colour alone |
| Card (`widgets::card`) | static | One group of related content. Cards are never nested |
| Table (`table`, `table_action`) | rows, optional row button | Lists of like items with real columns |
| Confirm dialog (`Pending` + `ask`) | open, busy | Before anything that changes the PC or goes online. At most two choices |
| Toast | success, failure | The result of a background job |
| Empty state | icon, title, one line on what goes here | History with no changes yet. Smaller lists use one muted line ("No blue-screen crashes in the last 90 days.") |

Busy work shows a spinner and words ("Working on it", "Saving a restore point") in place of the button that started it.

## Motion

None beyond egui's defaults: hover fills, the spinner and scrolling. A check-up tool is opened often and briefly, so nothing animates on page changes (the frequency gate).

## Decisions

- **Switches for startup apps (2026-11-12).** These used to be a column of filled "Start with Windows" buttons, which shouted and gave no clear state. They are now switches like Windows Settings > Apps > Startup. A switch still opens a confirm dialog, because the first change may also save a restore point.
- **The administrator note shows only where it matters:** Overview, Security, Crashes and Storage. Those are the pages that lose detail without administrator rights.
- **Timestamps read "today at 20:15"** rather than an ISO date.
- **Status cards on Overview stay.** Four equal cards could look like a stock "metric row", but each one is a real area with real counts, and the row answers the app's main question at a glance.
- **No brand colour beyond one accent.** Blue reads as "system tool" on Windows and keeps red, amber and green free for meaning.

## Verifying

`bootmend --screenshots <folder> --theme light|dark --size WxH` saves every page in parts. Check 880×640 (the smallest useful window) and 1440×900 in both themes after any visual change, and rerun the contrast script after any colour change.
