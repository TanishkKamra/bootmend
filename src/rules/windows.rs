//! Windows itself: support status, Windows 11 readiness, Windows Update and System Restore.

use super::{plural, Action, Area, Confidence, Finding, Level};
use crate::fixes::Fix;
use crate::snapshot::Snapshot;
use crate::util::J;
use chrono::NaiveDate;
use regex::Regex;

const SEC: Area = Area::Security;
/// Last day of Extended Security Updates for Windows 10 home users.
/// Source: microsoft.com/windows/extended-security-updates (checked 2026-11-12; Microsoft moved it from 2026 to 2027).
const ESU_END: &str = "2027-10-12";
const ESU_END_TEXT: &str = "October 12, 2027";

pub fn check(s: &Snapshot, f: &mut Vec<Finding>) {
    check_on(s, chrono::Local::now().date_naive(), f);
}

fn check_on(s: &Snapshot, today: NaiveDate, f: &mut Vec<Finding>) {
    windows10(s, today, f);
    windows11_version(s, today, f);
    licence(s, f);
    updates(s, today, f);
    restore(s, today, f);
}

fn date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

/// "13 October 2026".
fn long_date(d: NaiveDate) -> String {
    d.format("%-d %B %Y").to_string()
}

/// Windows 11 Home and Pro versions and the first day each gets no fixes, from Microsoft's lifecycle
/// page (learn.microsoft.com/lifecycle/products/windows-11-home-and-pro, checked 14 November 2026).
const WIN11_ENDS: &[(&str, &str)] = &[
    ("21H2", "2023-10-11"),
    ("22H2", "2024-10-09"),
    ("23H2", "2025-11-12"),
    ("24H2", "2026-10-14"),
    ("25H2", "2027-10-13"),
    ("26H1", "2028-03-15"),
    ("26H2", "2028-10-10"),
];

fn windows11_version(s: &Snapshot, today: NaiveDate, f: &mut Vec<Finding>) {
    let os = s.system.g("os");
    let Some(edition) = os.s("Edition") else { return };
    // Enterprise and Education run on other dates, and preview builds update themselves.
    let home_or_pro = edition.contains("Home") || edition.contains("Pro");
    if !edition.contains("Windows 11") || !home_or_pro || os.s("InsiderChannel").is_some_and(|c| !c.is_empty()) {
        return;
    }
    let Some(version) = os.s("Version") else { return };
    let Some(end) = WIN11_ENDS.iter().find(|(v, _)| *v == version).and_then(|(_, d)| date(d)) else { return };
    let last = long_date(end - chrono::Duration::days(1));
    // 90 days' warning is our call: long enough to sort out an upgrade that won't install.
    let (level, title, why) = if today >= end {
        (
            Level::DoFirst,
            format!("Windows 11 {version} no longer gets security fixes"),
            format!("Microsoft's last fixes for this version came on {last}. Each version of Windows 11 gets fixes for about two years."),
        )
    } else if (end - today).num_days() <= 90 {
        (
            Level::WorthDoing,
            format!("Windows 11 {version} stops getting security fixes soon"),
            format!("Microsoft's last fixes for this version come on {last}. Each version of Windows 11 gets fixes for about two years."),
        )
    } else {
        return;
    };
    f.push(
        Finding::new("win11-version", SEC, level, title)
            .why(why)
            .step("Open Windows Update and install the newest version of Windows 11 it offers. It's free and keeps your apps and files.")
            .step("If no new version shows up, sort out any other update problems in this list first.")
            .detail(format!("{edition}, version {version}"))
            .open("Open Windows Update", "ms-settings:windowsupdate"),
    );
}

fn licence(s: &Snapshot, f: &mut Vec<Finding>) {
    let os = s.system.g("os");
    // Windows' licence states: 0 unlicensed, 4 not genuine, 5 reminders only; 2, 3 and 6 are grace periods.
    let state = os.s("Activation");
    let level = match state.as_deref() {
        Some("Unlicensed" | "Status 4" | "Status 5") => Level::WorthDoing,
        Some("Status 2" | "Status 3" | "Status 6") => Level::NiceToHave,
        _ => Level::AllGood,
    };
    if level != Level::AllGood {
        f.push(
            Finding::new("not-activated", SEC, level, "Windows isn't activated")
                .why(if level == Level::WorthDoing {
                    "Windows can't confirm its licence, so some settings are locked and reminders keep appearing. Copies of Windows from unofficial sources often come with harmful extras."
                } else {
                    "Windows is in its grace period and needs to confirm its licence soon. Until then, reminders appear."
                })
                .step("Open Settings, System, Activation. If you changed hardware recently, the troubleshooter there usually sorts it out.")
                .detail(format!("Licence state: {}", state.unwrap_or_default()))
                .open("Open Activation", "ms-settings:activation"),
        );
    }
    if let Some(channel) = os.s("InsiderChannel").filter(|c| !c.is_empty()) {
        f.push(
            Finding::new("insider", SEC, Level::NiceToHave, "This PC gets preview versions of Windows")
                .why("Windows Insider builds bring new features early, but they're tested less and can have bugs. That's fine if you chose it.")
                .step("To go back to normal versions, open Settings, Windows Update, Windows Insider Program, and switch on Stop getting preview builds.")
                .detail(format!("Insider channel: {channel}"))
                .conf(Confidence::BestGuess)
                .open("Open Windows Insider Program", "ms-settings:windowsinsider"),
        );
    }
}

/// Whether Windows 11 supports this processor: `None` when the name doesn't tell us.
/// A rule of thumb from Microsoft's lists: Intel 8th generation and newer, AMD Ryzen 2000 and newer
/// (except the first Ryzen chips with graphics, such as the 2200G and 2400G).
pub fn cpu_supported(name: &str) -> Option<bool> {
    let n = name.replace("(R)", "").replace("(TM)", "");
    if n.contains("Core Ultra") || Regex::new(r"Core [357] \d{3}").ok()?.is_match(&n) || n.contains("Ryzen AI") {
        return Some(true);
    }
    if let Some(c) = Regex::new(r"Core i[3579]-(\d{4,5})").ok()?.captures(&n) {
        let m = &c[1];
        let generation: u32 = if m.len() == 5 { m[..2].parse().ok()? } else { m[..1].parse().ok()? };
        return Some(generation >= 8);
    }
    if Regex::new(r"Core i[3579] (CPU )?\d{3}\b").ok()?.is_match(&n) {
        return Some(false);
    }
    if let Some(c) = Regex::new(r"Ryzen.*?\b(\d)\d{3}([A-Z0-9]*)").ok()?.captures(&n) {
        let series: u32 = c[1].parse().ok()?;
        let apu = c[2].starts_with('G');
        return Some(series >= 3 || (series == 2 && !apu));
    }
    None
}

/// A readable name for an entry in Windows Update's history, and whether it's a Microsoft Store app update.
/// Store updates look like "9NBLGGH537C2-Microsoft.XboxGameOverlay".
pub fn update_name(title: &str) -> (String, bool) {
    let store = Regex::new(r"^[0-9A-Z]{12}-(?:Microsoft\.)?(.+)$").expect("valid pattern");
    let Some(c) = store.captures(title) else { return (title.to_string(), false) };
    let raw = c[1].rsplit('.').next().unwrap_or(&c[1]);
    let mut name = String::new();
    for (i, ch) in raw.char_indices() {
        if i > 0 && ch.is_uppercase() && !raw[..i].ends_with(char::is_uppercase) {
            name.push(' ');
        }
        name.push(ch);
    }
    (name, true)
}

#[derive(Debug, Default)]
pub struct Readiness {
    pub missing: Vec<String>,
    pub unsure: Vec<String>,
}

/// What stands between this PC and Windows 11, judged from the check's data.
pub fn win11_readiness(s: &Snapshot) -> Readiness {
    let mut r = Readiness::default();
    let dev = s.system.g("device");
    match (dev.b("TpmPresent"), dev.s("TpmSpec")) {
        (Some(true), Some(v)) if !v.trim_start().starts_with('2') => r.missing.push(format!("A TPM 2.0 security chip (this PC has version {v})")),
        (Some(true), _) => {}
        (Some(false), _) => r.missing.push("A TPM 2.0 security chip".into()),
        (None, _) => r.unsure.push("The TPM security chip".into()),
    }
    match dev.s("FirmwareMode").as_deref() {
        Some("UEFI") => {}
        Some(_) => r.missing.push("UEFI start-up mode (this PC starts in the older Legacy BIOS mode)".into()),
        None => r.unsure.push("The start-up mode (UEFI)".into()),
    }
    if s.system.g("memory").f("TotalGB").is_some_and(|g| g < 3.5) {
        r.missing.push("At least 4 GB of memory".into());
    }
    let c = s.storage.a("volumes").into_iter().find(|v| v.s("Letter").as_deref() == Some("C:"));
    if c.and_then(|v| v.f("SizeGB")).is_some_and(|g| g < 64.0) {
        r.missing.push("A drive of at least 64 GB".into());
    }
    match s.system.g("cpu").s("Name").as_deref().map(cpu_supported) {
        Some(Some(true)) => {}
        Some(Some(false)) => r.missing.push("A newer processor (Intel 8th generation or AMD Ryzen 2000 and up)".into()),
        _ => r.unsure.push("The processor".into()),
    }
    r
}

/// Whether this Windows 10 PC is signed up for Extended Security Updates. Windows' own sign-up
/// state: 3 = this device, 5 = through a Microsoft account. The values are community-documented,
/// not by Microsoft, hence the best-guess label.
fn esu_enrolled(s: &Snapshot) -> bool {
    let os = s.system.g("os");
    matches!(os.i("EsuState"), Some(3 | 5)) || os.b("EsuLicensed") == Some(true)
}

/// Windows 10 outside Extended Security Updates: it gets no updates at all, which its own to-do
/// covers, so checks that depend on updates stay quiet.
pub fn without_updates(s: &Snapshot) -> bool {
    s.system.g("os").s("Edition").is_some_and(|e| e.contains("Windows 10")) && !esu_enrolled(s)
}

fn windows10(s: &Snapshot, today: NaiveDate, f: &mut Vec<Finding>) {
    if !s.system.g("os").s("Edition").is_some_and(|e| e.contains("Windows 10")) {
        return;
    }
    let esu_over = date(ESU_END).is_some_and(|end| today > end);
    let enrolled = esu_enrolled(s);
    let r = win11_readiness(s);
    let level = if esu_over || !enrolled { Level::DoFirst } else { Level::WorthDoing };
    let why = if esu_over {
        format!("Microsoft ended support for Windows 10 on October 14, 2025, and Extended Security Updates ended on {ESU_END_TEXT}. This PC no longer gets security fixes.")
    } else if enrolled {
        format!("Microsoft ended support for Windows 10 on October 14, 2025. This PC is signed up for Extended Security Updates, so it gets security fixes until {ESU_END_TEXT}.")
    } else {
        "Microsoft ended support for Windows 10 on October 14, 2025. Security fixes now only reach PCs signed up for Extended Security Updates, and this PC doesn't seem to be, so it gets none.".to_string()
    };
    let mut x = Finding::new("win10", SEC, level, "Windows 10 no longer gets regular security updates").why(why).conf(Confidence::BestGuess);
    if r.missing.is_empty() {
        x = x
            .step("This PC looks ready for Windows 11, which is a free upgrade. Open Windows Update and look for the Windows 11 offer.")
            .step("Back up anything important first. Your files and apps normally carry over.")
            .open("Open Windows Update", "ms-settings:windowsupdate");
    } else {
        x = x.step("Windows 11 won't install on this PC as it is. It's missing the items listed below.");
        let tpm_off = r.missing.iter().any(|m| m.starts_with("A TPM")) && !r.missing.iter().any(|m| m.starts_with("A newer processor"));
        if tpm_off {
            x = x.step("Most PCs from 2017 on have the security chip built into the processor, just switched off. Look for \"fTPM\" (AMD) or \"PTT\" (Intel) in the firmware settings (BIOS).");
        }
        x = if esu_over {
            x.step("Until you can move to a supported system, keep Windows Security on and be extra careful with downloads and email attachments.")
        } else if enrolled {
            x.step(format!("Extended Security Updates cover you until {ESU_END_TEXT}. Plan to move to a supported system before then."))
        } else {
            x.step(format!("Meanwhile, sign up for Extended Security Updates in Windows Update. It's free if you sync settings with a Microsoft account, or 1,000 Microsoft Rewards points, or a one-time $30, and it covers you until {ESU_END_TEXT}."))
                .open("Open Windows Update", "ms-settings:windowsupdate")
        };
        x = x.details(r.missing.iter().map(|m| format!("Missing: {m}")));
    }
    x = x.details(r.unsure.iter().map(|m| format!("Couldn't confirm: {m}")));
    f.push(x);
}

fn updates(s: &Snapshot, today: NaiveDate, f: &mut Vec<Finding>) {
    let w = s.system.g("winupdate");
    let failing: Vec<String> = w
        .a("Failed")
        .into_iter()
        .filter(|u| u.i("Times").unwrap_or(0) >= 2 && !update_name(&u.s("Title").unwrap_or_default()).1)
        .map(|u| {
            format!(
                "{}: failed {} times, last on {} (error {})",
                update_name(&u.s("Title").unwrap_or_default()).0,
                u.i("Times").unwrap_or(0),
                u.s("Last").unwrap_or_default(),
                u.s("Code").unwrap_or_default()
            )
        })
        .collect();
    if !failing.is_empty() {
        f.push(
            Finding::new(
                "updates-failing",
                SEC,
                Level::WorthDoing,
                format!("{} to install", plural(failing.len(), "update keeps failing", "updates keep failing")),
            )
            .why("Windows tried more than once and failed each time. A stuck update can hold back the security fixes that come after it.")
            .step("Restart the PC, then open Windows Update and choose Check for updates.")
            .step("If it fails again, search Microsoft's site for the error code shown below.")
            .details(failing)
            .open("Open Windows Update", "ms-settings:windowsupdate")
            .open("Open update history", "ms-settings:windowsupdate-history"),
        );
    }
    if let Some((until, end)) = w.s("PausedUntil").and_then(|u| date(&u).map(|d| (u, d))).filter(|(_, d)| *d >= today) {
        let start = w.s("PausedSince").and_then(|d| date(&d)).map_or(today, |d| d.min(today));
        // Settings pauses for five weeks at most, so a longer pause was set by a tool or a policy.
        let long = (end - start).num_days() > 35;
        f.push(
            Finding::new("updates-paused", SEC, if long { Level::WorthDoing } else { Level::NiceToHave }, format!("Windows updates are paused until {until}"))
                .why(if long {
                    "This pause is longer than Windows Settings allows, so a tool or setting probably made it. Security fixes wait until it ends."
                } else {
                    "Pausing is fine for a short while, but security fixes wait too."
                })
                .step("Open Windows Update and choose Resume updates when you're ready.")
                .open("Open Windows Update", "ms-settings:windowsupdate"),
        );
    }
    if w.b("AutoUpdateOff") == Some(true) {
        f.push(
            Finding::new("updates-auto-off", SEC, Level::WorthDoing, "Automatic updates are switched off")
                .why("A system policy stops Windows from installing updates by itself, so security fixes only arrive when someone checks by hand. \"Tweaking\" tools often set this.")
                .step("If this PC belongs to a school or company, ask them first. Otherwise, let Windows update itself again.")
                .act(Action::Fix(Fix::AllowAutoUpdates)),
        );
    }
    let hotfix = s.system.a("updates").first().and_then(|u| u.s("Installed")).and_then(|d| date(&d));
    let history = w.s("LastInstalled").and_then(|d| date(&d));
    if let Some(last) = hotfix.max(history) {
        let days = (today - last).num_days();
        if days >= 45 {
            f.push(
                Finding::new(
                    "updates-old",
                    SEC,
                    if days >= 90 { Level::DoFirst } else { Level::WorthDoing },
                    format!("No Windows update installed in {days} days"),
                )
                .why(format!(
                    "Microsoft fixes security holes every month, so this PC has missed {} of them.",
                    if days >= 90 { "several rounds" } else { "at least one round" }
                ))
                .step("Open Windows Update and choose Check for updates. Restart when it asks.")
                .detail(format!("Last update installed: {last}"))
                .conf(Confidence::BestGuess)
                .open("Open Windows Update", "ms-settings:windowsupdate"),
            );
        }
    }
}

fn restore(s: &Snapshot, today: NaiveDate, f: &mut Vec<Finding>) {
    let r = s.security.g("restore");
    match r.b("COn") {
        Some(false) => f.push(
            Finding::new("restore-off", Area::Stability, Level::WorthDoing, "System Restore is off")
                .why("System Restore saves a snapshot of Windows settings and drivers before big changes, so a bad driver or update can be rolled back without touching your files. It's off for drive C.")
                .act(Action::Fix(Fix::EnableSystemProtection)),
        ),
        Some(true) => {
            let latest = r.g("Latest").s("When");
            let age = latest.as_deref().and_then(date).map(|d| (today - d).num_days());
            if age.is_none_or(|a| a > 30) {
                let title = if latest.is_some() { "No restore point in the last 30 days" } else { "There are no restore points yet" };
                let mut x = Finding::new("restore-old", Area::Stability, Level::NiceToHave, title)
                    .why("Windows usually saves one before updates and driver installs. A fresh one gives you a safe point to roll back to before you change anything.")
                    .act(Action::RestorePoint);
                if let Some(l) = latest {
                    x = x.detail(format!("Latest restore point: {l} ({})", r.g("Latest").s("What").unwrap_or_default()));
                }
                f.push(x);
            }
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixes::Fix;
    use crate::rules::Level;
    use serde_json::json;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn pc(edition: &str, cpu: &str, tpm: Option<&str>) -> Snapshot {
        Snapshot {
            system: json!({
                "os": {"Edition": edition},
                "cpu": {"Name": cpu},
                "memory": {"TotalGB": 16},
                "device": {"FirmwareMode": "UEFI", "SecureBoot": true, "TpmPresent": tpm.is_some(), "TpmSpec": tpm},
            }),
            storage: json!({"volumes": [{"Letter": "C:", "SizeGB": 237}]}),
            ..Default::default()
        }
    }

    fn run(s: &Snapshot, today: &str) -> Vec<Finding> {
        let mut f = vec![];
        check_on(s, day(today), &mut f);
        f
    }

    #[test]
    fn processors_are_sorted_by_generation() {
        for (name, want) in [
            ("Intel(R) Core(TM) i7-7700K CPU @ 4.20GHz", Some(false)),
            ("Intel(R) Core(TM) i5-8400 CPU @ 2.80GHz", Some(true)),
            ("Intel(R) Core(TM) i5-10400F CPU @ 2.90GHz", Some(true)),
            ("13th Gen Intel(R) Core(TM) i7-13700K", Some(true)),
            ("Intel(R) Core(TM) Ultra 7 155H", Some(true)),
            ("Intel(R) Core(TM) i5 CPU 750 @ 2.67GHz", Some(false)),
            ("AMD Ryzen 5 1600 Six-Core Processor", Some(false)),
            ("AMD Ryzen 5 2600 Six-Core Processor", Some(true)),
            ("AMD Ryzen 5 2400G with Radeon Vega Graphics", Some(false)),
            ("AMD Ryzen 7 5800X3D 8-Core Processor", Some(true)),
            ("AMD Ryzen 5 PRO 4650G with Radeon Graphics", Some(true)),
            ("AMD Ryzen AI 9 HX 370 w/ Radeon 890M", Some(true)),
            ("Intel(R) Pentium(R) CPU G4560 @ 3.50GHz", None),
        ] {
            assert_eq!(cpu_supported(name), want, "{name}");
        }
    }

    #[test]
    fn ready_windows_10_pc_is_pointed_at_the_free_upgrade() {
        let s = pc("Microsoft Windows 10 Pro", "Intel(R) Core(TM) i5-8400 CPU @ 2.80GHz", Some("2.0"));
        let f = run(&s, "2026-11-12");
        let x = f.iter().find(|x| x.id == "win10").expect("windows 10 finding");
        assert_eq!(x.level, Level::DoFirst, "not signed up for Extended Security Updates, so no fixes at all");
        assert!(x.steps.iter().any(|s| s.contains("free upgrade")), "{:?}", x.steps);
        assert!(win11_readiness(&s).missing.is_empty());
        let late = run(&s, "2027-10-20");
        assert_eq!(late.iter().find(|x| x.id == "win10").unwrap().level, Level::DoFirst);
    }

    #[test]
    fn signed_up_windows_10_pc_has_until_october_2027() {
        let level = |esu: serde_json::Value, licensed: Option<bool>, today: &str| {
            let mut s = pc("Microsoft Windows 10 Home", "Intel(R) Core(TM) i5-4590 CPU @ 3.30GHz", None);
            s.system["os"]["EsuState"] = esu;
            s.system["os"]["EsuLicensed"] = json!(licensed);
            run(&s, today).into_iter().find(|x| x.id == "win10").map(|x| (x.level, x.steps))
        };
        for state in [3, 5] {
            assert_eq!(level(json!(state), None, "2026-11-12").unwrap().0, Level::WorthDoing, "state {state} is enrolled");
        }
        assert_eq!(level(json!(null), Some(true), "2026-11-12").unwrap().0, Level::WorthDoing, "an ESU licence counts too");
        assert_eq!(level(json!(2), Some(false), "2026-11-12").unwrap().0, Level::DoFirst, "eligible isn't enrolled");
        assert_eq!(level(json!(3), None, "2027-10-20").unwrap().0, Level::DoFirst, "coverage has ended");
        let (_, steps) = level(json!(3), None, "2026-11-12").unwrap();
        assert!(!steps.iter().any(|s| s.contains("sign up")), "already signed up: {steps:?}");
    }

    #[test]
    fn missed_updates_grow_more_urgent() {
        let mut s = pc("Microsoft Windows 11 Pro", "AMD Ryzen 5 2600", Some("2.0"));
        let mut level = |last: &str| {
            s.system["winupdate"] = json!({ "LastInstalled": last });
            run(&s, "2026-11-12").into_iter().find(|x| x.id == "updates-old").map(|x| x.level)
        };
        assert_eq!(level("2026-09-29"), None, "44 days");
        assert_eq!(level("2026-09-28"), Some(Level::WorthDoing), "45 days");
        assert_eq!(level("2026-08-15"), Some(Level::WorthDoing), "89 days");
        assert_eq!(level("2026-08-14"), Some(Level::DoFirst), "90 days");
    }

    #[test]
    fn pauses_longer_than_settings_allow_are_worth_doing() {
        let mut s = pc("Microsoft Windows 11 Pro", "AMD Ryzen 5 2600", Some("2.0"));
        let mut level = |since: Option<&str>, until: &str| {
            s.system["winupdate"] = json!({ "PausedSince": since, "PausedUntil": until, "LastInstalled": "2026-11-01" });
            run(&s, "2026-11-12").into_iter().find(|x| x.id == "updates-paused").map(|x| x.level)
        };
        assert_eq!(level(Some("2026-11-05"), "2026-12-10"), Some(Level::NiceToHave), "35 days, the most Settings offers");
        assert_eq!(level(Some("2026-11-05"), "2027-03-01"), Some(Level::WorthDoing));
        assert_eq!(level(None, "2027-03-01"), Some(Level::WorthDoing));
        assert_eq!(level(None, "2026-11-20"), Some(Level::NiceToHave));
    }

    #[test]
    fn unready_pc_hears_what_is_missing() {
        let s = pc("Microsoft Windows 10 Home", "Intel(R) Core(TM) i5-4590 CPU @ 3.30GHz", None);
        let r = win11_readiness(&s);
        assert_eq!(r.missing.len(), 2, "{r:?}");
        let f = run(&s, "2026-11-12");
        let x = f.iter().find(|x| x.id == "win10").unwrap();
        assert!(x.steps.iter().any(|s| s.contains("Extended Security Updates")), "{:?}", x.steps);
    }

    #[test]
    fn windows_11_gets_no_upgrade_nag() {
        assert!(!run(&pc("Microsoft Windows 11 Pro", "AMD Ryzen 5 2600", Some("2.0")), "2026-11-12").iter().any(|x| x.id == "win10"));
    }

    #[test]
    fn update_problems_are_reported() {
        let mut s = pc("Microsoft Windows 11 Pro", "AMD Ryzen 5 2600", Some("2.0"));
        s.system["winupdate"] = json!({
            "LastInstalled": "2026-11-01",
            "Failed": [
                {"Title": "2026-10 Cumulative Update for Windows 11 (KB5000001)", "Times": 3, "Last": "2026-11-02", "Code": "0x800F0922"},
                {"Title": "Once only", "Times": 1, "Last": "2026-10-02", "Code": "0x1"},
                {"Title": "9NBLGGH537C2-Microsoft.XboxGameOverlay", "Times": 4, "Last": "2026-10-22", "Code": "0x80240034"}
            ],
            "PausedUntil": "2026-11-20",
            "AutoUpdateOff": true
        });
        s.system["updates"] = json!([{"Installed": "2026-05-01"}]);
        let f = run(&s, "2026-11-12");
        let failing = f.iter().find(|x| x.id == "updates-failing").expect("failing");
        assert_eq!(failing.details.len(), 1, "a single failure is noise");
        assert!(f.iter().any(|x| x.id == "updates-paused"));
        let auto = f.iter().find(|x| x.id == "updates-auto-off").unwrap();
        assert!(auto.actions.contains(&Action::Fix(Fix::AllowAutoUpdates)));
        assert!(!f.iter().any(|x| x.id == "updates-old"), "Windows Update history is newer than the hotfix list");
        s.system["winupdate"]["LastInstalled"] = json!("2026-08-01");
        assert!(run(&s, "2026-11-12").iter().any(|x| x.id == "updates-old"));
    }

    #[test]
    fn store_app_updates_get_readable_names() {
        assert_eq!(update_name("9NBLGGH537C2-Microsoft.XboxGameOverlay"), ("Xbox Game Overlay".to_string(), true));
        assert_eq!(update_name("9MV0B5HZVK9Z-Microsoft.GamingApp"), ("Gaming App".to_string(), true));
        let kb = "2026-10 Cumulative Update for Windows 11 (KB5000001)";
        assert_eq!(update_name(kb), (kb.to_string(), false));
    }

    #[test]
    fn system_restore_state_is_reported() {
        let mut s = pc("Microsoft Windows 11 Pro", "AMD Ryzen 5 2600", Some("2.0"));
        s.security = json!({"restore": {"COn": false}});
        let f = run(&s, "2026-11-12");
        assert!(f.iter().find(|x| x.id == "restore-off").unwrap().actions.contains(&Action::Fix(Fix::EnableSystemProtection)));
        s.security = json!({"restore": {"COn": true, "Latest": {"When": "2026-09-01 10:00", "What": "Windows Update"}}});
        let f = run(&s, "2026-11-12");
        assert!(f.iter().find(|x| x.id == "restore-old").unwrap().actions.contains(&Action::RestorePoint));
        s.security = json!({"restore": {"COn": null}});
        assert!(!run(&s, "2026-11-12").iter().any(|x| x.id.starts_with("restore")), "unknown without admin rights");
    }

    #[test]
    fn windows_11_versions_run_out_of_fixes() {
        let level = |edition: &str, version: &str, insider: &str, today: &str| {
            let mut s = pc(edition, "AMD Ryzen 5 2600", Some("2.0"));
            s.system["os"]["Version"] = json!(version);
            s.system["os"]["InsiderChannel"] = json!(insider);
            run(&s, today).into_iter().find(|x| x.id == "win11-version").map(|x| x.level)
        };
        let pro = "Microsoft Windows 11 Pro";
        assert_eq!(level(pro, "24H2", "", "2026-11-14"), Some(Level::DoFirst), "ended 13 October 2026");
        assert_eq!(level(pro, "24H2", "", "2026-10-13"), Some(Level::WorthDoing), "its last day");
        assert_eq!(level(pro, "25H2", "", "2026-11-14"), None);
        assert_eq!(level(pro, "25H2", "", "2027-07-20"), Some(Level::WorthDoing), "within 90 days of the end");
        assert_eq!(level(pro, "25H2", "", "2027-10-13"), Some(Level::DoFirst));
        assert_eq!(level("Microsoft Windows 11 Home", "23H2", "", "2026-11-14"), Some(Level::DoFirst));
        assert_eq!(level("Microsoft Windows 11 Enterprise", "24H2", "", "2026-11-14"), None, "other dates apply");
        assert_eq!(level(pro, "24H2", "Dev External", "2026-11-14"), None, "preview builds update themselves");
        assert_eq!(level(pro, "27H1", "", "2026-11-14"), None, "newer than we know");
    }
}
