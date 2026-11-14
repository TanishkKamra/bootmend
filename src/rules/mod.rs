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
        assert_eq!(reboot(None), Some(Level::NiceToHave), "unknown start: no louder than before");
    }
}
