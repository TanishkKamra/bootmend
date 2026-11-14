use super::data::{BAD_EXTENSIONS, REMOTE_TOOLS, VULNERABLE_APPS};
use super::{plural, Action, Area, Confidence, Finding, Level};
use crate::fixes::{ExclusionKind, Fix};
use crate::gui::Page;
use crate::snapshot::Snapshot;
use crate::util::{version_lt, J};

const SEC: Area = Area::Security;

pub fn check(s: &Snapshot, f: &mut Vec<Finding>) {
    defender(s, f);
    firewall(s, f);
    access(s, f);
    extensions(s, f);
    apps(s, f);
    system(s, f);
    device_access(s, f);
}

/// One app's most recent use of the camera, microphone or location, as Windows records it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceUse {
    pub what: &'static str,
    pub app: String,
    pub path: Option<String>,
    pub last_used: String,
    pub in_use: bool,
    /// Part of Windows itself, such as the location service.
    pub windows: bool,
}

/// A readable name for a program from Windows' camera and microphone records:
/// Store app ids and game build suffixes become the names people know.
pub fn friendly_app(raw: &str) -> String {
    let known = [
        ("immersivecontrolpanel", "Windows Settings"),
        ("WindowsCamera", "Camera app"),
        ("WindowsSoundRecorder", "Sound Recorder"),
        ("WhatsAppDesktop", "WhatsApp"),
        ("slackdesktop", "Slack"),
        ("CBS", "Windows search and Start"),
        ("Win32WebViewHost", "Windows web view"),
    ];
    if let Some((_, nice)) = known.iter().find(|(k, _)| k.eq_ignore_ascii_case(raw)) {
        return nice.to_string();
    }
    let mut name = raw;
    for suffix in ["-Win64-Shipping", "_x64_rwdi", "x64vk", "_dx12", "_dx11", "-Win64", "_x64"] {
        name = name.strip_suffix(suffix).unwrap_or(name);
    }
    name.to_string()
}

/// Camera, microphone and location use, newest first, one row per app and device.
pub fn device_use(s: &Snapshot) -> Vec<DeviceUse> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into()).to_lowercase();
    let mut out: Vec<DeviceUse> = vec![];
    for d in s.security.a("deviceAccess") {
        let what = match d.s("Kind").as_deref() {
            Some("webcam") => "Camera",
            Some("microphone") => "Microphone",
            Some("location") => "Location",
            _ => continue,
        };
        let Some(app) = d.s("App").filter(|a| !a.is_empty()).map(|a| friendly_app(&a)) else { continue };
        let path = d.s("Path");
        let windows = path.as_deref().is_some_and(|p| p.to_lowercase().starts_with(&windir));
        let row = DeviceUse { what, app, path, last_used: d.s("LastUsed").unwrap_or_default(), in_use: d.b("InUse") == Some(true), windows };
        match out.iter_mut().find(|x| x.what == row.what && x.app.eq_ignore_ascii_case(&row.app)) {
            Some(x) if row.last_used > x.last_used => *x = row,
            Some(_) => {}
            None => out.push(row),
        }
    }
    out.sort_by(|a, b| b.last_used.cmp(&a.last_used));
    out
}

/// Programs run from Downloads or a temporary folder rarely have a good reason to use the camera or microphone.
fn device_access(s: &Snapshot, f: &mut Vec<Finding>) {
    let odd: Vec<DeviceUse> = device_use(s)
        .into_iter()
        .filter(|d| d.what != "Location")
        .filter(|d| d.path.as_deref().is_some_and(|p| ["\\downloads\\", "\\temp\\", "\\tmp\\"].iter().any(|w| p.to_lowercase().contains(w))))
        .collect();
    if odd.is_empty() {
        return;
    }
    f.push(
        Finding::new("device-odd-app", SEC, Level::WorthDoing, format!("{} from an unusual folder used your camera or microphone", plural(odd.len(), "program", "programs")))
            .why("Apps you install normally live in Program Files. A program run straight from Downloads or a temporary folder that turns on the camera or microphone deserves a second look.")
            .step("If you don't recognise it, turn off its access in Settings, then delete the file and run a quick virus check.")
            .details(odd.iter().map(|d| format!("{} used the {} on {}: {}", d.app, d.what.to_lowercase(), d.last_used, d.path.clone().unwrap_or_default())))
            .open("Open camera settings", "ms-settings:privacy-webcam")
            .open("Open microphone settings", "ms-settings:privacy-microphone")
            .act(Action::Goto(Page::Security)),
    );
}

fn defender(s: &Snapshot, f: &mut Vec<Finding>) {
    let d = s.security.g("defender");
    let others = s.security.strs("otherAntivirus");
    if d.is_null() {
        return;
    }
    let realtime = d.b("RealTime") == Some(true) && d.b("AntivirusEnabled") != Some(false);
    if !realtime && others.is_empty() {
        f.push(
            Finding::new("av-off", SEC, Level::DoFirst, "Virus protection is switched off")
                .why("Nothing is checking files as they arrive, so a harmful download could run without any warning.")
                .step("Open Windows Security, choose Virus & threat protection, then turn on Real-time protection.")
                .open("Open Windows Security", "windowsdefender://threat"),
        );
    } else if !realtime {
        f.push(
            Finding::new("av-other", SEC, Level::AllGood, format!("{} is protecting this PC", others.join(" and ")))
                .why("Windows Security steps aside when another antivirus is installed. That is normal."),
        );
    }
    if realtime {
        if let Some(age) = d.i("SignatureAgeDays").filter(|a| *a >= 3) {
            // Windows reports 65535 days when the definitions have never been updated.
            let title = if age >= 65535 { "Virus definitions have never been updated".to_string() } else { format!("Virus definitions are {age} days old") };
            f.push(
                Finding::new("av-stale", SEC, Level::WorthDoing, title)
                    .why("Windows Security learns about new threats through daily updates. Old definitions miss recent ones.")
                    .step("Open Windows Update and check for updates. Definitions come through there.")
                    .open("Open Windows Update", "ms-settings:windowsupdate"),
            );
        }
        if d.b("Tamper") == Some(false) {
            f.push(
                Finding::new("av-tamper", SEC, Level::WorthDoing, "Tamper protection is off")
                    .why("Tamper protection stops other programs from quietly switching off your virus protection. Harmful apps often try exactly that.")
                    .step("Open Windows Security, go to Virus & threat protection settings, and turn on Tamper Protection.")
                    .open("Open Windows Security", "windowsdefender://threatsettings"),
            );
        }
        if d.i("PUAProtection") == Some(0) {
            f.push(
                Finding::new("av-pua", SEC, Level::WorthDoing, "Adware and bundled extras are not blocked")
                    .why("Windows Security can block apps that are not viruses but come with adware, browser toolbars or crypto miners. That setting is off.")
                    .act(Action::Fix(Fix::EnablePua)),
            );
        }
    }

    let paths = d.strs("ExclusionPaths");
    if paths.iter().any(|p| p.starts_with("N/A")) {
        if !s.elevated {
            f.push(
                Finding::new("av-exclusions-hidden", SEC, Level::NiceToHave, "Windows Security's skip list is hidden")
                    .why("Windows only shows which folders virus protection skips to administrators. Restart Bootmend as administrator to check it.")
                    .act(Action::Elevate),
            );
        }
    } else {
        let procs = d.strs("ExclusionProcesses");
        let exts = d.strs("ExclusionExtensions");
        let whole: Vec<&String> = paths.iter().filter(|p| p.trim_end_matches('\\').len() <= 2 && p.contains(':')).collect();
        let total = paths.len() + procs.len() + exts.len();
        if total > 0 {
            let level = if !whole.is_empty() { Level::DoFirst } else { Level::WorthDoing };
            let title = if !whole.is_empty() {
                format!("Virus protection skips all of drive {}", whole.iter().map(|w| w.trim_end_matches('\\').to_string()).collect::<Vec<_>>().join(" and "))
            } else {
                format!("Virus protection skips {}", plural(total, "location", "locations"))
            };
            let mut x = Finding::new("defender-exclusions", SEC, level, title)
                .why("Anything inside a skipped location is never checked, even when you open it. Games and developer tools sometimes add these, and so does harmful software trying to hide.")
                .step("Remove any entry you don't recognise. If an app you trust stops working, add just its own folder back.")
                .details(paths.iter().map(|p| format!("Folder: {p}")))
                .details(procs.iter().map(|p| format!("Program: {p}")))
                .details(exts.iter().map(|p| format!("File type: {p}")));
            for p in &paths {
                x = x.act(Action::Fix(Fix::RemoveExclusion { kind: ExclusionKind::Path, value: p.clone() }));
            }
            for p in &procs {
                x = x.act(Action::Fix(Fix::RemoveExclusion { kind: ExclusionKind::Process, value: p.clone() }));
            }
            for p in &exts {
                x = x.act(Action::Fix(Fix::RemoveExclusion { kind: ExclusionKind::Extension, value: p.clone() }));
            }
            let events = s.security.a("exclusionEvents");
            if !events.is_empty() {
                x = x.detail(format!("Last changed {}", events[0].s("Date").unwrap_or_default()));
            }
            f.push(x.act(Action::Goto(Page::Security)));
        }
    }

    let threats = s.security.a("threats");
    let unresolved: Vec<_> = threats.iter().filter(|t| t.b("ActionSuccess") == Some(false)).collect();
    if !unresolved.is_empty() {
        f.push(
            Finding::new(
                "threats-open",
                SEC,
                Level::DoFirst,
                format!("Windows Security found {} it could not remove", plural(unresolved.len(), "item", "items")),
            )
            .why("These were detected but are still waiting for you to choose what to do.")
            .step("Open Protection history in Windows Security and choose Remove or Quarantine for each one.")
            .details(unresolved.iter().map(|t| format!("{} ({})", t.s("Name").unwrap_or_default(), t.s("Date").unwrap_or_default())))
            .open("Open protection history", "windowsdefender://history"),
        );
    } else if let Some(t) = threats.first() {
        f.push(
            Finding::new("threats-handled", SEC, Level::AllGood, format!("Windows Security dealt with {} recently", plural(threats.len(), "item", "items")))
                .why("Each one was removed or quarantined. It's worth a glance to see where they came from, so you can avoid that source.")
                .detail(format!("Most recent: {} on {}", t.s("Name").unwrap_or_default(), t.s("Date").unwrap_or_default()))
                .open("Open protection history", "windowsdefender://history"),
        );
    }
}

fn firewall(s: &Snapshot, f: &mut Vec<Finding>) {
    let off: Vec<String> = s.security.a("firewallProfiles").into_iter().filter(|p| p.b("Enabled") == Some(false)).filter_map(|p| p.s("Name")).collect();
    if !off.is_empty() {
        f.push(
            Finding::new("firewall-off", SEC, Level::DoFirst, format!("The firewall is off for {} networks", off.join(" and ")))
                .why("The firewall stops other devices on the network from reaching apps on this PC that were never meant to be reachable.")
                .step("Open Windows Security, choose Firewall & network protection, and turn it on for each network type.")
                .open("Open firewall settings", "windowsdefender://network"),
        );
    }
    let stale: Vec<_> = s.security.a("firewallRules").into_iter().filter(|r| r.b("ProgramExists") == Some(false)).collect();
    if !stale.is_empty() {
        let mut x = Finding::new("firewall-stale", SEC, Level::NiceToHave, format!("{} for apps that are gone", plural(stale.len(), "firewall opening remains", "firewall openings remain")))
            .why("These rules let network traffic in for programs that are no longer installed. They do nothing useful now, and switching them off is tidy and safe.")
            .details(stale.iter().map(|r| r.s("DisplayName").unwrap_or_default()));
        for r in stale.iter().take(12) {
            if let (Some(n), Some(d)) = (r.s("Name"), r.s("DisplayName")) {
                x = x.act(Action::Fix(Fix::DisableFirewallRule { name: n, display: d }));
            }
        }
        f.push(x);
    }
}

fn access(s: &Snapshot, f: &mut Vec<Finding>) {
    let a = s.security.g("access");
    if a.b("UAC") == Some(false) {
        f.push(
            Finding::new("uac-off", SEC, Level::DoFirst, "Apps can make system changes without asking")
                .why("User Account Control is off, so any program can change Windows without the usual \"Do you want to allow this app\" prompt.")
                .step("Search Start for \"Change User Account Control settings\" and move the slider to the default level, then restart.")
                .open("Open UAC settings", "UserAccountControlSettings.exe"),
        );
    }
    if a.b("RemoteDesktop") == Some(true) {
        f.push(
            Finding::new("rdp-on", SEC, Level::WorthDoing, "Remote Desktop is turned on")
                .why("Other computers can sign in to this one over the network. If you don't use it, turning it off removes a common target.")
                .open("Open Remote Desktop settings", "ms-settings:remotedesktop"),
        );
    }
    if a.b("RemoteAssistance") == Some(true) {
        f.push(
            Finding::new("ra-on", SEC, Level::NiceToHave, "Remote Assistance invitations are allowed")
                .why("Remote Assistance lets someone you invite see and control your screen. Scammers who phone pretending to be tech support often ask people to use it.")
                .act(Action::Fix(Fix::DisableRemoteAssistance)),
        );
    }
    if a.b("SMB1") == Some(true) {
        f.push(
            Finding::new("smb1-on", SEC, Level::WorthDoing, "An outdated file-sharing protocol is on")
                .why("SMB1 is from the 1990s and was how the WannaCry outbreak spread. Modern PCs and network drives use newer versions.")
                .act(Action::Fix(Fix::DisableSmb1)),
        );
    }
    if a.s("SmartScreen").is_some_and(|v| v.eq_ignore_ascii_case("off")) {
        f.push(
            Finding::new("smartscreen-off", SEC, Level::WorthDoing, "Download warnings are switched off")
                .why("SmartScreen warns you before running apps and files that are known to be harmful or that very few people have downloaded.")
                .open("Open app & browser control", "windowsdefender://appbrowser"),
        );
    }
    if a.b("AutoLogon") == Some(true) {
        f.push(
            Finding::new("autologon", SEC, Level::WorthDoing, "Windows signs in without a password")
                .why("Anyone who switches this PC on gets straight into your account, along with your email, browser and files.")
                .step("Search Start for \"netplwiz\", tick \"Users must enter a user name and password\", and confirm."),
        );
    }
    if let Some(crd) = a.s("ChromeRemoteDesktop").filter(|c| c.starts_with("Running")) {
        f.push(
            Finding::new("crd", SEC, Level::WorthDoing, "Chrome Remote Desktop is set up")
                .why("This PC can be controlled from another device that knows the PIN. That's fine if you set it up yourself. If you didn't, remove it from Apps.")
                .detail(format!("Service: {crd}"))
                .open("Open installed apps", "ms-settings:appsfeatures"),
        );
    }
    let mut remote: Vec<&str> = s
        .network
        .a("listening")
        .into_iter()
        .filter_map(|l| l.s("Process"))
        .chain(s.software.a("services").into_iter().filter_map(|x| x.s("Name")))
        .filter_map(|p| REMOTE_TOOLS.iter().find(|(k, _)| p.to_lowercase().contains(k)).map(|(_, n)| *n))
        .collect();
    remote.sort();
    remote.dedup();
    remote.retain(|n| *n != "Chrome Remote Desktop");
    if !remote.is_empty() {
        f.push(
            Finding::new("remote-tools", SEC, Level::WorthDoing, format!("Remote control app found: {}", remote.join(", ")))
                .why("These apps let someone else control this PC. They are useful for helping family or for work. If you didn't install one, or you were asked to by a caller, uninstall it.")
                .open("Open installed apps", "ms-settings:appsfeatures"),
        );
    }
    let accounts = s.security.a("accounts");
    let no_pw: Vec<String> = accounts
        .iter()
        .filter(|a| a.b("Enabled") == Some(true) && a.b("PasswordRequired") == Some(false) && a.s("Source").as_deref() == Some("Local"))
        .filter_map(|a| a.s("Name"))
        .collect();
    if !no_pw.is_empty() {
        f.push(
            Finding::new("no-password", SEC, Level::WorthDoing, format!("{} can be set up without a password", plural(no_pw.len(), "account", "accounts")))
                .why("Windows allows these local accounts to have an empty password. If any of them really has none, anyone at the keyboard can use it.")
                .details(no_pw)
                .conf(Confidence::BestGuess)
                .open("Open sign-in options", "ms-settings:signinoptions"),
        );
    }
    let hosts = s.security.g("hosts");
    let hijack: Vec<String> = hosts
        .strs("Sample")
        .into_iter()
        .filter(|l| {
            let l = l.to_lowercase();
            ["google.", "microsoft.", "windowsupdate", "bing.", "facebook.", "paypal.", "apple."].iter().any(|d| l.contains(d))
                && !l.starts_with("0.0.0.0")
                && !l.starts_with("127.")
        })
        .collect();
    if !hijack.is_empty() {
        f.push(
            Finding::new("hosts-redirect", SEC, Level::WorthDoing, "Some well-known websites are redirected on this PC")
                .why("The Windows hosts file sends these addresses somewhere other than their real servers. Harmful software sometimes does this to fake login pages.")
                .details(hijack)
                .conf(Confidence::BestGuess),
        );
    }
    let unsigned: Vec<String> = s
        .security
        .a("kernelDrivers")
        .into_iter()
        .filter(|k| k.s("Signer").is_some_and(|x| x.starts_with("UNSIGNED")))
        .filter_map(|k| k.s("Display").or(k.s("Name")))
        .collect();
    if !unsigned.is_empty() {
        f.push(
            Finding::new(
                "drivers-unsigned",
                SEC,
                Level::WorthDoing,
                format!("{} running without a valid signature", plural(unsigned.len(), "driver is", "drivers are")),
            )
            .why("Drivers run with full control of Windows. Legitimate ones carry a digital signature from their maker. A missing one deserves a closer look.")
            .details(unsigned),
        );
    }
}

fn extensions(s: &Snapshot, f: &mut Vec<Finding>) {
    for e in &s.extensions {
        let lname = e.name.to_lowercase();
        if let Some(b) = BAD_EXTENSIONS.iter().find(|b| b.id == Some(e.id.as_str()) || lname.contains(b.name_contains)) {
            f.push(
                Finding::new(format!("ext-bad-{}", e.id), SEC, Level::DoFirst, format!("Remove the \"{}\" extension from {}", e.name, e.browser))
                    .why(b.why)
                    .step(format!("In {}, open the extensions page, find {}, and choose Remove.", e.browser, e.name))
                    .step("If you shared private details in AI chats while it was installed, treat them as seen by others. Change any passwords you pasted there.")
                    .detail(format!("Source: {}", b.source))
                    .detail(format!("Profile: {}  ID: {}", e.profile, e.id)),
            );
        }
    }
    let outside: Vec<_> = s.extensions.iter().filter(|e| !e.from_store && e.enabled && !BAD_EXTENSIONS.iter().any(|b| b.id == Some(e.id.as_str()))).collect();
    if !outside.is_empty() {
        f.push(
            Finding::new("ext-outside", SEC, Level::WorthDoing, format!("{} came from outside the browser's store", plural(outside.len(), "extension", "extensions")))
                .why("Store extensions are reviewed and update automatically. Ones added another way are sometimes installed by other software without asking. Keep the ones you recognise.")
                .details(outside.iter().map(|e| format!("{} ({}, {})", e.name, e.browser, e.profile)))
                .act(Action::Goto(Page::Software)),
        );
    }
    let broad = s.extensions.iter().filter(|e| e.all_sites && e.enabled).count();
    if broad >= 5 {
        f.push(
            Finding::new("ext-broad", SEC, Level::NiceToHave, format!("{broad} extensions can read every website you visit"))
                .why("That access is normal for ad blockers and password managers, but each one is a way in if its maker sells it or gets hacked. Remove the ones you no longer use.")
                .act(Action::Goto(Page::Software)),
        );
    }
}

fn apps(s: &Snapshot, f: &mut Vec<Finding>) {
    for a in s.software.a("apps") {
        let (Some(name), Some(ver)) = (a.s("Name"), a.s("Version")) else { continue };
        if let Some(v) = VULNERABLE_APPS.iter().find(|v| name.starts_with(v.name_starts) && version_lt(&ver, v.fixed_in)) {
            f.push(
                Finding::new(format!("vuln-{}", v.name_starts), SEC, Level::DoFirst, format!("Update {} to version {} or newer", v.name_starts, v.fixed_in))
                    .why(v.why)
                    .step("Download the latest version from the official site and install it over the old one. Your settings are kept.")
                    .detail(format!("Installed: {name} ({ver})"))
                    .detail(format!("Flaw: {}, fixed in {}", v.cve, v.fixed_in))
                    .open("Open the official download page", v.download),
            );
        }
    }
    if let Some(up) = &s.upgrades {
        if !up.is_empty() {
            f.push(
                Finding::new("app-updates", Area::Speed, Level::WorthDoing, format!("{} have updates waiting", plural(up.len(), "app", "apps")))
                    .why("Updates fix security holes and bugs. You can update them one by one from the Apps page.")
                    .details(up.iter().map(|u| format!("{}: {} to {}", u.name, u.installed, u.available)))
                    .act(Action::Goto(Page::Software)),
            );
        }
    }
}

fn system(s: &Snapshot, f: &mut Vec<Finding>) {
    let dev = s.system.g("device");
    if dev.s("FirmwareMode").as_deref() == Some("UEFI") && dev.b("SecureBoot") == Some(false) {
        f.push(
            Finding::new("secureboot-off", SEC, Level::WorthDoing, "Secure Boot is off")
                .why("Secure Boot makes sure only trusted software loads before Windows starts, which blocks a rare but nasty kind of malware. Some games' anti-cheat also requires it.")
                .step("Turning it on happens in the PC's firmware settings (BIOS). Look up your motherboard or laptop model for the exact steps.")
                .conf(Confidence::Measured),
        );
    }
    let os = s.system.g("os");
    match os.s("MemoryIntegrity").as_deref() {
        // A real gap only where the hardware can run it; elsewhere it's an extra at best.
        Some("Off") if os.b("VbsAvailable") == Some(true) => f.push(
            Finding::new("hvci-off", SEC, Level::WorthDoing, "Memory integrity is off")
                .why("This Windows feature stops harmful code from slipping into the heart of Windows. Older drivers sometimes block it, and Windows will tell you which.")
                .open("Open Core isolation", "windowsdefender://coreisolation"),
        ),
        Some("Off") => f.push(
            Finding::new("hvci-off", SEC, Level::NiceToHave, "Memory integrity is off")
                .why("This Windows feature stops harmful code from slipping into the heart of Windows. It needs virtualization, which looks switched off in the firmware settings (BIOS) or isn't supported by this processor.")
                .conf(Confidence::BestGuess)
                .open("Open Core isolation", "windowsdefender://coreisolation"),
        ),
        Some("Enabled, not running") => f.push(
            Finding::new("hvci-off", SEC, Level::NiceToHave, "Memory integrity is on but not running")
                .why("It's switched on, but Windows couldn't start it. A restart usually finishes the job. If it stays off, an older driver is in the way, and Windows Security names it.")
                .open("Open Core isolation", "windowsdefender://coreisolation"),
        ),
        _ => {}
    }
    if dev.b("HasBattery") == Some(true) {
        let sysvol = s.storage.a("volumes").into_iter().find(|v| v.s("Letter").as_deref() == Some("C:"));
        if sysvol.and_then(|v| v.s("BitLocker")).is_some_and(|b| b.starts_with("Off")) {
            f.push(
                Finding::new("encryption-off", SEC, Level::WorthDoing, "This laptop's drive is not encrypted")
                    .why("If the laptop is lost or stolen, someone could take the drive out and read your files. Encryption prevents that, and you won't notice it day to day.")
                    .step("Open Settings, Privacy & security, Device encryption. If you don't see it, your Windows edition may only offer BitLocker.")
                    .open("Open device encryption", "ms-settings:deviceencryption"),
            );
        }
    }
}
