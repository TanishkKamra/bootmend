use super::data::{BAD_EXTENSIONS, REMOTE_TOOLS, VULNERABLE_APPS};
use super::{plural, Action, Area, Confidence, Finding, Level};
use crate::fixes::{ExclusionKind, Fix};
use crate::gui::Page;
use crate::snapshot::Snapshot;
use crate::util::{version_lt, J};
use regex::Regex;
use std::sync::OnceLock;

const SEC: Area = Area::Security;

pub fn check(s: &Snapshot, f: &mut Vec<Finding>) {
    defender(s, f);
    firewall(s, f);
    access(s, f);
    extensions(s, f);
    apps(s, f);
    system(s, f);
    device_access(s, f);
    backup(s, f);
    extras(s, f);
    browser_and_network(s, f);
    hardening(s, f);
}

/// Work and school PCs: their IT department manages these settings.
pub(super) fn joined(s: &Snapshot) -> bool {
    s.system.g("device").b("Joined") == Some(true)
}

/// Windows 11 and its build number, from the build string ("26200.7000").
fn win11_build(s: &Snapshot) -> Option<u32> {
    let os = s.system.g("os");
    if !os.s("Edition").is_some_and(|e| e.contains("Windows 11")) {
        return None;
    }
    os.s("Build")?.split('.').next()?.parse().ok()
}

/// Apps that copy files somewhere else on a schedule. Sync folders such as Google Drive aren't
/// here: they only cover what's kept inside them.
const BACKUP_APPS: &[&str] = &[
    "acronis",
    "aomei backupper",
    "backblaze",
    "carbonite",
    "cobian",
    "crashplan",
    "duplicati",
    "easeus todo backup",
    "idrive",
    "iperius",
    "macrium",
    "paragon backup",
    "syncback",
    "veeam",
];

fn backup(s: &Snapshot, f: &mut Vec<Finding>) {
    let b = s.storage.g("backup");
    if b.is_null() {
        return;
    }
    let signed_in = b.b("OneDriveSignedIn") == Some(true);
    let running = b.b("OneDriveRunning") == Some(true);
    let folders = b.strs("FoldersInOneDrive");
    if !folders.is_empty() && !(signed_in && running) {
        let state = if signed_in { "isn't running" } else { "is signed out" };
        let mut x = Finding::new("onedrive-stopped", SEC, Level::WorthDoing, "OneDrive isn't backing up your folders")
            .why(format!(
                "Your {} are kept in the OneDrive folder, but OneDrive {state}, so nothing new is copied to the cloud. If this PC fails, recent changes would be lost.",
                join_and(&folders)
            ));
        x = if signed_in {
            x.step("Open OneDrive from the Start menu. It picks up where it left off.")
        } else {
            x.step("Open OneDrive from the Start menu and sign in with the same account. It uploads what changed.")
        };
        f.push(x.step("If you stopped using OneDrive on purpose, your files are still on this PC, just not copied anywhere."));
        return;
    }
    let app = s.software.a("apps").into_iter().filter_map(|a| a.s("Name")).any(|n| BACKUP_APPS.iter().any(|k| n.to_lowercase().contains(k)));
    if (signed_in && running && !folders.is_empty()) || b.b("FileHistory") == Some(true) || app {
        return;
    }
    let first = if signed_in {
        "OneDrive is signed in, but only its own folder is copied. In OneDrive's settings, open Sync and backup, then Manage back up, and switch on Desktop, Documents and Pictures."
    } else {
        "The simplest option is OneDrive: sign in and choose to back up Desktop, Documents and Pictures. The free plan holds 5 GB."
    };
    f.push(
        Finding::new("no-backup", SEC, Level::WorthDoing, "We found no backup of your files")
            .why("If the drive fails or the PC is lost, your documents and photos go with it. We looked for OneDrive folder backup, File History and common backup apps.")
            .step(first)
            .step("Or plug in an external drive and turn on File History: search Start for \"File History\".")
            .step("If you already copy your files somewhere yourself, we can't see that, so you can ignore this.")
            .conf(Confidence::BestGuess)
            .open("Open Windows Backup", "ms-settings:backup"),
    );
}

/// Public DNS services people choose on purpose. Anything else set by hand is worth a look.
const KNOWN_DNS: &[&str] = &[
    "1.1.1.",
    "1.0.0.",
    "8.8.8.8",
    "8.8.4.4",
    "9.9.9.",
    "149.112.112.",
    "208.67.220.",
    "208.67.222.",
    "94.140.14.",
    "94.140.15.",
    "45.90.28.",
    "45.90.30.",
    "76.76.2.",
    "76.76.10.",
    "185.228.168.",
    "185.228.169.",
];

/// A public internet address: not private, loopback, link-local or a carrier's shared range.
fn public_ipv4(ip: &str) -> bool {
    let Ok(a) = ip.trim().parse::<std::net::Ipv4Addr>() else { return false };
    let [x, y, ..] = a.octets();
    !(a.is_private() || a.is_loopback() || a.is_link_local() || a.is_unspecified() || (x == 100 && (64..128).contains(&y)))
}

/// Whether every server in a proxy setting ("http=127.0.0.1:8888;https=...") runs on this PC.
fn local_proxy(setting: &str) -> bool {
    setting.split(';').filter(|p| !p.trim().is_empty()).all(|p| {
        let p = p.rsplit('=').next().unwrap_or(p).trim();
        let p = p.split("://").last().unwrap_or(p);
        let host = p.split(['/', ':']).next().unwrap_or(p).to_lowercase();
        host == "localhost" || host.starts_with("127.") || p.starts_with("[::1]")
    })
}

fn browser_and_network(s: &Snapshot, f: &mut Vec<Finding>) {
    // Work and school PCs set all of these on purpose.
    if joined(s) {
        return;
    }
    let pol = s.security.a("browserPolicies");
    if !pol.is_empty() {
        let mut browsers: Vec<String> = vec![];
        for b in pol.iter().filter_map(|p| p.s("Browser")) {
            if !browsers.contains(&b) {
                browsers.push(b);
            }
        }
        f.push(
            Finding::new("browser-policies", SEC, Level::WorthDoing, format!("Something is controlling {}'s settings", join_and(&browsers)))
                .why("Settings such as the home page, search engine or extensions are locked by a policy, the way a work PC is managed. On a home PC, adware usually put them there. The browser's menu then says it's managed by your organisation.")
                .step("If you don't recognise them, remove programs you didn't install on purpose from Settings, Apps, Installed apps.")
                .step("Then run a full scan in Windows Security, which removes known adware.")
                .details(pol.iter().map(|p| format!("{}: {} = {}", p.s("Browser").unwrap_or_default(), p.s("Setting").unwrap_or_default(), p.s("Value").unwrap_or_default())))
                .conf(Confidence::BestGuess)
                .acts(policy_fixes(&pol))
                .open("Open Installed apps", "ms-settings:appsfeatures"),
        );
    }
    let p = s.network.g("proxy");
    let mut proxies = vec![];
    if p.b("Enabled") == Some(true) {
        proxies.extend(p.s("Server").filter(|x| !x.is_empty()).map(|x| format!("Proxy server: {x}")));
    }
    proxies.extend(p.s("Pac").filter(|x| !x.is_empty()).map(|x| format!("Setup script: {x}")));
    proxies.extend(p.s("WinHttp").filter(|x| !x.is_empty()).map(|x| format!("For Windows itself: {x} (netsh winhttp reset proxy clears it)")));
    if !proxies.is_empty() {
        let local = proxies.iter().all(|x| local_proxy(x.split_once(": ").map_or("", |(_, v)| v.split(' ').next().unwrap_or(v))));
        f.push(
            Finding::new("proxy", SEC, if local { Level::NiceToHave } else { Level::WorthDoing }, "Your internet traffic goes through a proxy")
                .why(if local {
                    "A program on this PC passes your web traffic through itself. VPNs, ad blockers and developer tools do this. If you use none of those, something else set it up."
                } else {
                    "Web traffic is sent through another computer first. Workplaces do this on purpose; on a home PC, adware sometimes sets it up to watch or change what you see."
                })
                .step("If you don't use a VPN or a tool that needs it, open Settings, Network & internet, Proxy, and switch off Use a proxy server and Use setup script.")
                .details(proxies)
                .act(Action::Fix(Fix::TurnOffProxy {
                    enabled: p.b("Enabled") == Some(true) && p.s("Server").is_some_and(|x| !x.is_empty()),
                    pac: p.s("Pac").unwrap_or_default(),
                    winhttp: p.s("WinHttp").unwrap_or_default(),
                }))
                .open("Open proxy settings", "ms-settings:network-proxy"),
        );
    }
    let odd: Vec<String> = s.network.strs("staticDns").into_iter().filter(|ip| public_ipv4(ip) && !KNOWN_DNS.iter().any(|k| ip.starts_with(k))).collect();
    if !odd.is_empty() {
        // Adapters still in the PC can be set back to automatic; the servers go into the undo.
        let fixes = s.network.a("dnsByHand").into_iter().filter_map(|a| {
            let servers = a.strs("Servers");
            if !servers.iter().any(|x| odd.contains(x)) {
                return None;
            }
            Some(Action::Fix(Fix::DnsAutomatic { adapter: a.s("Adapter")?, servers }))
        });
        f.push(
            Finding::new("dns-changed", SEC, Level::WorthDoing, "This PC uses a DNS server we don't recognise")
                .why("DNS turns website names into addresses, so whoever runs it can send you to fake copies of real sites. This one was set on the PC by hand or by a program, not by your router.")
                .step("If you didn't choose it, open Settings, Network & internet, your connection, Hardware properties, and set DNS server assignment to Automatic (DHCP).")
                .details(odd.iter().map(|i| format!("DNS server: {i}")))
                .conf(Confidence::BestGuess)
                .acts(fixes)
                .open("Open network settings", "ms-settings:network-status"),
        );
    }
    let w = s.network.g("wifi");
    if let Some(ssid) = w.s("SSID") {
        let (auth, cipher) = (w.s("Auth").unwrap_or_default(), w.s("Cipher").unwrap_or_default());
        let weak = auth.contains("WEP") || cipher.contains("WEP") || cipher.contains("TKIP");
        let open = !weak && auth.eq_ignore_ascii_case("Open");
        if weak || open {
            f.push(
                Finding::new(
                    "wifi-weak",
                    SEC,
                    Level::WorthDoing,
                    if open { format!("{ssid} is a Wi-Fi network without a password") } else { format!("{ssid} uses outdated Wi-Fi security") },
                )
                .why(if open {
                    "Anyone nearby can see traffic on a network without a password. At a café or airport that's normal: avoid banking there, or use your phone's hotspot."
                } else {
                    "Its security can be cracked in minutes, letting someone nearby join the network or read what's sent over it."
                })
                .step(if open {
                    "If this is your own network, set a password on the router using WPA2 or WPA3."
                } else {
                    "In the router's settings, switch Wi-Fi security to WPA2 (AES) or WPA3. The label on the router or its manual says how to sign in to it."
                })
                .detail(format!("Security: {auth} {cipher}").trim_end().to_string()),
            );
        }
    }
}

/// The services Windows needs to stay protected and up to date: friendly name, and how Windows 11
/// starts each one. None marks the three whose settings Windows refuses to let even administrators
/// change, so they get no button.
const CORE_SERVICES: &[(&str, &str, Option<&str>)] = &[
    ("wuauserv", "Windows Update", Some("Manual")),
    ("BITS", "Background Intelligent Transfer Service, which downloads updates", Some("Delayed")),
    ("UsoSvc", "Update Orchestrator Service", Some("Delayed")),
    ("WinDefend", "Microsoft Defender Antivirus", None),
    ("mpssvc", "Windows Defender Firewall", None),
    ("wscsvc", "Security Center", None),
    ("EventLog", "Windows Event Log", Some("Automatic")),
    ("CryptSvc", "Cryptographic Services", Some("Automatic")),
    ("Winmgmt", "Windows Management Instrumentation", Some("Automatic")),
    ("W32Time", "Windows Time", Some("Manual")),
];

/// One fix per policy key, in the order the keys were found.
fn policy_fixes(pol: &[&serde_json::Value]) -> Vec<Action> {
    let mut keys: Vec<(String, String, Vec<String>)> = vec![];
    for p in pol {
        let (Some(browser), Some(key), Some(setting)) = (p.s("Browser"), p.s("Key"), p.s("Setting")) else { continue };
        match keys.iter_mut().find(|(_, k, _)| *k == key) {
            Some((_, _, settings)) => settings.push(setting),
            None => keys.push((browser, key, vec![setting])),
        }
    }
    keys.into_iter().map(|(browser, key, settings)| Action::Fix(Fix::RemoveBrowserPolicies { browser, key, settings })).collect()
}

/// A program that starts from a throwaway folder or a hidden script, as harmful software does.
fn odd_start(cmd: &str) -> bool {
    static PATTERNS: OnceLock<[Regex; 2]> = OnceLock::new();
    let [encoded, loose] = PATTERNS.get_or_init(|| {
        [
            // PowerShell takes any short form of -EncodedCommand ("-e", "-ec", "-enc"...), but "-ex" is -ExecutionPolicy.
            Regex::new(r"(?i)\b(powershell|pwsh)(\.exe)?\b.*\s[-/](e|ec|en[a-z]*)\s").expect("valid"),
            Regex::new(r"(?i)c:\\programdata\\[^\\]+\.(exe|dll|bat|cmd|vbs|js|ps1)\b").expect("valid"),
        ]
    });
    let c = cmd.to_lowercase();
    let places = ["\\appdata\\local\\temp\\", "%temp%", "%tmp%", "\\windows\\temp\\", "\\users\\public\\", "\\downloads\\", "$recycle.bin"];
    places.iter().any(|p| c.contains(p))
        || encoded.is_match(cmd)
        || loose.is_match(cmd)
        || c.contains("mshta")
        || (c.contains("rundll32") && (c.contains("http") || c.contains("javascript:")))
}

fn hardening(s: &Snapshot, f: &mut Vec<Finding>) {
    let sec = &s.security;
    let d = sec.g("defender");
    let others = !sec.strs("otherAntivirus").is_empty();
    let mut off = sec.strs("disabledServices");
    if others {
        off.retain(|x| x != "WinDefend");
    }
    if !off.is_empty() {
        let known = |n: &String| CORE_SERVICES.iter().find(|(k, _, _)| k.eq_ignore_ascii_case(n));
        let name = |n: &String| known(n).map_or(n.clone(), |(_, v, _)| v.to_string());
        let title = if off.len() == 1 { format!("{} is switched off", name(&off[0])) } else { format!("{} Windows services are switched off", off.len()) };
        let locked: Vec<String> = off.iter().filter(|n| known(n).is_some_and(|(_, _, start)| start.is_none())).map(name).collect();
        let mut x = Finding::new("services-off", SEC, Level::WorthDoing, title)
            .why("Windows needs these to stay up to date and protected. \"Debloat\" and tuning tools often switch them off.")
            .details(off.iter().map(|n| format!("{} ({n})", name(n))))
            .acts(off.iter().filter_map(|n| {
                let (_, display, start) = known(n)?;
                Some(Action::Fix(Fix::EnableService { name: n.clone(), display: display.to_string(), start: start.as_ref()?.to_string() }))
            }));
        if !locked.is_empty() {
            x = x.step(format!(
                "Windows doesn't let anyone change {} directly. Undo it in the tool that switched it off, or open Settings, System, Recovery, and choose Fix problems using Windows Update, which reinstalls Windows and keeps your files and apps.",
                join_and(&locked)
            ));
        }
        f.push(x);
    }
    let mut odd: Vec<String> = s
        .software
        .a("startup")
        .into_iter()
        .filter(|x| x.b("Enabled") != Some(false) && x.s("Command").is_some_and(|c| odd_start(&c)))
        .map(|x| format!("{}: {}", x.s("Name").unwrap_or_default(), x.s("Command").unwrap_or_default()))
        .collect();
    odd.extend(
        s.software
            .a("tasks")
            .into_iter()
            .filter(|x| x.s("Action").is_some_and(|c| odd_start(&c)))
            .map(|x| format!("Scheduled task {}: {}", x.s("Name").unwrap_or_default(), x.s("Action").unwrap_or_default())),
    );
    if !odd.is_empty() {
        f.push(
            Finding::new("startup-odd", SEC, Level::WorthDoing, format!("{} from a temporary or hidden place", plural(odd.len(), "program starts", "programs start")))
                .why("Real apps start from where they're installed. Starting from a temporary folder, the Downloads folder or a hidden script is a common trick of harmful software.")
                .step("Look up each name below. If you don't recognise it, switch it off in Tune-up's start-up list, then run a full scan in Windows Security.")
                .details(odd.into_iter().map(|x| x.chars().take(160).collect()))
                .conf(Confidence::BestGuess)
                .act(Action::Goto(Page::Startup)),
        );
    }
    let defender_on = d.b("RealTime") == Some(true) && !others;
    if defender_on && d.i("CloudProtection") == Some(0) {
        f.push(
            Finding::new("av-cloud-off", SEC, Level::WorthDoing, "Cloud protection is off")
                .why("Windows Security checks new and unknown files with Microsoft's cloud, which catches threats hours before a normal update. Windows Security shows a warning about this too.")
                .step("Open Windows Security, Virus & threat protection, Manage settings, and switch on Cloud-delivered protection.")
                .act(Action::Fix(Fix::EnableCloudProtection))
                .open("Open virus protection settings", "windowsdefender://threatsettings"),
        );
    }
    if defender_on && d.i("SampleSubmission") == Some(2) {
        f.push(
            Finding::new("av-samples-off", SEC, Level::WorthDoing, "Automatic sample submission is off")
                .why("When Windows Security finds a suspicious program, it can send it to Microsoft to check. Files likely to hold personal information still ask first. Windows Security shows a warning about this too.")
                .step("Open Windows Security, Virus & threat protection, Manage settings, and switch on Automatic sample submission.")
                .act(Action::Fix(Fix::EnableSampleSubmission))
                .open("Open virus protection settings", "windowsdefender://threatsettings"),
        );
    }
    let today = chrono::Local::now().date_naive();
    let last_scan = [d.s("LastQuickScan"), d.s("LastFullScan")]
        .into_iter()
        .flatten()
        .filter_map(|x| chrono::NaiveDate::parse_from_str(x.get(..10)?, "%Y-%m-%d").ok())
        .max();
    // Real-time protection is the main defence, so a quiet month is only worth a nudge (our judgement).
    if let Some(days) = last_scan.map(|x| (today - x).num_days()).filter(|&n| defender_on && n > 30) {
        f.push(
            Finding::new("av-scan-old", SEC, Level::NiceToHave, format!("Windows Security hasn't scanned in {days} days"))
                .why("Real-time protection checks files as you use them, so this isn't urgent. A scan also catches anything that slipped in while protection was off.")
                .step("Open Windows Security, Virus & threat protection, and choose Quick scan.")
                .open("Open virus protection", "windowsdefender://threat"),
        );
    }
    // Windows pauses encryption for a restart around some updates.
    if s.system.g("os").b("PendingReboot") != Some(true) {
        let paused: Vec<String> = s
            .storage
            .a("volumes")
            .into_iter()
            .filter(|v| v.s("BitLocker").is_some_and(|b| b.starts_with("Off") && b.contains("Encrypt")))
            .filter_map(|v| v.s("Letter"))
            .collect();
        if !paused.is_empty() {
            f.push(
                Finding::new("bitlocker-suspended", SEC, Level::WorthDoing, format!("Encryption is paused on {}", join_and(&paused)))
                    .why("The drive is encrypted, but its protection is paused, so anyone who takes the drive out could still read it. Windows pauses it briefly around some updates; this time it stayed off.")
                    .step("Open Settings, Privacy & security, Device encryption, and switch it on. On Pro editions, BitLocker in Control Panel shows Resume protection instead.")
                    .acts(paused.iter().map(|d| Action::Fix(Fix::ResumeEncryption { drive: d.clone() })))
                    .open("Open device encryption", "ms-settings:deviceencryption"),
            );
        }
    }
    let os = s.system.g("os");
    if win11_build(s).is_some() && os.i("DriverBlocklist") == Some(0) {
        // Memory integrity enforces the list anyway.
        let level = if os.s("MemoryIntegrity").as_deref() == Some("On") { Level::NiceToHave } else { Level::WorthDoing };
        f.push(
            Finding::new("driver-blocklist-off", SEC, level, "Windows' list of unsafe drivers is switched off")
                .why("Microsoft keeps a list of drivers with known security holes that attackers misuse, and Windows blocks them. Some old games' anti-cheat or hardware tools ask you to switch it off.")
                .step("Open Windows Security, Device security, Core isolation details, and switch on Microsoft Vulnerable Driver Blocklist.")
                .act(Action::Fix(Fix::EnableDriverBlocklist))
                .open("Open Core isolation", "windowsdefender://coreisolation"),
        );
    }
    let running: Vec<String> =
        sec.a("antivirus").into_iter().filter(|a| a.b("On") == Some(true)).filter_map(|a| a.s("Name")).filter(|n| !n.contains("Defender")).collect();
    if running.len() >= 2 {
        f.push(
            Finding::new("two-av", SEC, Level::WorthDoing, "Two antivirus apps are running at once")
                .why("They check the same files twice and can block each other, which slows the PC and can cause crashes. One good antivirus is enough, and Windows Security is free.")
                .step("Uninstall the one you don't want from Settings, Apps, Installed apps.")
                .details(running)
                .open("Open Installed apps", "ms-settings:appsfeatures"),
        );
    }
    let daily = sec.g("dailyAccount").s("Name").unwrap_or_default();
    let daily = daily.rsplit('\\').next().unwrap_or_default().to_lowercase();
    for a in sec.a("accounts") {
        let (Some(name), Some(rid)) = (a.s("Name"), a.i("Rid")) else { continue };
        if a.b("Enabled") != Some(true) || name.to_lowercase() == daily {
            continue;
        }
        let (id, title, why) = match rid {
            501 => ("guest-on", "The Guest account is switched on", "Anyone at the keyboard can use this PC through it without a password."),
            500 => (
                "builtin-admin-on",
                "The built-in Administrator account is switched on",
                "It's the same on every Windows PC, often has no password and never asks before making changes, so attackers try it first.",
            ),
            _ => continue,
        };
        f.push(
            Finding::new(id, SEC, Level::WorthDoing, title)
                .why(why)
                .step(format!("Search Start for Command Prompt, choose Run as administrator, and type: net user \"{name}\" /active:no"))
                .act(Action::Fix(Fix::DisableAccount { name: name.clone() })),
        );
    }
}

/// "Desktop, Documents and Pictures".
fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Protections that are off by default or can get in the way, plus LSA protection, which Windows
/// Security warns about.
fn extras(s: &Snapshot, f: &mut Vec<Finding>) {
    let d = s.security.g("defender");
    let build = win11_build(s);
    if build.is_some() && !joined(s) && d.s("SmartAppControl").as_deref() == Some("Off") {
        f.push(
            Finding::new("sac-off", SEC, Level::NiceToHave, "Smart App Control is off")
                .why("It blocks unknown and untrusted apps before they run, on top of your antivirus. It can also block some older or home-made apps; if one you need is blocked, switch it off again.")
                .step("Open Windows Security, App & browser control, Smart App Control settings, and choose On. If the switch is greyed out, install waiting updates first.")
                .open("Open App & browser control", "windowsdefender://appbrowser"),
        );
    }
    let defender_on = d.b("RealTime") == Some(true) && s.security.strs("otherAntivirus").is_empty();
    if defender_on && d.i("ControlledFolderAccess") == Some(0) {
        f.push(
            Finding::new("cfa-off", SEC, Level::NiceToHave, "Ransomware protection for your folders is off")
                .why("Controlled folder access stops unknown apps from changing files in Documents, Pictures and other personal folders, which is what ransomware does. It can also stop a trusted app from saving there; Windows tells you when, and you can allow that app.")
                .step("Open Windows Security, Virus & threat protection, Ransomware protection, and switch on Controlled folder access.")
                .open("Open Virus & threat protection", "windowsdefender://threat"),
        );
    }
    let daily = s.security.g("dailyAccount");
    let others = daily.strs("OtherAdmins");
    // With no other administrator, demoting this account would leave nobody able to fix the PC.
    if daily.b("Admin") == Some(true) && !others.is_empty() && s.security.g("access").b("UAC") != Some(false) && !joined(s) {
        let other = &others[0];
        f.push(
            Finding::new("daily-admin", SEC, Level::NiceToHave, "You use an administrator account every day")
                .why("A harmful program that runs while you're signed in can change anything on the PC. With a standard account it can't, and Windows asks for an administrator's password when a change really needs one.")
                .step(format!("Sign in to {other}, the other administrator account. You'll need its password."))
                .step("Open Settings, Accounts, Other users, select your everyday account, choose Change account type, and pick Standard User.")
                .step(format!("Sign back in to your own account. When Windows needs permission for a change, it asks for {other}'s password."))
                .details(others.iter().map(|o| format!("Other administrator: {o}")))
                .open("Open Other users", "ms-settings:otherusers"),
        );
    }
    // Windows Security has a switch for it from Windows 11 22H2 (build 22621) on. Windows 10 has
    // only a registry setting that locks into the firmware, which isn't safe to suggest.
    let a = s.security.g("access");
    let on = a.b("LsaRunning") == Some(true) || matches!(a.i("LsaPpl"), Some(1 | 2));
    if build.is_some_and(|b| b >= 22621) && !on && !joined(s) {
        f.push(
            Finding::new("lsa-off", SEC, Level::WorthDoing, "Local Security Authority protection is off")
                .why("It stops harmful programs from reading your sign-in details out of Windows' memory. Windows Security shows a warning about it too.")
                .step("Open Windows Security, Device security, Core isolation details, switch on Local Security Authority protection, then restart.")
                .step("Rarely, an old fingerprint reader or smart-card add-on stops working. If that happens, switch it off again.")
                .act(Action::Fix(Fix::EnableLsaProtection))
                .open("Open Core isolation", "windowsdefender://coreisolation"),
        );
    }
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
    // The 2011 Secure Boot certificates expired in 2026, and Windows Update swaps in the 2023 ones.
    // The status values are community-documented, hence the best-guess label.
    if dev.b("SecureBoot") == Some(true) && !super::windows::without_updates(s) {
        if let Some(status) = dev.s("SecureBootCert") {
            let capable = dev.i("SecureBootCertCapable");
            if status != "Updated" || capable.is_some_and(|c| c < 2) {
                let started = status != "NotStarted" || capable.is_some_and(|c| c >= 1);
                let mut x = Finding::new("secureboot-cert", SEC, Level::WorthDoing, "Secure Boot needs its new certificates")
                    .why(if started {
                        "The certificates Secure Boot used since 2011 expired in 2026. Windows has started switching to the 2023 ones and needs a restart or two to finish."
                    } else {
                        "The certificates Secure Boot used since 2011 expired in 2026. Windows Update installs the 2023 replacements, but this PC doesn't have them yet. It still starts, but gets no new fixes for the start-up process."
                    })
                    .step("Install everything waiting in Windows Update, then restart twice.")
                    .step("If this is still here after a few days, look for a firmware (BIOS) update on your PC or motherboard maker's support page. Bootmend never updates firmware itself.")
                    .detail(format!("Certificate update: {status}"))
                    .conf(Confidence::BestGuess)
                    .open("Open Windows Update", "ms-settings:windowsupdate");
                if dev.b("SecureBootCertError") == Some(true) {
                    x = x.detail("Windows reported an error the last time it tried");
                }
                f.push(x);
            }
        }
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
        // "Off / FullyEncrypted" is paused encryption, which has its own to-do.
        if sysvol.and_then(|v| v.s("BitLocker")).is_some_and(|b| b.starts_with("Off") && !b.contains("Encrypt")) {
            f.push(
                Finding::new("encryption-off", SEC, Level::WorthDoing, "This laptop's drive is not encrypted")
                    .why("If the laptop is lost or stolen, someone could take the drive out and read your files. Encryption prevents that, and you won't notice it day to day.")
                    .step("Open Settings, Privacy & security, Device encryption. If you don't see it, your Windows edition may only offer BitLocker.")
                    .open("Open device encryption", "ms-settings:deviceencryption"),
            );
        }
    }
}
