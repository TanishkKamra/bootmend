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
- Work and school PCs (joined to a domain or Microsoft Entra) skip settings their IT department manages: Smart App Control, LSA protection and the administrator account.

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
| System drive almost full | Under 5% of the drive free, at least 5 GB and at most 10 GB (6.4 GB on a 128 GB drive, 10 GB from 200 GB up). A monthly update alone needs 2-3 GB |
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
| Restart waiting | Updates have waited 7 days or more for a restart (*judgement*), or since a date Windows didn't record. Dated from Windows' setup log or update history, whichever is earlier |
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
| System drive filling up | Under 10% of the drive free, at least 15 GB and at most 25 GB (*judgement*): room for a yearly update (6-11 GB) plus a monthly one |
| No backup found | No OneDrive folder backup, File History or common backup app (best guess: copies made by hand can't be seen). A failing drive has its own Do first to-do |
| OneDrive isn't backing up | Desktop, Documents or Pictures live in OneDrive, but OneDrive is signed out or not running |
| Secure Boot certificates | Secure Boot is on but the 2023 certificates aren't in place (best guess: the status values are community-documented). Not on Windows 10 without updates, which has its own to-do |
| Local Security Authority protection off | Windows 11 22H2 and later, where Windows Security warns about it. Not on Windows 10, whose only setting locks into the firmware |
| Windows 10 with Extended Security Updates | Signed up, until 12 October 2027 |

## Nice to have

| Finding | When |
|---|---|
| Windows Security's skip list hidden, leftover firewall openings, Remote Assistance on, extensions that read every site | Whenever found |
| Memory integrity off where the hardware can't run it, or switched on but not running | As Windows reports it |
| Smart App Control off (Windows 11), ransomware protection (Controlled folder access) off while Windows Security is the antivirus | Whenever found. Both can block apps you trust, so they stay optional |
| Daily account is an administrator | Only when another administrator account exists and User Account Control is on. With one administrator, demoting it would leave nobody able to fix the PC |
| Updates paused | 35 days or less |
| Restart waiting | Under 7 days |
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
- Space Windows updates need: [Microsoft Support, free up space for Windows updates](https://support.microsoft.com/en-us/windows/deployment/updates-lifecycle/free-up-space-for-windows-updates).
- Secure Boot certificates expiring in 2026: [Microsoft Support, Secure Boot certificate expiration](https://support.microsoft.com/en-us/topic/windows-secure-boot-certificate-expiration-and-ca-updates-7ff40d33-95dc-4c3c-8725-a9b95457578e). The `UEFICA2023Status` values are community-documented ([4sysops](https://4sysops.com/archives/update-windows-secure-boot-db-certificates-with-group-policy-and-powershell/)).
- LSA protection and its start-up event: [Microsoft Learn, configuring added LSA protection](https://learn.microsoft.com/en-us/windows-server/security/credentials-protection-and-management/configuring-additional-lsa-protection).
- Smart App Control: [Microsoft Support FAQ](https://support.microsoft.com/en-us/topic/what-is-smart-app-control-285ea03d-fa88-4d56-882e-6698afdb7003). Controlled folder access: [Microsoft Learn](https://learn.microsoft.com/en-us/defender-endpoint/controlled-folder-access-overview).
