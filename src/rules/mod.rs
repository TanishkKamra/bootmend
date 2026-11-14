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
    Open { label: &'static str, target: String },
    Goto(Page),
    AnalyzeDumps,
    Elevate,
    RestorePoint,
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
    pub fn open(self, label: &'static str, target: &str) -> Self {
        self.act(Action::Open { label, target: target.to_string() })
    }
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
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
}
