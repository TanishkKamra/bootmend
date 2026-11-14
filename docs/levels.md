# How Bootmend ranks what it finds

Every finding gets one of four levels. Home groups the to-dos by level, and the level sets each area's score (Do first costs 30 points, Worth doing 10, Nice to have 3). The rules live in `src/rules/`, and every threshold below has a test there.

| Level | What it means |
|---|---|
| Do first | Files, money or safety are at risk now. |
| Worth doing | A real problem, or a door left open. |
| Nice to have | An improvement only. Nothing is wrong. |
| All good | Checked and fine, or already dealt with. |

## Rules

- Nice to have never labels a real problem. If ignoring it can cost data, money or safety, it is at least Worth doing.
- Where Windows Security shows the same thing, Bootmend never ranks it lower: a red warning there is Do first, a yellow one at least Worth doing.
- A security setting that costs nothing to turn on (CIS Level 1) is Worth doing. One that can get in the way (CIS Level 2) is Nice to have.
- A finding Bootmend can't confirm is marked "Our best guess" rather than ranked lower.
- Thresholds marked *judgement* have no official number behind them. They are our call, written down so they stay consistent.

## Do first

| Finding | When |
|---|---|
| Virus protection off | Real-time protection is off and no other antivirus is running |
| Windows Security skips a whole drive | An exclusion covers a drive root (folders alone are Worth doing) |
| Threats not removed | Windows Security found something it couldn't remove |
| Firewall off | Off for any network type |
| Apps change the system without asking | User Account Control is off |
| Harmful browser extension | On Bootmend's list of known harmful extensions |
| App with a known security hole | Older than the version that fixed it |
| Drive failing | The drive predicts failure, Windows marks it unhealthy, or it reports pending, uncorrectable or media errors |
| Blue screens | 3 or more in 90 days (1 or 2 are Worth doing) |
| System drive almost full | Under 5% or 5 GB free |
| Windows 10 without security fixes | Not signed up for Extended Security Updates, or after they end on 12 October 2027 |
| No Windows update | None installed for 90 days or more (*judgement*) |

## Worth doing

| Finding | When |
|---|---|
| Virus definitions out of date | 3 days or older (*judgement*); Windows reports 65535 when they have never updated |
| Tamper protection off, adware not blocked | As Windows Security reports them |
| Remote Desktop, SMB1, download warnings off, sign-in without a password, redirected websites, extensions from outside the store, unsigned drivers, unusual apps using the camera or microphone | Whenever found |
| Chrome Remote Desktop, other remote control apps | Whenever found. Scammers ask people to install these (CISA AA23-025A) |
| Local account allowed an empty password | Whenever found (best guess: Windows can't say whether the password really is empty) |
| Secure Boot off | On a PC that boots with UEFI |
| Laptop drive not encrypted | A laptop whose system drive has BitLocker or device encryption off |
| Memory integrity off | Only where the hardware can run it (virtualization available); otherwise Nice to have |
| Updates failing | The same update failed twice or more in 60 days |
| Automatic updates off | Turned off by a policy |
| Updates paused | For longer than the 35 days Windows Settings allows |
| No Windows update | 45 to 89 days (*judgement*) |
| System Restore off | On the system drive |
| Restart waiting | Updates have waited 7 days or more for a restart (*judgement*) |
| App crashes | One app crashed 5 or more times in 30 days, or 3 or more different apps crashed (*judgement*). Several apps become one to-do, since they often share a cause |
| Graphics driver | Over 12 months old. Virtual screens (Parsec, remote desktop, virtual machines) are skipped |
| No proper graphics driver | Windows' basic display driver is in use |
| App updates waiting | Updates found through winget |
| Memory | Low total memory, one channel only, or nearly full during the check |
| Windows on a hard disk | The system drive is not an SSD |
| Drive wear and health | 90% or more of rated life used, replaced sectors, low spare space, or running hot |
| Devices not working | Device Manager reports a problem |
| Battery | Holds under 50% of its original charge |
| Switched off without shutting down, hardware errors, overclocking | Power loss events; 10 or more hardware errors in 90 days, or blue screens that point at hardware; an overclock alongside errors |
| System drive filling up | Under 10% or 15 GB free |
| Windows 10 with Extended Security Updates | Signed up, until 12 October 2027 |

## Nice to have

| Finding | When |
|---|---|
| Windows Security's skip list hidden, leftover firewall openings, Remote Assistance on, extensions that read every site | Whenever found |
| Memory integrity off where the hardware can't run it, or switched on but not running | As Windows reports it |
| Updates paused | 35 days or less |
| Restart waiting | Under 7 days, or since an unknown date |
| Old restore point, memory slower than rated, big folders, many startup apps, slow start-up, Power saver on a desktop | Whenever found |
| PC not restarted | Running for more than 14 days |
| Drive wear | 75% to 89% of rated life used |
| Battery | Holds 50% to 69% of its original charge |
| Hardware errors, overclocking | Under 10 errors and no blue screens that point at hardware; an overclock with no errors |
| Another drive almost full | Under 5% free on a drive other than C: |

## All good

Another antivirus protecting the PC, threats Windows Security already removed, and any area with nothing to fix.

## Sources

- Windows 10 Extended Security Updates and their end date: [microsoft.com/windows/extended-security-updates](https://www.microsoft.com/windows/extended-security-updates) (checked 12 November 2026).
- Windows 10 ESU sign-up state (`ESUEligibility` 3 or 5 means enrolled): community-documented in [abbodi1406/ConsumerESU](https://github.com/abbodi1406/ConsumerESU) and on [TenForums](https://tenforums.com/windows-10-news/220564-windows-10-consumer-extended-security-updates-esu-program-24.html), not by Microsoft. Hence the best-guess label.
- Memory integrity hardware needs: [Microsoft Learn, memory integrity for PC makers](https://learn.microsoft.com/windows-hardware/design/device-experiences/oem-hvci-enablement).
- Remote control apps used in scams: [CISA advisory AA23-025A](https://www.cisa.gov/news-events/cybersecurity-advisories/aa23-025a).
