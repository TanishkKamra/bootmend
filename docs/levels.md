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
- Work and school PCs (joined to a domain or Microsoft Entra) skip settings their IT department manages: Smart App Control, LSA protection, the administrator account, browser policies, proxies and DNS servers.

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
| Drive failing | The drive predicts failure, Windows marks it unhealthy, or it reports pending, uncorrectable or media errors. A Crucial MX500 reporting exactly one pending sector and no uncorrectable errors is skipped, as these drives flip that count between 0 and 1 by themselves |
| Blue screens | 3 or more in 90 days (1 or 2 are Worth doing) |
| System drive almost full | Under 5% of the drive free, at least 5 GB and at most 10 GB (6.4 GB on a 128 GB drive, 10 GB from 200 GB up). A monthly update alone needs 2-3 GB |
| Windows 10 without security fixes | Not signed up for Extended Security Updates, or after they end on 12 October 2027 |
| Windows 11 version without security fixes | Home or Pro on a version past Microsoft's end date (24H2 ended on 13 October 2026). Not Enterprise, Education or preview builds, which run on other dates |
| No Windows update | None installed for 90 days or more (*judgement*) |
| Memory test found errors | Windows Memory Diagnostic's latest result in 90 days reports errors (event 1102 or 1202). No result means no test was run, so nothing is shown |
| Processor needs a BIOS update | An Intel Core i5, i7 or i9 13th or 14th Gen desktop processor built on Raptor Lake's own chip (CPUID B0671, model 183), on microcode below 0x12B. Some i5s sold under these names are older Alder Lake chips, which aren't affected and are skipped. K, KF and KS models are measured; the others are a best guess, as Intel hasn't listed exactly which 65 W chips are affected |
| Faulty SSD firmware | Samsung 990 PRO on 0B2QJXD7 (wears out fast), or 980 PRO on 3B2QGXA7 (health drops until it turns read-only) |
| Temporary profile | Windows signed in with a temporary profile, so files saved now are lost at sign-out |

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
| Devices not working | Device Manager reports a problem. Devices that are switched off, waiting for a restart or turned off in the BIOS (codes 14, 21, 22, 29, 44, 45 and 47) aren't faults and are skipped |
| Motherboard parts with no driver | A chipset part runs with no driver at all (code 28): an ACPI device, or a motherboard controller such as a PCI bridge, storage, USB, SMBus or management controller |
| Devices with no driver | Any other device with no driver at all (code 28) |
| Windows Update blocked by a setting | A policy sends updates to another server (WSUS), so fixes may stop arriving, or hides Windows Update's buttons, so updates can't be checked for or installed by hand (background updates carry on). Work and school PCs are skipped |
| Windows' own files damaged | Windows' image health check says repairable or not repairable. Only checked when Bootmend runs as administrator |
| Newer BIOS fix for the processor | An affected Intel 13th or 14th Gen processor on 0x12B or later but below 0x12F, Intel's latest advice |
| Ran low on memory | Windows logged a low-memory warning (event 2004) 2 or more times in 30 days (*judgement*: once can be a one-off) |
| App freezes | One app stopped responding 5 or more times in 30 days (*judgement*, matching app crashes) |
| Windows Recovery off | Windows' recovery tools are switched off (best guess: read from Windows' own status file, which Microsoft doesn't document) |
| WD or SanDisk SSD firmware | A 2 TB SN770, SN580 or SN5000 on firmware older than the fix, on Windows 11 24H2 or later, where it can cause blue screens (best guess: the affected list comes from retailers, not Western Digital) |
| Battery | Holds under 50% of its original charge |
| Switched off without shutting down, hardware errors, overclocking | Power loss events; 10 or more hardware errors in 90 days, or blue screens that point at hardware; an overclock alongside errors |
| System drive filling up | Under 10% of the drive free, at least 15 GB and at most 25 GB (*judgement*): room for a yearly update (6-11 GB) plus a monthly one |
| No backup found | No OneDrive folder backup, File History or common backup app (best guess: copies made by hand can't be seen). A failing drive has its own Do first to-do |
| OneDrive isn't backing up | Desktop, Documents or Pictures live in OneDrive, but OneDrive is signed out or not running |
| Secure Boot certificates | Secure Boot is on but the 2023 certificates aren't in place (best guess: the status values are community-documented). Not on Windows 10 without updates, which has its own to-do |
| Local Security Authority protection off | Windows 11 22H2 and later, where Windows Security warns about it. Not on Windows 10, whose only setting locks into the firmware |
| Windows 10 with Extended Security Updates | Signed up, until 12 October 2027 |
| Windows 11 version ending soon | 90 days or less before its last security fixes (*judgement*: time to sort out an upgrade that won't install) |
| Windows not activated | Unlicensed, not genuine, or in reminder-only mode. Grace periods are Nice to have |
| Drive errors in Windows' log | On a drive inside the PC, over 90 days: any bad block, failure warning or file system damage; 3 or more retries, resets, controller errors or lost writes (*judgement*: one is often a hiccup). USB drives and memory sticks are skipped, and a failing drive has its own Do first to-do |
| Graphics driver stops responding | On 3 or more different days in 30 (*judgement*). Counted by day because one freeze often brings several resets within minutes. PCs with only a virtual screen are skipped |
| Windows still set up for testing | Test signing, driver signature checks off, kernel debugging, Data Execution Prevention off, a memory or processor limit below what the PC has, safe mode, or Driver Verifier |
| No page file | With under 16 GB of memory, or when Windows warned it couldn't save crash files |
| Crash files switched off | After a crash in the last 90 days. Tuning tools sometimes switch them off |
| No page file on the system drive | Page files only on other drives and no dedicated crash file, after a crash or a start-up warning. Windows can't save a crash file without one |
| Crash files that couldn't be saved | A failed write in 90 days with no saved file within 5 minutes; a failure next to a saved file is one crash, partly saved (best guess: usually the drive stopped responding or the power dropped) |
| Browser settings forced by a policy | A policy sets the home page, start-up pages, new tab page, search engine, a proxy or force-installed extensions (best guess: some security apps do this on purpose) |
| Proxy | Web traffic goes through another computer: a proxy server, setup script or Windows' own WinHTTP proxy. One running on this PC is Nice to have |
| DNS server we don't recognise | Set on the PC by hand, public, and not a well-known service such as Cloudflare, Google, Quad9, OpenDNS, AdGuard, NextDNS, Control D or CleanBrowsing (best guess) |
| Weak or open Wi-Fi | The connected network uses WEP or TKIP, or has no password |
| Core Windows services switched off | Windows Update, BITS, Update Orchestrator, Microsoft Defender (unless another antivirus runs), the firewall, Security Center, Event Log, Cryptographic Services, WMI or Windows Time set to Disabled |
| Programs starting from a temporary or hidden place | A start-up entry or scheduled task in Temp, Downloads, Public, the Recycle Bin or loose in ProgramData, or run through encoded PowerShell, mshta, or rundll32 with a web address (best guess) |
| Cloud protection off, automatic sample submission off | While Windows Security is the antivirus. Windows Security shows a warning for both |
| Encryption paused | BitLocker or device encryption is suspended on a drive that is still encrypted, and no restart is waiting |
| Vulnerable driver blocklist off | Windows 11 without memory integrity running. With memory integrity, which enforces the list anyway, Nice to have |
| Two antivirus apps running | Two or more besides Microsoft Defender, both switched on |
| Guest or built-in Administrator account on | Enabled, unless it is the account in daily use |

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
| Preview versions of Windows | The PC is in the Windows Insider Program (best guess: it may be on purpose) |
| Windows activation grace period | Windows still has time to confirm its licence |
| Windows Security hasn't scanned | Over 30 days since its last quick or full scan (*judgement*: real-time protection is the main defence) |
| No page file | With 16 GB of memory or more and no warning about crash files |
| Crash files switched off | No crash in 90 days |
| No page file on the system drive | No crash and no start-up warning |
| Network cable at 100 Mbps or less | A wired port that can do more is linked at 10 or 100 Mbps (*judgement*: usually the cable, plug or router port) |
| TRIM off | On an SSD that holds Windows |

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
- Windows 11 end dates: [Microsoft Learn lifecycle, Windows 11 Home and Pro](https://learn.microsoft.com/en-us/lifecycle/products/windows-11-home-and-pro) (checked 14 November 2026). The end date shown there is the first day without fixes.
- Graphics driver timeouts (event 4101, stop codes 0x117 and 0x141): [Microsoft Learn, timeout detection and recovery](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/timeout-detection-and-recovery).
- Drive errors in the System log: [Microsoft Learn, data corruption and disk errors](https://learn.microsoft.com/en-us/troubleshoot/windows-server/backup-and-storage/troubleshoot-data-corruption-and-disk-errors).
- Crucial MX500 pending count (SMART 197): smartmontools' drive database says the MX500 "may flip 0 <> 1" ([ticket #1227](https://www.smartmontools.org/ticket/1227)). It first flagged only older firmware, then in October 2020 [stopped warning on it for every firmware](https://github.com/smartmontools/smartmontools/commit/49e58c280cc087faa583a99f70c174ae421e2e47). No release note on [Crucial's MX500 firmware page](https://www.crucial.com/support/ssd-support/mx500-support) mentions a fix (checked 15 November 2026). Still reported in 2026 ([Scrutiny issue #1124](https://github.com/AnalogJ/scrutiny/issues/1124)). The flips are only reported on the MX500, so other Crucial drives keep the usual rule.
- Page file and crash files: [Microsoft Learn, introduction to the page file](https://learn.microsoft.com/en-us/troubleshoot/windows-client/performance/introduction-to-the-page-file) and [memory dump file options](https://learn.microsoft.com/en-us/troubleshoot/windows-server/performance/memory-dump-file-options) (CrashDumpEnabled 0 is none, 7 is Windows' default; a small dump needs a page file on the boot volume; checked 14 November 2026).
- Crash files that fail to save (volmgr event 161): no Microsoft page documents the event itself. Causes from [Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/5893224/how-to-fix-dump-file-creation-failed-due-to-error) and [Microsoft's storage team on crash dump failures](https://learn.microsoft.com/en-us/archive/blogs/storwdk/understanding-the-crashdump-test-and-debugging-failures) (checked 14 November 2026).
- Vulnerable driver blocklist: [Microsoft Learn, recommended driver block rules](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/app-control-for-business/design/microsoft-recommended-driver-block-rules).
- Sample submission warning: community-documented ([TheWindowsClub](https://www.thewindowsclub.com/automatic-sample-submission-is-off-in-microsoft-defender)); Microsoft publishes no list of Windows Security's warnings. Which antivirus apps are switched on comes from Security Center's `productState`, also community-documented.
- Device Manager problem codes: [Microsoft Learn, Device Manager error messages](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/device-manager-error-messages). Driver dates aren't used to call a driver old, because Windows' own drivers are all dated 21 June 2006 on purpose ([BleepingComputer, Microsoft engineers explain](https://www.bleepingcomputer.com/news/microsoft/microsoft-employees-explain-why-all-windows-drivers-are-dated-june-21-2006/)).
- Intel 13th and 14th Gen instability: [Intel support article](https://www.intel.com/content/www/us/en/support/articles/000102331/processors.html) and [Intel's 0x12F update](https://community.intel.com/t5/Mobile-and-Desktop-Processors/Intel-Core-13th-and-14th-Gen-Vmin-Shift-Instabilty-Update-New/m-p/1686948). Where Windows keeps the microcode revision (the upper 4 bytes of `Update Revision`) is community-documented ([Mozilla bug 1796282](https://bugzilla.mozilla.org/show_bug.cgi?id=1796282)). That the fix is only for Raptor Lake's chip (CPUID B0671), not the Alder Lake chips some i5s use, comes from forum reports ([Intel Community](https://community.intel.com/t5/Mobile-and-Desktop-Processors/Is-the-i5-13400F-C0-SRMBN-affected-by-the-recent-Vmin-Stability/m-p/1636385), [Level1Techs](https://forum.level1techs.com/t/has-anyone-found-instability-with-the-rebadged-alder-lake-13th-14th-gen-cpus/214723)).
- Windows Memory Diagnostic results: no Microsoft page lists the event IDs; 1101 and 1201 pass, 1102 and 1202 found errors, as logged on real PCs ([NinjaOne](https://www.ninjaone.com/blog/read-memory-diagnostics-tool-results/), [Ten Forums](https://www.tenforums.com/tutorials/91178-read-memory-diagnostics-tool-results-event-viewer-windows-10-a.html)).
- Low memory warnings (event 2004): [Microsoft Learn, application or service memory leaks](https://learn.microsoft.com/en-us/troubleshoot/windows-server/performance/troubleshoot-application-service-memory-leaks).
- Windows image health: [Microsoft Learn, Repair-WindowsImage](https://learn.microsoft.com/en-us/powershell/module/dism/repair-windowsimage?view=windowsserver2025-ps) and [repair a Windows image](https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/repair-a-windows-image?view=windows-11).
- Windows Update policies: [Microsoft Learn, Policy CSP Update](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-update) and [manage additional Windows Update settings](https://learn.microsoft.com/en-us/windows/deployment/update/waas-wu-settings).
- Windows Recovery: [Microsoft Learn, Windows RE technical reference](https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/windows-recovery-environment--windows-re--technical-reference?view=windows-11) and [REAgentC options](https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/reagentc-command-line-options?view=windows-11).
- SSD firmware: [Samsung's firmware page](https://semiconductor.samsung.com/consumer-storage/support/tools/); the 990 PRO and 980 PRO faults are reported by the press, as Samsung's own notices aren't online. WD and SanDisk on 24H2: [DROIX knowledge base](https://droix.net/knowledge-base/article/addressing-windows-11-24h2-bsods-on-wd-sandisk-ssds/).
- Temporary profile: [Microsoft Learn, Win32_UserProfile](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/legacy/ee886409(v=vs.85)) (status bit 1 is Temporary) and [User Profiles Service event 1511](https://learn.microsoft.com/en-us/archive/technet-wiki/3571.windows-user-profiles-service-event-1511-windows-cannot-find-the-local-profile-and-is-logging-you-on-with-a-temporary-profile).
- Smart App Control: [Microsoft Support FAQ](https://support.microsoft.com/en-us/topic/what-is-smart-app-control-285ea03d-fa88-4d56-882e-6698afdb7003). Controlled folder access: [Microsoft Learn](https://learn.microsoft.com/en-us/defender-endpoint/controlled-folder-access-overview).
