//! Turns a snapshot into plain-language findings, each with a reason, steps and actions.

pub mod data;
mod hardware;
pub mod security;
mod speed;
mod stability;
pub mod upgrades;
mod windows;
pub use stability::{bugcheck_code, crashed_app};
pub use windows::{update_name, win11_readiness};

use crate::fixes::Fix;
use crate::gui::Page;
use crate::snapshot::Snapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Area {
    Security,
    Stability,
    Speed,
    Hardware,
}

impl Area {
    pub const ALL: [Area; 4] = [Area::Security, Area::Stability, Area::Speed, Area::Hardware];
    pub fn name(self) -> &'static str {
        match self {
            Area::Security => "Security",
            Area::Stability => "Stability",
            Area::Speed => "Speed",
            Area::Hardware => "Hardware",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    DoFirst,
    WorthDoing,
    NiceToHave,
    AllGood,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::DoFirst => "Do first",
            Level::WorthDoing => "Worth doing",
            Level::NiceToHave => "Nice to have",
            Level::AllGood => "All good",
        }
    }
    fn penalty(self) -> i32 {
        match self {
            Level::DoFirst => 30,
            Level::WorthDoing => 10,
            Level::NiceToHave => 3,
            Level::AllGood => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Measured,
    BestGuess,
}

impl Confidence {
    pub fn name(self) -> &'static str {
        match self {
            Confidence::Measured => "Measured on this PC",
            Confidence::BestGuess => "Our best guess",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Fix(Fix),
    Open {
        label: &'static str,
        target: String,
    },
    Goto(Page),
    AnalyzeDumps,
    Elevate,
    RestorePoint,
    /// Copy Documents, Desktop and Pictures to a USB drive.
    CopyFiles,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub id: String,
    pub area: Area,
    pub level: Level,
    pub title: String,
    pub summary: String,
    pub steps: Vec<String>,
    pub details: Vec<String>,
    pub confidence: Confidence,
    pub actions: Vec<Action>,
}

impl Finding {
    pub fn new(id: impl Into<String>, area: Area, level: Level, title: impl Into<String>) -> Self {
        Finding {
            id: id.into(),
            area,
            level,
            title: title.into(),
            summary: String::new(),
            steps: vec![],
            details: vec![],
            confidence: Confidence::Measured,
            actions: vec![],
        }
    }
    pub fn why(mut self, s: impl Into<String>) -> Self {
        self.summary = s.into();
        self
    }
    pub fn step(mut self, s: impl Into<String>) -> Self {
        self.steps.push(s.into());
        self
    }
    pub fn detail(mut self, s: impl Into<String>) -> Self {
        self.details.push(s.into());
        self
    }
    pub fn details(mut self, it: impl IntoIterator<Item = String>) -> Self {
        self.details.extend(it);
        self
    }
    pub fn conf(mut self, c: Confidence) -> Self {
        self.confidence = c;
        self
    }
    pub fn act(mut self, a: Action) -> Self {
        self.actions.push(a);
        self
    }
    pub fn acts(mut self, it: impl IntoIterator<Item = Action>) -> Self {
        self.actions.extend(it);
        self
    }
    pub fn open(self, label: &'static str, target: &str) -> Self {
        self.act(Action::Open { label, target: target.to_string() })
    }
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    let digits = n.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{grouped} {}", if n == 1 { one } else { many })
}

/// "Desktop, Documents and Pictures".
pub fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

pub fn evaluate(s: &Snapshot) -> Vec<Finding> {
    let mut f = Vec::new();
    security::check(s, &mut f);
    stability::check(s, &mut f);
    speed::check(s, &mut f);
    hardware::check(s, &mut f);
    windows::check(s, &mut f);
    for area in Area::ALL {
        if !f.iter().any(|x| x.area == area && x.level != Level::AllGood) {
            f.push(
                Finding::new(
                    format!("{}-clear", area.name().to_lowercase()),
                    area,
                    Level::AllGood,
                    format!("Nothing to fix in {}", area.name().to_lowercase()),
                )
                .why("Every check in this area came back fine."),
            );
        }
    }
    f.sort_by_key(|a| (a.level, a.area));
    f
}

pub struct Score {
    pub value: u8,
    pub verdict: &'static str,
    pub open: usize,
}

pub fn score(findings: &[Finding], area: Area) -> Score {
    let mine: Vec<_> = findings.iter().filter(|f| f.area == area && f.level != Level::AllGood).collect();
    let v = (100 - mine.iter().map(|f| f.level.penalty()).sum::<i32>()).clamp(0, 100) as u8;
    let verdict = match v {
        85..=100 => "Looks good",
        60..=84 => "Worth a look",
        _ => "Needs attention",
    };
    Score { value: v, verdict, open: mine.len() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> Snapshot {
        let mut s = Snapshot { elevated: true, ..Default::default() };
        s.security = json!({
            "defender": {"Present": true, "AntivirusEnabled": true, "RealTime": true, "Tamper": true, "SignatureAgeDays": 0, "PUAProtection": 1, "ExclusionPaths": []},
            "firewallProfiles": [{"Name": "Public", "Enabled": true}],
            "access": {"UAC": true, "RemoteDesktop": false, "RemoteAssistance": false, "SMB1": false, "AutoLogon": false}
        });
        s.system = json!({"device": {"SecureBoot": true, "FirmwareMode": "UEFI"}, "os": {"PendingReboot": false}});
        s
    }

    #[test]
    fn healthy_pc_is_all_good() {
        let f = evaluate(&base());
        for a in Area::ALL {
            assert_eq!(score(&f, a).value, 100, "{a:?}: {:?}", f.iter().filter(|x| x.area == a).map(|x| &x.title).collect::<Vec<_>>());
        }
    }

    #[test]
    fn whole_drive_exclusion_is_do_first() {
        let mut s = base();
        s.security["defender"]["ExclusionPaths"] = json!(["C:\\", "D:\\Games"]);
        let f = evaluate(&s);
        let x = f.iter().find(|x| x.id == "defender-exclusions").expect("finding");
        assert_eq!(x.level, Level::DoFirst);
        assert!(x.actions.iter().filter(|a| matches!(a, Action::Fix(_))).count() == 2);
        assert!(score(&f, Area::Security).value <= 70);
    }

    #[test]
    fn known_bad_extension_and_old_winrar_are_flagged() {
        let mut s = base();
        s.extensions.push(crate::snapshot::Extension {
            browser: "Chrome".into(),
            id: "eppiocemhmnlbhjplcgkofciiegomcon".into(),
            name: "Urban VPN Proxy".into(),
            enabled: true,
            from_store: true,
            ..Default::default()
        });
        s.software = json!({"apps": [{"Name": "WinRAR 7.01 (64-bit)", "Version": "7.01.0"}, {"Name": "7-Zip 24.09 (x64)", "Version": "24.09"}]});
        let f = evaluate(&s);
        assert!(f.iter().any(|x| x.id.starts_with("ext-bad") && x.level == Level::DoFirst));
        assert!(f.iter().any(|x| x.id.starts_with("vuln-WinRAR")));
        assert!(!f.iter().any(|x| x.id.starts_with("vuln-7-Zip")), "24.09 is fixed");
    }

    #[test]
    fn camera_use_is_listed_and_odd_folders_flagged() {
        let mut s = base();
        s.security["deviceAccess"] = json!([
            {"Kind": "webcam", "App": "Discord", "Path": "C:\\Users\\me\\AppData\\Local\\Discord\\app-1\\Discord.exe", "LastUsed": "2026-11-01 10:00", "InUse": false},
            {"Kind": "webcam", "App": "Discord", "Path": "C:\\Users\\me\\AppData\\Local\\Discord\\app-1\\Discord.exe", "LastUsed": "2026-11-05 10:00", "InUse": true},
            {"Kind": "location", "App": "svchost", "Path": "C:\\Windows\\System32\\svchost.exe", "LastUsed": "2026-11-02 08:00", "InUse": false},
            {"Kind": "microphone", "App": "setup", "Path": "C:\\Users\\me\\Downloads\\setup.exe", "LastUsed": "2026-10-01 09:00", "InUse": false},
            {"Kind": "microphone", "App": "WhatsAppDesktop", "Path": null, "Store": true, "LastUsed": "2026-11-03 09:00", "InUse": false}
        ]);
        let rows = security::device_use(&s);
        assert_eq!(rows.len(), 4, "Discord appears once: {rows:?}");
        assert_eq!(rows[0].app, "Discord");
        assert!(rows[0].in_use, "the newest record wins");
        assert!(rows.iter().find(|r| r.app == "svchost").unwrap().windows);
        assert_eq!(rows.iter().find(|r| r.what == "Microphone" && r.app.starts_with("WhatsApp")).unwrap().app, "WhatsApp");
        assert_eq!(security::friendly_app("VALORANT-Win64-Shipping"), "VALORANT");
        assert_eq!(security::friendly_app("immersivecontrolpanel"), "Windows Settings");
        assert_eq!(security::friendly_app("Google Chrome"), "Google Chrome");
        let f = evaluate(&s);
        let x = f.iter().find(|x| x.id == "device-odd-app").expect("Downloads app flagged");
        assert_eq!(x.details.len(), 1, "{:?}", x.details);
    }

    #[test]
    fn single_memory_stick_is_noticed() {
        let mut s = base();
        s.system["memory"] = json!({"TotalGB": 16, "Slots": 4, "Sticks": [{"SizeGB": 16, "SpeedMTs": 3200, "ConfiguredMTs": 2133}]});
        s.system["device"]["ChassisTypes"] = json!([3]);
        let f = evaluate(&s);
        assert!(f.iter().any(|x| x.id == "ram-single-channel"));
        assert!(f.iter().any(|x| x.id == "ram-slow"));
    }

    fn level_of(f: &[Finding], id: &str) -> Option<Level> {
        f.iter().find(|x| x.id == id).map(|x| x.level)
    }

    fn ago(days: i64) -> String {
        (chrono::Local::now().date_naive() - chrono::Duration::days(days)).format("%Y-%m-%d").to_string()
    }

    #[test]
    fn open_doors_are_worth_doing() {
        let mut s = base();
        s.security["access"]["ChromeRemoteDesktop"] = json!("Running / Automatic");
        s.security["accounts"] = json!([{"Name": "Spare", "Enabled": true, "PasswordRequired": false, "Source": "Local"}]);
        s.software = json!({"services": [{"Name": "TeamViewer"}]});
        s.system["device"] = json!({"SecureBoot": false, "FirmwareMode": "UEFI", "HasBattery": true});
        s.storage = json!({"volumes": [{"Letter": "C:", "SizeGB": 476, "FreeGB": 200, "BitLocker": "Off"}]});
        let f = evaluate(&s);
        for id in ["crd", "no-password", "remote-tools", "secureboot-off", "encryption-off"] {
            assert_eq!(level_of(&f, id), Some(Level::WorthDoing), "{id}");
        }
    }

    #[test]
    fn threats_already_dealt_with_need_nothing() {
        let mut s = base();
        s.security["threats"] = json!([{"Name": "Trojan:Win32/Wacatac", "Date": "2026-11-01", "ActionSuccess": true}]);
        let f = evaluate(&s);
        assert_eq!(level_of(&f, "threats-handled"), Some(Level::AllGood));
        assert_eq!(score(&f, Area::Security).value, 100);
    }

    #[test]
    fn virus_definitions_go_stale_after_three_days() {
        let stale = |days: i64| {
            let mut s = base();
            s.security["defender"]["SignatureAgeDays"] = json!(days);
            evaluate(&s).into_iter().find(|x| x.id == "av-stale")
        };
        assert!(stale(2).is_none());
        assert_eq!(stale(3).map(|x| x.level), Some(Level::WorthDoing));
        let never = stale(65535).expect("65535 means never updated");
        assert_eq!(never.level, Level::WorthDoing);
        assert!(never.title.contains("never"), "{}", never.title);
    }

    #[test]
    fn memory_integrity_matters_where_it_can_run() {
        let hvci = |state: &str, vbs: Option<bool>| {
            let mut s = base();
            s.system["os"]["MemoryIntegrity"] = json!(state);
            s.system["os"]["VbsAvailable"] = json!(vbs);
            level_of(&evaluate(&s), "hvci-off")
        };
        assert_eq!(hvci("Off", Some(true)), Some(Level::WorthDoing));
        assert_eq!(hvci("Off", Some(false)), Some(Level::NiceToHave), "the hardware can't run it as set up");
        assert_eq!(hvci("Off", None), Some(Level::NiceToHave));
        assert_eq!(hvci("Enabled, not running", Some(true)), Some(Level::NiceToHave), "a driver is in the way");
        assert_eq!(hvci("On", Some(true)), None);
    }

    #[test]
    fn app_crashes_count_by_app_and_by_spread() {
        let crashes = |list: serde_json::Value| {
            let mut s = base();
            s.health = json!({ "appCrashes": list });
            evaluate(&s)
        };
        let f = crashes(json!([{"App": "chrome.exe", "Name": "Google Chrome", "Count": 5}, {"App": "x.exe", "Count": 1}]));
        let x = f.iter().find(|x| x.id == "appcrash-chrome.exe").expect("five crashes of one app");
        assert_eq!(x.level, Level::WorthDoing);
        assert!(x.title.starts_with("Google Chrome"), "{}", x.title);
        assert!(!crashes(json!([{"App": "a.exe", "Count": 4}, {"App": "b.exe", "Count": 1}])).iter().any(|x| x.id.starts_with("appcrash")));
        let f = crashes(json!([{"App": "VALORANT-Win64-Shipping.exe", "Name": null, "Count": 16}]));
        let x = f.iter().find(|x| x.id.starts_with("appcrash")).unwrap();
        assert!(x.title.starts_with("VALORANT crashed"), "no description: the file name is cleaned up: {}", x.title);
        let f = crashes(json!([{"App": "a.exe", "Count": 6}, {"App": "b.exe", "Count": 1}, {"App": "c.exe", "Count": 1}]));
        let all: Vec<_> = f.iter().filter(|x| x.id.starts_with("appcrash")).collect();
        assert_eq!(all.len(), 1, "one finding for the pattern, not one per app");
        assert_eq!(all[0].id, "appcrash-many");
        assert_eq!(all[0].level, Level::WorthDoing);
        assert_eq!(all[0].details.len(), 3);
    }

    #[test]
    fn graphics_drivers_age_after_a_year_and_virtual_screens_are_skipped() {
        let mut s = base();
        s.system["gpus"] = json!([
            {"Name": "NVIDIA GeForce RTX 3060", "DriverDate": ago(380)},
            {"Name": "AMD Radeon RX 6600", "DriverDate": ago(300)},
            {"Name": "Parsec Virtual Display Adapter", "DriverDate": ago(1500)},
            {"Name": "Microsoft Remote Display Adapter", "DriverDate": ago(1500)}
        ]);
        let f = evaluate(&s);
        let old: Vec<_> = f.iter().filter(|x| x.id.starts_with("gpu-old")).map(|x| (x.id.as_str(), x.level)).collect();
        assert_eq!(old, [("gpu-old-NVIDIA GeForce RTX 3060", Level::WorthDoing)]);
    }

    #[test]
    fn a_restart_left_waiting_a_week_is_worth_doing() {
        let reboot = |since: Option<String>| {
            let mut s = base();
            s.system["os"]["PendingReboot"] = json!(true);
            s.system["winupdate"] = json!({ "RestartWaitingSince": since });
            level_of(&evaluate(&s), "reboot")
        };
        assert_eq!(reboot(Some(ago(7))), Some(Level::WorthDoing));
        assert_eq!(reboot(Some(ago(2))), Some(Level::NiceToHave));
        assert_eq!(reboot(None), Some(Level::WorthDoing), "unknown start: it may have waited long");
    }

    fn win11(s: &mut Snapshot) {
        s.system["os"]["Edition"] = json!("Microsoft Windows 11 Home");
        s.system["os"]["Build"] = json!("26200.7000");
    }

    #[test]
    fn free_space_levels_follow_the_size_of_the_drive() {
        let space = |letter: &str, size: f64, free: f64| {
            let mut s = base();
            s.storage = json!({"volumes": [{"Letter": letter, "SizeGB": size, "FreeGB": free}]});
            level_of(&evaluate(&s), &format!("space-{letter}"))
        };
        // Small drive: 5% and 10%, with floors of 5 GB and 15 GB.
        assert_eq!(space("C:", 128.0, 6.0), Some(Level::DoFirst));
        assert_eq!(space("C:", 128.0, 14.0), Some(Level::WorthDoing));
        assert_eq!(space("C:", 128.0, 16.0), None);
        // Big drive: capped at 10 GB and 25 GB, so 150 GB free on 2 TB is fine.
        assert_eq!(space("C:", 2000.0, 9.5), Some(Level::DoFirst));
        assert_eq!(space("C:", 2000.0, 24.0), Some(Level::WorthDoing));
        assert_eq!(space("C:", 2000.0, 150.0), None);
        assert_eq!(space("D:", 1000.0, 40.0), Some(Level::NiceToHave), "other drives keep the 5% rule");
    }

    #[test]
    fn files_without_a_backup_are_worth_doing() {
        let backup = |b: serde_json::Value, apps: serde_json::Value| {
            let mut s = base();
            s.storage = json!({ "backup": b });
            s.software = json!({ "apps": apps });
            let f = evaluate(&s);
            (level_of(&f, "no-backup"), level_of(&f, "onedrive-stopped"))
        };
        let none = json!({"OneDriveSignedIn": false, "OneDriveRunning": false, "FoldersInOneDrive": [], "FileHistory": false});
        assert_eq!(backup(none.clone(), json!([])), (Some(Level::WorthDoing), None));
        assert_eq!(backup(none.clone(), json!([{"Name": "Macrium Reflect Free"}])), (None, None), "a backup app counts");
        let mut fh = none.clone();
        fh["FileHistory"] = json!(true);
        assert_eq!(backup(fh, json!([])), (None, None));
        let synced = json!({"OneDriveSignedIn": true, "OneDriveRunning": true, "FoldersInOneDrive": ["Desktop", "Documents"], "FileHistory": false});
        assert_eq!(backup(synced, json!([])), (None, None));
        // Folders still point into OneDrive, but nobody is signed in, so nothing syncs.
        let stopped = json!({"OneDriveSignedIn": false, "OneDriveRunning": false, "FoldersInOneDrive": ["Desktop", "Documents"], "FileHistory": false});
        assert_eq!(backup(stopped, json!([])), (None, Some(Level::WorthDoing)));
        // Signed in, but only the OneDrive folder itself syncs.
        let bare = json!({"OneDriveSignedIn": true, "OneDriveRunning": true, "FoldersInOneDrive": [], "FileHistory": false});
        assert_eq!(backup(bare, json!([])), (Some(Level::WorthDoing), None));
        let mut s = base();
        s.storage = json!({});
        assert_eq!(level_of(&evaluate(&s), "no-backup"), None, "no data, no guess");
    }

    #[test]
    fn secure_boot_certificates_must_be_the_2023_ones() {
        let cert = |status: Option<&str>, capable: Option<i64>, sb: bool| {
            let mut s = base();
            win11(&mut s);
            s.system["device"]["SecureBoot"] = json!(sb);
            s.system["device"]["SecureBootCert"] = json!(status);
            s.system["device"]["SecureBootCertCapable"] = json!(capable);
            evaluate(&s).into_iter().find(|x| x.id == "secureboot-cert").map(|x| (x.level, x.confidence))
        };
        assert_eq!(cert(Some("NotStarted"), Some(0), true), Some((Level::WorthDoing, Confidence::BestGuess)));
        assert_eq!(cert(Some("InProgress"), Some(1), true).map(|x| x.0), Some(Level::WorthDoing));
        assert_eq!(cert(Some("Updated"), Some(1), true).map(|x| x.0), Some(Level::WorthDoing), "not booting the new boot manager yet");
        assert_eq!(cert(Some("Updated"), Some(2), true), None);
        assert_eq!(cert(None, None, true), None, "no data, no guess");
        assert_eq!(cert(Some("NotStarted"), Some(0), false), None, "Secure Boot off has its own to-do");
        let mut s = base();
        s.system["os"]["Edition"] = json!("Microsoft Windows 10 Home");
        s.system["device"]["SecureBootCert"] = json!("NotStarted");
        assert_eq!(level_of(&evaluate(&s), "secureboot-cert"), None, "Windows 10 without updates is covered by its own to-do");
    }

    #[test]
    fn security_extras_that_can_get_in_the_way_are_nice_to_have() {
        let mut s = base();
        win11(&mut s);
        s.security["defender"]["SmartAppControl"] = json!("Off");
        s.security["defender"]["ControlledFolderAccess"] = json!(0);
        s.security["dailyAccount"] = json!({"Name": "PC\\sam", "Admin": true, "OtherAdmins": ["Helper"]});
        let f = evaluate(&s);
        for id in ["sac-off", "cfa-off", "daily-admin"] {
            assert_eq!(level_of(&f, id), Some(Level::NiceToHave), "{id}");
        }
        // Already on, or not Windows' to manage.
        s.security["defender"]["SmartAppControl"] = json!("Eval");
        s.security["defender"]["ControlledFolderAccess"] = json!(2);
        s.security["dailyAccount"]["OtherAdmins"] = json!([]);
        let f = evaluate(&s);
        for id in ["sac-off", "cfa-off", "daily-admin"] {
            assert_eq!(level_of(&f, id), None, "{id}");
        }
        let mut other = base();
        other.security["defender"]["ControlledFolderAccess"] = json!(0);
        other.security["otherAntivirus"] = json!(["Norton 360"]);
        assert_eq!(level_of(&evaluate(&other), "cfa-off"), None, "only matters while Windows Security protects the PC");
        let mut work = base();
        win11(&mut work);
        work.system["device"]["Joined"] = json!(true);
        work.security["defender"]["SmartAppControl"] = json!("Off");
        work.security["dailyAccount"] = json!({"Name": "CORP\\sam", "Admin": true, "OtherAdmins": ["Helper"]});
        let f = evaluate(&work);
        assert_eq!((level_of(&f, "sac-off"), level_of(&f, "daily-admin")), (None, None), "work PCs are managed by their IT");
        let mut no_uac = base();
        no_uac.security["access"]["UAC"] = json!(false);
        no_uac.security["dailyAccount"] = json!({"Name": "PC\\sam", "Admin": true, "OtherAdmins": ["Helper"]});
        assert_eq!(level_of(&evaluate(&no_uac), "daily-admin"), None, "UAC off already has its own to-do");
    }

    #[test]
    fn lsa_protection_off_is_worth_doing_where_windows_security_warns() {
        let lsa = |edition: &str, build: &str, ppl: Option<i64>, running: Option<bool>| {
            let mut s = base();
            s.system["os"]["Edition"] = json!(edition);
            s.system["os"]["Build"] = json!(build);
            s.security["access"]["LsaPpl"] = json!(ppl);
            s.security["access"]["LsaRunning"] = json!(running);
            level_of(&evaluate(&s), "lsa-off")
        };
        assert_eq!(lsa("Microsoft Windows 11 Home", "26200.7000", None, Some(false)), Some(Level::WorthDoing));
        assert_eq!(lsa("Microsoft Windows 11 Home", "26200.7000", None, Some(true)), None, "Windows says it started protected");
        assert_eq!(lsa("Microsoft Windows 11 Home", "26200.7000", Some(2), Some(false)), None, "on, starts protected after a restart");
        assert_eq!(lsa("Microsoft Windows 11 Home", "26200.7000", None, None), Some(Level::WorthDoing));
        assert_eq!(lsa("Microsoft Windows 11 Home", "22000.2000", None, Some(false)), None, "no switch for it before 22H2");
        assert_eq!(lsa("Microsoft Windows 10 Pro", "19045.6000", None, Some(false)), None, "Windows 10 has no safe switch");
        let mut work = base();
        win11(&mut work);
        work.system["device"]["Joined"] = json!(true);
        assert_eq!(level_of(&evaluate(&work), "lsa-off"), None, "work PCs are managed by their IT");
    }

    #[test]
    fn activation_and_preview_builds() {
        let os = |activation: &str, insider: &str| {
            let mut s = base();
            s.system["os"]["Activation"] = json!(activation);
            s.system["os"]["InsiderChannel"] = json!(insider);
            let f = evaluate(&s);
            (level_of(&f, "not-activated"), level_of(&f, "insider"))
        };
        assert_eq!(os("Unlicensed", ""), (Some(Level::WorthDoing), None));
        assert_eq!(os("Status 5", ""), (Some(Level::WorthDoing), None), "notification mode");
        assert_eq!(os("Status 2", ""), (Some(Level::NiceToHave), None), "still in its grace period");
        assert_eq!(os("Activated", ""), (None, None));
        assert_eq!(os("Activated", "Dev External"), (None, Some(Level::NiceToHave)));
    }

    #[test]
    fn disk_errors_count_on_internal_drives() {
        let errors = |list: serde_json::Value| {
            let mut s = base();
            s.storage = json!({"disks": [{"Number": 0, "Bus": "NVMe"}, {"Number": 1, "Bus": "USB"}], "volumes": [{"Letter": "C:", "Disk": 0}, {"Letter": "D:", "Disk": 1}], "diskErrors": list});
            level_of(&evaluate(&s), "disk-errors")
        };
        assert_eq!(errors(json!([{"Source": "disk", "Id": 51, "Count": 1, "Disk": 0}])), None, "one hiccup");
        assert_eq!(errors(json!([{"Source": "disk", "Id": 51, "Count": 3, "Disk": 0}])), Some(Level::WorthDoing));
        assert_eq!(
            errors(json!([{"Source": "stornvme", "Id": 129, "Count": 2, "Disk": null}, {"Source": "disk", "Id": 153, "Count": 1, "Disk": 0}])),
            Some(Level::WorthDoing)
        );
        assert_eq!(errors(json!([{"Source": "disk", "Id": 7, "Count": 1, "Disk": 0}])), Some(Level::WorthDoing), "a bad block");
        assert_eq!(errors(json!([{"Source": "Ntfs", "Id": 55, "Count": 1, "Disk": null}])), Some(Level::WorthDoing), "file system damage");
        assert_eq!(errors(json!([{"Source": "disk", "Id": 7, "Count": 9, "Disk": 1}])), None, "USB drives come and go");
        let ntfs = |id: i64, n: i64, vol: &str| errors(json!([{"Source": "Microsoft-Windows-Ntfs", "Id": id, "Count": n, "Disk": null, "Volume": vol}]));
        assert_eq!(ntfs(55, 1, "C"), Some(Level::WorthDoing), "damage on the system drive");
        assert_eq!(ntfs(140, 2, "C"), None, "lost writes need repeating");
        assert_eq!(ntfs(140, 3, "C"), Some(Level::WorthDoing));
        assert_eq!(ntfs(140, 9, "E"), None, "a memory stick pulled out");
        assert_eq!(ntfs(55, 1, "D"), None, "a USB drive");
    }

    #[test]
    fn graphics_driver_resets_repeat_before_they_count() {
        let resets = |tdr: Vec<String>, watchdog: Vec<String>, gpu: &str| {
            let mut s = base();
            s.system["gpus"] = json!([{ "Name": gpu, "DriverDate": ago(30) }]);
            s.health = json!({ "gpuResets": { "Tdr": tdr, "Watchdog": watchdog } });
            level_of(&evaluate(&s), "gpu-resets")
        };
        let gpu = "AMD Radeon RX 6750 XT";
        assert_eq!(resets(vec![ago(2), ago(40)], vec![ago(3), ago(5)], gpu), Some(Level::WorthDoing));
        assert_eq!(resets(vec![ago(2), ago(40)], vec![ago(45)], gpu), None, "only one in 30 days");
        let evening: Vec<String> = ["21:23", "21:24", "21:25"].iter().map(|t| format!("{} {t}", ago(2))).collect();
        assert_eq!(resets(vec![], evening, gpu), None, "one bad evening is not a pattern");
        assert_eq!(resets(vec![ago(1), ago(2), ago(3)], vec![], "Parsec Virtual Display Adapter"), None, "no real graphics card");
    }

    #[test]
    fn leftover_test_settings_in_the_boot_options() {
        let boot = |opts: &str, verifier: bool| {
            let mut s = base();
            s.system["os"]["StartOptions"] = json!(opts);
            s.system["os"]["Verifier"] = json!(verifier);
            s.system["cpu"] = json!({ "Threads": 12 });
            level_of(&evaluate(&s), "boot-test-mode")
        };
        assert_eq!(boot(" NOEXECUTE=OPTOUT  NUMPROC=12  HYPERVISORLAUNCHTYPE=AUTO  NOVGA", false), None, "Windows writes these itself");
        assert_eq!(boot(" TESTSIGNING  NOEXECUTE=OPTOUT", false), Some(Level::WorthDoing));
        assert_eq!(boot(" NOEXECUTE=OPTOUT  NUMPROC=4", false), Some(Level::WorthDoing), "a third of the processor");
        assert_eq!(boot(" NOEXECUTE=ALWAYSOFF", false), Some(Level::WorthDoing));
        assert_eq!(boot(" NOEXECUTE=OPTOUT", true), Some(Level::WorthDoing), "Driver Verifier left on");
    }

    #[test]
    fn a_missing_page_file_matters_most_with_little_memory() {
        let page = |files: serde_json::Value, ram: f64, warned: bool| {
            let mut s = base();
            let events = if warned { json!([{"Date": "2026-11-01 09:00", "Id": 46}]) } else { json!([]) };
            s.health = json!({ "perf": { "Pagefiles": files, "RamTotalGB": ram }, "dumpEvents": events });
            level_of(&evaluate(&s), "no-pagefile")
        };
        assert_eq!(page(json!([]), 8.0, false), Some(Level::WorthDoing));
        assert_eq!(page(json!([]), 32.0, true), Some(Level::WorthDoing), "Windows warned it can't save crash files");
        assert_eq!(page(json!([]), 32.0, false), Some(Level::NiceToHave));
        assert_eq!(page(json!(["C:\\pagefile.sys 4096 MB"]), 8.0, false), None);
        let mut s = base();
        s.health = json!({});
        assert_eq!(level_of(&evaluate(&s), "no-pagefile"), None, "no data, no guess");
    }

    #[test]
    fn crash_files_windows_could_not_save() {
        let ev = |list: &[(&str, i64)]| json!(list.iter().map(|(d, i)| json!({"Date": d, "Id": i})).collect::<Vec<_>>());
        let pc = |files: serde_json::Value, enabled: i64, events: serde_json::Value| {
            let mut s = base();
            s.health = json!({
                "perf": {"Pagefiles": files, "RamTotalGB": 32.0},
                "crashControl": {"Enabled": enabled, "DedicatedDumpFile": "", "SystemDrive": "C:"},
                "dumpEvents": events
            });
            s
        };
        let on_c = || json!(["C:\\pagefile.sys 4096 MB", "D:\\pagefile.sys 2048 MB"]);
        let fixes = |s: &Snapshot, id: &str| -> Vec<Fix> {
            let f = evaluate(s);
            let x = f.iter().find(|x| x.id == id).unwrap_or_else(|| panic!("{id} missing"));
            x.actions.iter().filter_map(|a| if let Action::Fix(fx) = a { Some(fx.clone()) } else { None }).collect()
        };
        let none_about_crash_files = |s: &Snapshot| evaluate(s).iter().all(|x| !x.id.starts_with("crash-files"));

        // A failure within minutes of a saved file is the same crash, partly saved.
        let s = pc(
            on_c(),
            7,
            ev(&[("2026-10-19 06:07", 162), ("2026-10-19 06:07", 161), ("2026-09-15 20:28", 161), ("2026-09-15 19:57", 162), ("2026-09-09 19:38", 161)]),
        );
        let f = evaluate(&s);
        let x = f.iter().find(|x| x.id == "crash-files-lost").expect("finding");
        assert_eq!(x.title, "Windows couldn't save the details of 2 crashes");
        assert_eq!(x.level, Level::WorthDoing);
        assert_eq!(x.details, ["2026-09-15 20:28", "2026-09-09 19:38"]);
        assert!(x.actions.contains(&Action::Goto(Page::Storage)));
        assert!(none_about_crash_files(&pc(on_c(), 7, ev(&[("2026-10-19 06:07", 162), ("2026-10-19 06:08", 161)]))), "partly saved");
        assert!(none_about_crash_files(&pc(on_c(), 7, json!([]))));

        // Switched off: worth doing once Windows has crashed.
        let mut off = pc(on_c(), 0, json!([]));
        assert_eq!(level_of(&evaluate(&off), "crash-files-off"), Some(Level::NiceToHave));
        assert_eq!(fixes(&off, "crash-files-off"), [Fix::EnableCrashFiles]);
        off.health["unexpectedShutdowns"] = json!([{"Date": "2026-11-01 10:00", "Code": "0x3B", "PowerButton": false}]);
        assert_eq!(level_of(&evaluate(&off), "crash-files-off"), Some(Level::WorthDoing));

        // Page files only on other drives leave Windows nowhere to write one.
        let d_only = pc(json!(["D:\\pagefile.sys 4096 MB"]), 7, json!([]));
        assert_eq!(level_of(&evaluate(&d_only), "crash-files-no-room"), Some(Level::NiceToHave));
        assert_eq!(fixes(&d_only, "crash-files-no-room"), [Fix::SystemManagedPagefile]);
        let warned = pc(json!(["d:\\pagefile.sys 4096 MB"]), 7, ev(&[("2026-11-01 09:00", 46)]));
        assert_eq!(level_of(&evaluate(&warned), "crash-files-no-room"), Some(Level::WorthDoing));
        let mut dedicated = pc(json!(["D:\\pagefile.sys 4096 MB"]), 7, json!([]));
        dedicated.health["crashControl"]["DedicatedDumpFile"] = json!("D:\\dedicated.sys");
        assert!(none_about_crash_files(&dedicated), "a dedicated crash file works instead");
        assert!(none_about_crash_files(&pc(json!(["c:\\pagefile.sys 4096 MB"]), 7, json!([]))), "any case");

        // No page file at all is the no-pagefile to-do's job.
        let none = pc(json!([]), 7, ev(&[("2026-11-01 09:00", 46)]));
        assert_eq!(level_of(&evaluate(&none), "no-pagefile"), Some(Level::WorthDoing));
        assert!(none_about_crash_files(&none));
    }

    #[test]
    fn browser_settings_forced_by_a_policy() {
        let mut s = base();
        s.security["browserPolicies"] = json!([{"Browser": "Chrome", "Setting": "ExtensionInstallForcelist", "Value": "abcdef;https://x.example/update"}]);
        let x = evaluate(&s).into_iter().find(|x| x.id == "browser-policies").expect("forced extension");
        assert_eq!((x.level, x.confidence), (Level::WorthDoing, Confidence::BestGuess));
        s.system["device"]["Joined"] = json!(true);
        assert_eq!(level_of(&evaluate(&s), "browser-policies"), None, "work PCs are managed by their IT");
    }

    #[test]
    fn proxies_and_dns_servers_nobody_chose() {
        let net = |n: serde_json::Value| {
            let mut s = base();
            s.network = n;
            let f = evaluate(&s);
            (level_of(&f, "proxy"), level_of(&f, "dns-changed"))
        };
        assert_eq!(net(json!({"proxy": {"Enabled": true, "Server": "45.1.2.3:8080"}})), (Some(Level::WorthDoing), None));
        assert_eq!(net(json!({"proxy": {"Enabled": true, "Server": "127.0.0.1:8888"}})), (Some(Level::NiceToHave), None), "a local tool");
        assert_eq!(net(json!({"proxy": {"Enabled": false, "Server": "45.1.2.3:8080"}})), (None, None), "saved but off");
        assert_eq!(net(json!({"proxy": {"Pac": "http://cfg.example/proxy.pac"}})), (Some(Level::WorthDoing), None));
        assert_eq!(net(json!({"proxy": {"WinHttp": "45.1.2.3:3128"}})), (Some(Level::WorthDoing), None));
        assert_eq!(net(json!({"staticDns": ["185.10.10.10"]})), (None, Some(Level::WorthDoing)));
        assert_eq!(net(json!({"staticDns": ["1.1.1.1", "192.168.1.1", "127.0.2.2", "100.100.100.100"]})), (None, None));
    }

    #[test]
    fn switched_off_windows_services() {
        let mut s = base();
        s.security["disabledServices"] = json!(["wuauserv"]);
        assert_eq!(level_of(&evaluate(&s), "services-off"), Some(Level::WorthDoing));
        s.security["disabledServices"] = json!(["WinDefend"]);
        s.security["otherAntivirus"] = json!(["Norton 360"]);
        assert_eq!(level_of(&evaluate(&s), "services-off"), None, "another antivirus has taken over");
    }

    #[test]
    fn programs_starting_from_throwaway_folders() {
        let start = |startup: serde_json::Value, tasks: serde_json::Value| {
            let mut s = base();
            s.software = json!({ "startup": startup, "tasks": tasks });
            level_of(&evaluate(&s), "startup-odd")
        };
        let temp = json!([{"Name": "upd", "Command": "C:\\Users\\a\\AppData\\Local\\Temp\\upd.exe", "Enabled": true}]);
        assert_eq!(start(temp, json!([])), Some(Level::WorthDoing));
        let discord =
            json!([{"Name": "Discord", "Command": "\"C:\\Users\\a\\AppData\\Local\\Discord\\Update.exe\" --processStart Discord.exe", "Enabled": true}]);
        assert_eq!(start(discord, json!([])), None, "apps live in AppData");
        let off = json!([{"Name": "upd", "Command": "C:\\Users\\Public\\upd.exe", "Enabled": false}]);
        assert_eq!(start(off, json!([])), None, "already switched off");
        assert_eq!(start(json!([]), json!([{"Name": "t", "Action": "powershell.exe -w hidden -enc SQBFAFgA"}])), Some(Level::WorthDoing));
        assert_eq!(start(json!([]), json!([{"Name": "t", "Action": "pwsh -NoProfile -EncodedC SQBFAFgA"}])), Some(Level::WorthDoing), "a short form");
        assert_eq!(
            start(json!([]), json!([{"Name": "backup", "Action": r"powershell.exe -ExecutionPolicy Bypass -File C:\Program Files\Tool\run.ps1"}])),
            None,
            "-ex is not -enc"
        );
        assert_eq!(start(json!([{"Name": "x", "Command": r"%TEMP%\upd.exe", "Enabled": true}]), json!([])), Some(Level::WorthDoing));
        assert_eq!(start(json!([]), json!([{"Name": "t", "Action": "mshta.exe https://x.example/a.hta"}])), Some(Level::WorthDoing));
    }

    #[test]
    fn windows_security_cloud_checks_and_scans() {
        let av = |cloud: i64, samples: i64, quick: Option<String>, full: Option<String>| {
            let mut s = base();
            s.security["defender"]["CloudProtection"] = json!(cloud);
            s.security["defender"]["SampleSubmission"] = json!(samples);
            s.security["defender"]["LastQuickScan"] = json!(quick);
            s.security["defender"]["LastFullScan"] = json!(full);
            let f = evaluate(&s);
            (level_of(&f, "av-cloud-off"), level_of(&f, "av-samples-off"), level_of(&f, "av-scan-old"))
        };
        assert_eq!(av(2, 1, Some(ago(1)), None), (None, None, None));
        assert_eq!(av(0, 1, Some(ago(1)), None), (Some(Level::WorthDoing), None, None));
        assert_eq!(av(2, 2, Some(ago(1)), None), (None, Some(Level::WorthDoing), None), "Windows Security warns about it");
        assert_eq!(av(2, 1, Some(ago(40)), Some(ago(60))), (None, None, Some(Level::NiceToHave)));
        assert_eq!(av(2, 1, None, None), (None, None, None), "no data, no guess");
        let mut s = base();
        s.security["defender"]["CloudProtection"] = json!(0);
        s.security["otherAntivirus"] = json!(["Norton 360"]);
        assert_eq!(level_of(&evaluate(&s), "av-cloud-off"), None);
    }

    #[test]
    fn paused_drive_encryption_is_not_missing_encryption() {
        let enc = |bitlocker: &str, pending: bool| {
            let mut s = base();
            s.system["device"]["HasBattery"] = json!(true);
            s.system["os"]["PendingReboot"] = json!(pending);
            s.storage = json!({"volumes": [{"Letter": "C:", "SizeGB": 476, "FreeGB": 200, "BitLocker": bitlocker}]});
            let f = evaluate(&s);
            (level_of(&f, "bitlocker-suspended"), level_of(&f, "encryption-off"))
        };
        assert_eq!(enc("Off / FullyEncrypted", false), (Some(Level::WorthDoing), None));
        assert_eq!(enc("Off / FullyEncrypted", true), (None, None), "Windows pauses it around updates");
        assert_eq!(enc("Off / FullyDecrypted", false), (None, Some(Level::WorthDoing)));
        assert_eq!(enc("On / FullyEncrypted", false), (None, None));
    }

    #[test]
    fn driver_blocklist_and_antivirus_overlap() {
        let mut s = base();
        win11(&mut s);
        s.system["os"]["DriverBlocklist"] = json!(0);
        assert_eq!(level_of(&evaluate(&s), "driver-blocklist-off"), Some(Level::WorthDoing));
        s.system["os"]["MemoryIntegrity"] = json!("On");
        assert_eq!(level_of(&evaluate(&s), "driver-blocklist-off"), Some(Level::NiceToHave), "memory integrity enforces it anyway");
        s.system["os"]["DriverBlocklist"] = json!(null);
        assert_eq!(level_of(&evaluate(&s), "driver-blocklist-off"), None, "on by default");
        let mut s = base();
        s.security["antivirus"] = json!([{"Name": "Norton 360", "On": true}, {"Name": "McAfee", "On": true}, {"Name": "Windows Defender", "On": false}]);
        assert_eq!(level_of(&evaluate(&s), "two-av"), Some(Level::WorthDoing));
        s.security["antivirus"] = json!([{"Name": "Norton 360", "On": true}, {"Name": "McAfee", "On": false}]);
        assert_eq!(level_of(&evaluate(&s), "two-av"), None);
    }

    #[test]
    fn guest_and_built_in_administrator_accounts() {
        let acc = |name: &str, rid: i64, daily: &str| {
            let mut s = base();
            s.security["accounts"] = json!([{"Name": name, "Rid": rid, "Enabled": true, "PasswordRequired": true}]);
            s.security["dailyAccount"] = json!({"Name": daily, "Admin": true, "OtherAdmins": []});
            let f = evaluate(&s);
            (level_of(&f, "guest-on"), level_of(&f, "builtin-admin-on"))
        };
        assert_eq!(acc("Guest", 501, "PC\\sam"), (Some(Level::WorthDoing), None));
        assert_eq!(acc("Administrator", 500, "PC\\sam"), (None, Some(Level::WorthDoing)));
        assert_eq!(acc("Administrator", 500, "PC\\Administrator"), (None, None), "it's the account in use");
    }

    #[test]
    fn weak_wifi_security() {
        let wifi = |w: serde_json::Value| {
            let mut s = base();
            s.network = json!({ "wifi": w });
            level_of(&evaluate(&s), "wifi-weak")
        };
        assert_eq!(wifi(json!({"SSID": "Cafe", "Auth": "Open"})), Some(Level::WorthDoing));
        assert_eq!(wifi(json!({"SSID": "Home", "Auth": "WEP"})), Some(Level::WorthDoing));
        assert_eq!(wifi(json!({"SSID": "Home", "Auth": "WPA2-Personal", "Cipher": "TKIP"})), Some(Level::WorthDoing));
        assert_eq!(wifi(json!({"SSID": "Home", "Auth": "WPA2-Personal", "Cipher": "CCMP"})), None);
        assert_eq!(wifi(json!({"Adapter": "Intel Wi-Fi 6"})), None, "not connected");
    }

    #[test]
    fn slow_cable_links_and_trim() {
        let link = |speed: &str, media: &str| {
            let mut s = base();
            s.network = json!({"adapters": [{"Device": "Realtek PCIe GbE Family Controller", "Speed": speed, "Status": "Up", "MediaType": media}]});
            level_of(&evaluate(&s), "link-slow")
        };
        assert_eq!(link("100 Mbps", "802.3"), Some(Level::NiceToHave));
        assert_eq!(link("1 Gbps", "802.3"), None);
        assert_eq!(link("100 Mbps", "Native 802.11"), None, "Wi-Fi speeds vary");
        let trim = |media: &str| {
            let mut s = base();
            s.storage = json!({"trim": {"Off": true}, "disks": [{"Number": 0, "IsSystem": true, "Media": media}]});
            level_of(&evaluate(&s), "trim-off")
        };
        assert_eq!(trim("SSD"), Some(Level::NiceToHave));
        assert_eq!(trim("HDD"), None);
    }

    #[test]
    fn new_to_dos_come_with_a_fix() {
        use crate::fixes::Fix;
        let fixes = |s: &Snapshot, id: &str| -> Vec<Fix> {
            let f = evaluate(s);
            let x = f.iter().find(|x| x.id == id).unwrap_or_else(|| panic!("{id} missing"));
            x.actions.iter().filter_map(|a| if let Action::Fix(fx) = a { Some(fx.clone()) } else { None }).collect()
        };
        let mut s = base();
        win11(&mut s);
        s.security["defender"]["CloudProtection"] = json!(0);
        s.security["defender"]["SampleSubmission"] = json!(2);
        s.security["disabledServices"] = json!(["UsoSvc", "W32Time"]);
        s.security["accounts"] = json!([{"Name": "Guest", "Rid": 501, "Enabled": true}]);
        s.security["browserPolicies"] = json!([
            {"Browser": "Chrome", "Setting": "HomepageLocation", "Value": "http://x.example", "Key": r"HKLM\SOFTWARE\Policies\Google\Chrome"},
            {"Browser": "Chrome", "Setting": "ExtensionInstallForcelist", "Value": "abc", "Key": r"HKLM\SOFTWARE\Policies\Google\Chrome"},
            {"Browser": "Edge", "Setting": "NewTabPageLocation", "Value": "http://x.example", "Key": r"HKCU\SOFTWARE\Policies\Microsoft\Edge"}
        ]);
        s.system["os"]["DriverBlocklist"] = json!(0);
        s.health = json!({"perf": {"Pagefiles": [], "RamTotalGB": 8.0}});
        s.storage = json!({
            "disks": [{"Number": 0, "Bus": "NVMe", "Media": "SSD", "IsSystem": true}],
            "volumes": [{"Letter": "C:", "Disk": 0, "BitLocker": "Off / FullyEncrypted"}],
            "trim": {"Off": true}
        });
        s.network = json!({
            "proxy": {"Enabled": true, "Server": "203.0.113.5:8080", "Pac": "http://203.0.113.5/p.pac", "WinHttp": null},
            "staticDns": ["185.10.10.10", "1.1.1.1"],
            "dnsByHand": [{"Adapter": "Wi-Fi", "Servers": ["185.10.10.10", "1.1.1.1"]}, {"Adapter": "Ethernet", "Servers": ["1.1.1.1"]}]
        });
        assert_eq!(fixes(&s, "av-cloud-off"), [Fix::EnableCloudProtection]);
        assert_eq!(fixes(&s, "av-samples-off"), [Fix::EnableSampleSubmission]);
        assert_eq!(
            fixes(&s, "services-off"),
            [
                Fix::EnableService { name: "UsoSvc".into(), display: "Update Orchestrator Service".into(), start: "Delayed".into() },
                Fix::EnableService { name: "W32Time".into(), display: "Windows Time".into(), start: "Manual".into() },
            ]
        );
        assert_eq!(fixes(&s, "guest-on"), [Fix::DisableAccount { name: "Guest".into() }]);
        assert_eq!(
            fixes(&s, "browser-policies"),
            [
                Fix::RemoveBrowserPolicies {
                    browser: "Chrome".into(),
                    key: r"HKLM\SOFTWARE\Policies\Google\Chrome".into(),
                    settings: vec!["HomepageLocation".into(), "ExtensionInstallForcelist".into()]
                },
                Fix::RemoveBrowserPolicies {
                    browser: "Edge".into(),
                    key: r"HKCU\SOFTWARE\Policies\Microsoft\Edge".into(),
                    settings: vec!["NewTabPageLocation".into()]
                },
            ]
        );
        assert_eq!(fixes(&s, "driver-blocklist-off"), [Fix::EnableDriverBlocklist]);
        assert_eq!(fixes(&s, "lsa-off"), [Fix::EnableLsaProtection]);
        assert_eq!(fixes(&s, "no-pagefile"), [Fix::SystemManagedPagefile]);
        assert_eq!(fixes(&s, "trim-off"), [Fix::EnableTrim]);
        assert_eq!(fixes(&s, "bitlocker-suspended"), [Fix::ResumeEncryption { drive: "C:".into() }]);
        assert_eq!(fixes(&s, "proxy"), [Fix::TurnOffProxy { enabled: true, pac: "http://203.0.113.5/p.pac".into(), winhttp: String::new() }]);
        assert_eq!(
            fixes(&s, "dns-changed"),
            [Fix::DnsAutomatic { adapter: "Wi-Fi".into(), servers: vec!["185.10.10.10".into(), "1.1.1.1".into()] }],
            "only the adapter with the unknown server"
        );
    }

    #[test]
    fn devices_are_sorted_by_what_is_wrong() {
        let mut s = base();
        s.system["problemDevices"] = json!([
            {"Name": "Microsoft Device Association Root Enumerator", "Class": "SoftwareDevice", "Code": 22, "Bus": "SWD"},
            {"Name": "USB stick", "Class": "DiskDrive", "Code": 47, "Bus": "USBSTOR"},
            {"Name": "Card reader", "Class": "SDHost", "Code": 14, "Bus": "PCI"},
            {"Name": "Onboard sound", "Class": "MEDIA", "Code": 29, "Bus": "PCI"}
        ]);
        let f = evaluate(&s);
        assert!(
            !f.iter().any(|x| x.id.starts_with("devices") || x.id == "drivers-chipset"),
            "switched off, ejected or waiting for a restart is not broken: {:?}",
            f.iter().map(|x| &x.id).collect::<Vec<_>>()
        );
        s.system["problemDevices"] = json!([
            {"Name": "SM Bus Controller", "Class": null, "Code": 28, "Bus": "PCI", "Cc": "0C05"},
            {"Name": "PCI Simple Communications Controller", "Class": null, "Code": 28, "Bus": "PCI", "Cc": "0780"},
            {"Name": "Unknown device", "Class": null, "Code": 28, "Bus": "ACPI"},
            {"Name": "PCI Device", "Class": null, "Code": 28, "Bus": "PCI", "Cc": "1300"},
            {"Name": "Network Controller", "Class": null, "Code": 28, "Bus": "PCI", "Cc": "0280"},
            {"Name": "Wireless headset", "Class": null, "Code": 28, "Bus": "BTHENUM"},
            {"Name": "Unknown USB Device (Device Descriptor Request Failed)", "Class": "USB", "Code": 43, "Bus": "USB"}
        ]);
        let f = evaluate(&s);
        let chipset = f.iter().find(|x| x.id == "drivers-chipset").expect("motherboard parts without a driver");
        assert_eq!((chipset.level, chipset.details.len()), (Level::WorthDoing, 4));
        let none = f.iter().find(|x| x.id == "devices-no-driver").expect("other devices without a driver");
        assert_eq!((none.level, none.details.len()), (Level::WorthDoing, 2));
        assert!(none.title.starts_with("2 devices"), "{}", none.title);
        let broken = f.iter().find(|x| x.id == "devices").expect("a driver that fails");
        assert_eq!((broken.level, broken.details.len()), (Level::WorthDoing, 1));
    }

    #[test]
    fn updates_sent_to_another_server_or_hidden_by_a_policy() {
        const KEY: &str = r"HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate";
        let blocked = |policy: serde_json::Value, joined: bool| {
            let mut s = base();
            s.system["winupdate"] = json!({ "Policy": policy });
            s.system["device"]["Joined"] = json!(joined);
            evaluate(&s).into_iter().find(|x| x.id == "updates-blocked")
        };
        let x =
            blocked(json!({"WUServer": "http://127.0.0.1:8530", "WUStatusServer": "http://127.0.0.1:8530", "UseWUServer": 1}), false).expect("sent elsewhere");
        assert_eq!(x.level, Level::WorthDoing);
        let settings = vec!["WUServer".to_string(), "WUStatusServer".into(), r"AU\UseWUServer".into()];
        assert_eq!(x.actions, [Action::Fix(Fix::RemoveUpdateBlock { key: KEY.into(), settings })]);
        assert!(x.details.iter().any(|d| d.contains("127.0.0.1")), "{:?}", x.details);
        assert!(x.summary.contains("security fixes"), "{}", x.summary);
        assert!(blocked(json!({"WUServer": "http://wsus", "UseWUServer": 0}), false).is_none(), "a server named but not used");
        assert!(blocked(json!({"WUServer": "http://wsus", "UseWUServer": 1}), true).is_none(), "work and school PCs are their IT's call");
        let x = blocked(json!({"SetDisableUXWUAccess": 1}), false).expect("hidden");
        assert_eq!(x.actions, [Action::Fix(Fix::RemoveUpdateBlock { key: KEY.into(), settings: vec!["SetDisableUXWUAccess".into()] })]);
        // Hiding the page leaves background updates running (Microsoft's own description), so it
        // mustn't claim fixes stop arriving.
        assert!(x.summary.contains("yourself") && !x.summary.contains("security fixes"), "{}", x.summary);
        assert!(blocked(json!({"SetDisableUXWUAccess": 0}), false).is_none());
        assert!(blocked(json!(null), false).is_none());
    }

    #[test]
    fn damaged_windows_files_need_a_repair() {
        let state = |v: serde_json::Value| {
            let mut s = base();
            s.health = json!({ "imageHealth": v });
            level_of(&evaluate(&s), "windows-damaged")
        };
        assert_eq!(state(json!("Repairable")), Some(Level::WorthDoing));
        assert_eq!(state(json!("NonRepairable")), Some(Level::WorthDoing));
        let repair = |v: &str| {
            let mut s = base();
            s.health = json!({ "imageHealth": v });
            evaluate(&s).into_iter().find(|f| f.id == "windows-damaged").unwrap().actions.contains(&Action::Fix(Fix::RepairWindowsFiles))
        };
        assert!(repair("Repairable"), "Bootmend can run the repair itself");
        assert!(!repair("NonRepairable"), "Windows can't repair this, so a reset is the way");
        assert_eq!(state(json!("Healthy")), None);
        assert_eq!(state(json!(null)), None, "only read with administrator rights");
    }

    #[test]
    fn intel_13th_and_14th_gen_desktops_need_the_microcode_fix() {
        let check_die = |id: serde_json::Value, name: &str, microcode: serde_json::Value| {
            let mut s = base();
            s.system["cpu"] = json!({ "Name": name, "Identifier": id, "Microcode": microcode });
            evaluate(&s).into_iter().find(|x| x.id == "intel-microcode")
        };
        // Raptor Lake's own chip (CPUID B0671), the one Intel's fix is for.
        let check = |name: &str, microcode: serde_json::Value| check_die(json!("Intel64 Family 6 Model 183 Stepping 1"), name, microcode);
        // Windows keeps an Intel revision in bytes 4 to 7 of an 8-byte value, lowest byte first.
        let rev = |r: u32| {
            let b = r.to_le_bytes();
            json!([0, 0, 0, 0, b[0], b[1], b[2], b[3]])
        };
        let x = check("13th Gen Intel(R) Core(TM) i9-13900K", rev(0x11F)).expect("before the fix");
        assert_eq!((x.level, x.confidence), (Level::DoFirst, Confidence::Measured));
        assert!(x.details.iter().any(|d| d.contains("0x11F")), "{:?}", x.details);
        assert_eq!(check("Intel(R) Core(TM) i7-14700KF", rev(0x12B)).map(|x| x.level), Some(Level::WorthDoing), "Intel's newest advice is 0x12F");
        assert!(check("Intel(R) Core(TM) i7-14700K", rev(0x12F)).is_none());
        assert!(check("Intel(R) Core(TM) i9-14900KS", rev(0x130)).is_none());
        assert_eq!(check("13th Gen Intel(R) Core(TM) i5-13400F", rev(0x104)).map(|x| x.confidence), Some(Confidence::BestGuess), "some 65 W chips");
        for name in [
            "13th Gen Intel(R) Core(TM) i7-13700H",
            "13th Gen Intel(R) Core(TM) i9-13980HX",
            "13th Gen Intel(R) Core(TM) i3-13100",
            "12th Gen Intel(R) Core(TM) i9-12900K",
            "Intel(R) Core(TM) i9-14900T",
        ] {
            assert!(check(name, rev(0x104)).is_none(), "{name} is not affected");
        }
        assert!(check("AMD Ryzen 5 3600 6-Core Processor", json!([0x35, 0x10, 0x70, 0x08])).is_none());
        assert!(check("13th Gen Intel(R) Core(TM) i9-13900K", json!(null)).is_none(), "unread: no guess");
        // Some 13th and 14th Gen i5s are Alder Lake chips under the new name. They aren't affected,
        // and their own microcode (0x35 and up) is numbered far below 0x12B.
        let i5 = "13th Gen Intel(R) Core(TM) i5-13400F";
        assert!(check_die(json!("Intel64 Family 6 Model 191 Stepping 2"), i5, rev(0x35)).is_none(), "Alder Lake chip");
        assert!(check_die(json!("Intel64 Family 6 Model 151 Stepping 2"), i5, rev(0x35)).is_none(), "Alder Lake chip");
        assert!(check_die(json!(null), i5, rev(0x104)).is_none(), "unknown chip: no guess");
        assert!(check("13th Gen Intel(R) Core(TM) i9-13900K", rev(0)).is_none(), "unread: no guess");
    }

    #[test]
    fn a_failed_memory_test_is_do_first() {
        let test = |id: i64| {
            let mut s = base();
            s.health = json!({ "memoryTest": {"Id": id, "Date": "2026-11-01 09:00"} });
            level_of(&evaluate(&s), "memory-test-failed")
        };
        assert_eq!(test(1202), Some(Level::DoFirst));
        assert_eq!(test(1102), Some(Level::DoFirst));
        assert_eq!(test(1201), None, "the latest test passed");
        assert_eq!(test(1101), None);
    }

    #[test]
    fn running_low_on_memory_more_than_once() {
        let low = |n: usize| {
            let mut s = base();
            let events: Vec<_> = (0..n).map(|i| json!({"Date": format!("2026-11-0{} 10:00", i + 1), "Apps": ["chrome.exe", "Teams.exe"]})).collect();
            s.health = json!({ "lowMemory": events });
            evaluate(&s).into_iter().find(|x| x.id == "low-memory")
        };
        assert!(low(1).is_none(), "once can be a one-off");
        let x = low(3).expect("a pattern");
        assert_eq!(x.level, Level::WorthDoing);
        assert!(x.title.contains("3 times"), "{}", x.title);
        assert!(x.details.iter().any(|d| d.contains("Chrome")), "{:?}", x.details);
    }

    #[test]
    fn apps_that_keep_freezing() {
        let mut s = base();
        s.health = json!({ "appHangs": [{"App": "WhatsApp.Root.exe", "Name": "WhatsApp", "Count": 6}, {"App": "x.exe", "Count": 4}] });
        let f = evaluate(&s);
        let x = f.iter().find(|x| x.id == "appfreeze-WhatsApp.Root.exe").expect("six freezes");
        assert_eq!(x.level, Level::WorthDoing);
        assert!(x.title.starts_with("WhatsApp froze 6 times"), "{}", x.title);
        assert!(!f.iter().any(|x| x.id == "appfreeze-x.exe"), "four is under the bar");
    }

    #[test]
    fn windows_recovery_switched_off() {
        let re = |state: serde_json::Value| {
            let mut s = base();
            s.system["recovery"] = json!({ "InstallState": state });
            evaluate(&s).into_iter().find(|x| x.id == "recovery-off")
        };
        let x = re(json!(0)).expect("switched off");
        assert_eq!((x.level, x.confidence), (Level::WorthDoing, Confidence::BestGuess));
        assert!(x.actions.contains(&Action::Fix(Fix::EnableRecovery)));
        assert!(re(json!(1)).is_none());
        assert!(re(json!(null)).is_none(), "no status file: no guess");
    }

    #[test]
    fn drive_firmware_with_known_faults() {
        let fw = |model: &str, firmware: &str, size: f64, build: &str| {
            let mut s = base();
            s.system["os"]["Build"] = json!(build);
            s.storage = json!({ "disks": [{"Number": 1, "Model": model, "Firmware": firmware, "SizeGB": size}] });
            evaluate(&s).into_iter().find(|x| x.id == "ssd-firmware-1")
        };
        let x = fw("Samsung SSD 990 PRO 2TB", "0B2QJXD7", 1863.0, "26100.1").expect("wears out fast");
        assert_eq!((x.level, x.confidence), (Level::DoFirst, Confidence::Measured));
        assert!(fw("Samsung SSD 990 PRO 2TB", "1B2QJXD7", 1863.0, "26100.1").is_none());
        assert_eq!(fw("Samsung SSD 980 PRO 1TB", "3B2QGXA7", 931.5, "19045.1").map(|x| x.level), Some(Level::DoFirst));
        assert!(fw("Samsung SSD 980 PRO 1TB", "5B2QGXA7", 931.5, "19045.1").is_none());
        let x = fw("WD_BLACK SN770 2TB", "731100WD", 1863.0, "26100.2033").expect("blue screens on 24H2");
        assert_eq!((x.level, x.confidence), (Level::WorthDoing, Confidence::BestGuess));
        assert!(fw("WD_BLACK SN770 2TB", "731130WD", 1863.0, "26100.1").is_none());
        assert!(fw("WD_BLACK SN770 1TB", "731100WD", 931.5, "26100.1").is_none(), "only the 2 TB drives");
        assert!(fw("WD_BLACK SN770 2TB", "731100WD", 1863.0, "22631.4000").is_none(), "only on 24H2 and later");
        assert!(fw("WD Blue SN580 2TB", "281040WD", 1863.0, "26200.1").is_some());
        assert!(fw("WD Blue SN5000 2TB", "291010WD", 1863.0, "26200.1").is_some());
        assert!(fw("WD Blue SN5000 2TB", "291020WD", 1863.0, "26200.1").is_none());
    }

    /// Bootmend copies the person's folders off a failing drive only when they're on it.
    /// Blue screens on these dates, and a graphics driver and its sound part updated together.
    fn crashed_after_update(crashes: &[&str]) -> Snapshot {
        let mut s = base();
        let gpu = r"PCI\VEN_1002&DEV_73DF\6&15bb039d&0&00000019";
        s.health = json!({
            "bugchecks": crashes.iter().map(|d| json!({"Date": d, "Text": "The bugcheck was: 0x0000003b (0x0, 0x0)."})).collect::<Vec<_>>(),
            "driverUpdates": [
                {"Device": gpu, "Name": "AMD Radeon RX 6750 XT", "Class": "Display", "Image": "amdkmdag.sys", "Version": "32.0.21045.20006", "Before": "32.0.21045.16010", "Date": "2026-11-05 10:34"},
                {"Device": r"HDAUDIO\FUNC_01&VEN_1002\7&1b7372a&0&0001", "Name": "AMD High Definition Audio Device", "Class": "MEDIA", "Image": "AtihdWT6.sys", "Version": "10.0.1.43", "Before": "10.0.1.42", "Date": "2026-11-05 10:34"}
            ]
        });
        s
    }

    fn rollback(s: &Snapshot) -> Option<(Fix, Confidence)> {
        let f = evaluate(s).into_iter().find(|f| f.id == "bsod")?;
        f.actions.iter().find_map(|a| match a {
            Action::Fix(x @ Fix::RollBackDriver { .. }) => Some((x.clone(), f.confidence)),
            _ => None,
        })
    }

    fn gpu_rollback() -> Fix {
        Fix::RollBackDriver { device: r"PCI\VEN_1002&DEV_73DF\6&15bb039d&0&00000019".into(), name: "AMD Radeon RX 6750 XT".into() }
    }

    fn dump(image: &str, date: &str) -> crate::snapshot::DumpAnalysis {
        crate::snapshot::DumpAnalysis { image: Some(image.into()), date: date.into(), code: "0x3B".into(), ..Default::default() }
    }

    #[test]
    fn crash_files_naming_a_freshly_updated_driver_offer_the_version_before() {
        let mut s = crashed_after_update(&["2026-11-20 21:00", "2026-11-21 22:00"]);
        s.dumps = vec![dump("amdkmdag.sys", "2026-11-20 21:01")];
        assert_eq!(rollback(&s), Some((gpu_rollback(), Confidence::Measured)), "measured even though the crashes began two weeks later");
    }

    #[test]
    fn blue_screens_soon_after_an_update_offer_the_graphics_driver_as_a_best_guess() {
        let s = crashed_after_update(&["2026-11-07 09:00", "2026-11-08 18:30", "2026-11-12 20:00"]);
        assert_eq!(rollback(&s), Some((gpu_rollback(), Confidence::BestGuess)), "the graphics driver wins over its sound part");
    }

    #[test]
    fn no_rollback_without_a_clear_link_to_the_update() {
        // Crashing before the update too: the old version crashed as well.
        assert_eq!(rollback(&crashed_after_update(&["2026-10-20 10:00", "2026-11-07 09:00"])), None);
        // First crash more than a week after it.
        assert_eq!(rollback(&crashed_after_update(&["2026-11-20 10:00"])), None);
        // The crash files name a driver that crashed before its update as well.
        let mut s = crashed_after_update(&["2026-11-20 21:00"]);
        s.dumps = vec![dump("amdkmdag.sys", "2026-11-01 08:00"), dump("amdkmdag.sys", "2026-11-20 21:01")];
        assert_eq!(rollback(&s), None);
        // The last change went down a version: that was a rollback already.
        let mut s = crashed_after_update(&["2026-11-07 09:00"]);
        s.health["driverUpdates"].as_array_mut().unwrap().truncate(1);
        s.health["driverUpdates"][0]["Before"] = json!("32.0.21045.30000");
        assert_eq!(rollback(&s), None);
        // Two updates nothing tells apart.
        let mut s = crashed_after_update(&["2026-11-07 09:00"]);
        s.health["driverUpdates"][0]["Class"] = json!("Net");
        assert_eq!(rollback(&s), None);
    }

    #[test]
    fn big_counts_are_grouped_in_thousands() {
        assert_eq!(plural(1, "file", "files"), "1 file");
        assert_eq!(plural(999, "file", "files"), "999 files");
        assert_eq!(plural(1_000, "file", "files"), "1,000 files");
        assert_eq!(plural(14_230, "file", "files"), "14,230 files");
        assert_eq!(plural(1_234_567, "file", "files"), "1,234,567 files");
    }

    #[test]
    fn a_failing_drive_with_your_folders_offers_to_copy_them() {
        let copy = |failing: i64, folders: serde_json::Value| {
            let mut s = base();
            s.storage = json!({
                "disks": [{"Number": 0, "Model": "Inside", "Health": "Healthy"}, {"Number": 1, "Model": "Second", "Health": "Healthy"}],
                "volumes": [{"Letter": "C:", "Disk": 0}, {"Letter": "D:", "Disk": 1}],
                "folderDrives": folders
            });
            s.storage["disks"][failing as usize]["Health"] = json!("Warning");
            s.drives = crate::collect::smart::drive_health(&s.storage);
            let f = evaluate(&s);
            let x = f.iter().find(|x| x.id == format!("drive-failing-{failing}")).expect("a failing drive");
            x.actions.contains(&Action::CopyFiles)
        };
        assert!(copy(0, json!(["C:"])));
        assert!(!copy(1, json!(["C:"])), "a second drive keeps the plain advice");
        assert!(copy(1, json!(["C:", "D:"])), "folders moved to the second drive");
        assert!(!copy(0, json!(null)), "an older check didn't note where the folders are");
    }

    #[test]
    fn mx500_pending_blip_is_not_a_failing_drive() {
        // Crucial MX500s flip SMART 197 between 0 and 1 by themselves (smartmontools ticket #1227).
        let drive = |model: &str, health: &str, attrs: &[(u8, u64)]| {
            let mut table = vec![0u8; 512];
            for (i, (id, raw)) in attrs.iter().enumerate() {
                let e = 2 + i * 12;
                table[e] = *id;
                table[e + 5..e + 11].copy_from_slice(&raw.to_le_bytes()[..6]);
            }
            let mut s = base();
            s.storage = json!({
                "disks": [{"Number": 0, "Model": model, "Bus": "SATA", "Media": "SSD", "Health": health}],
                "ataSmart": [{"Instance": format!(r"SCSI\Disk&Ven_&Prod_{model}\4&1"), "Data": table}]
            });
            s.drives = crate::collect::smart::drive_health(&s.storage);
            let f = evaluate(&s);
            (level_of(&f, "drive-failing-0"), s.drives[0].pending)
        };
        // Cleared at the source, so the "what changed" list doesn't report 0 to 1 either.
        assert_eq!(drive("CT1000MX500SSD1", "Healthy", &[(197, 1)]), (None, None));
        let (failing, _) = drive("CT500MX500SSD1N", "Healthy", &[(5, 3), (197, 1)]);
        assert_eq!(failing, None, "retired blocks keep their own Worth doing to-do");
        assert_eq!(drive("CT1000MX500SSD1", "Healthy", &[(197, 2)]), (Some(Level::DoFirst), Some(2.0)));
        assert_eq!(drive("CT1000MX500SSD1", "Healthy", &[(197, 1), (198, 1)]).0, Some(Level::DoFirst));
        assert_eq!(drive("CT1000MX500SSD1", "Healthy", &[(187, 1), (197, 1)]).0, Some(Level::DoFirst));
        assert_eq!(drive("CT1000MX500SSD1", "Warning", &[(197, 1)]).0, Some(Level::DoFirst));
        assert_eq!(drive("CT1000BX500SSD1", "Healthy", &[(197, 1)]).0, Some(Level::DoFirst), "only the MX500 is known to do this");
    }

    #[test]
    fn a_temporary_profile_is_do_first() {
        let p = |temp: bool| {
            let mut s = base();
            s.system["profile"] = json!({ "Temporary": temp });
            level_of(&evaluate(&s), "temp-profile")
        };
        assert_eq!(p(true), Some(Level::DoFirst));
        assert_eq!(p(false), None);
    }
}
