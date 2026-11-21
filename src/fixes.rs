//! Opt-in fixes. Every fix shows what it will do, asks first, and stores an undo step.

use crate::util::{data_dir, powershell, ps_quote};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Fix {
    DisableStartup {
        name: String,
        approved_key: String,
    },
    EnableStartup {
        name: String,
        approved_key: String,
    },
    RemoveExclusion {
        kind: ExclusionKind,
        value: String,
    },
    EnablePua,
    DisableRemoteAssistance,
    DisableFirewallRule {
        name: String,
        display: String,
    },
    ServiceManual {
        name: String,
        display: String,
    },
    DisableSmb1,
    EnableSystemProtection,
    AllowAutoUpdates,
    /// `start` is Windows' default for the service: "Automatic", "Manual" or "Delayed".
    EnableService {
        name: String,
        display: String,
        start: String,
    },
    DisableAccount {
        name: String,
    },
    EnableCloudProtection,
    EnableSampleSubmission,
    EnableTrim,
    ResumeEncryption {
        drive: String,
    },
    SystemManagedPagefile,
    EnableDriverBlocklist,
    EnableLsaProtection,
    EnableCrashFiles,
    /// What was switched on, so undo puts back exactly that.
    TurnOffProxy {
        enabled: bool,
        pac: String,
        winhttp: String,
    },
    DnsAutomatic {
        adapter: String,
        servers: Vec<String>,
    },
    /// `key` as reg.exe writes it: "HKLM\SOFTWARE\Policies\Google\Chrome".
    RemoveBrowserPolicies {
        browser: String,
        key: String,
        settings: Vec<String>,
    },
    EnableRecovery,
    /// Update policies that send updates to another server or hide Windows Update. `key` as reg.exe
    /// writes it; a setting in a subkey is named "AU\UseWUServer".
    RemoveUpdateBlock {
        key: String,
        settings: Vec<String>,
    },
    /// DISM, then System File Checker. It only puts back Windows' own copies, so there's nothing to undo.
    RepairWindowsFiles,
    /// Puts back the driver Windows kept from before the device's last update. Windows then removes
    /// the newer one, so there's nothing to undo.
    RollBackDriver {
        /// The device instance ID: "PCI\VEN_1002&DEV_73DF\6&15bb039d&0&00000019".
        device: String,
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ExclusionKind {
    Path,
    Process,
    Extension,
}

impl ExclusionKind {
    fn param(self) -> &'static str {
        match self {
            ExclusionKind::Path => "ExclusionPath",
            ExclusionKind::Process => "ExclusionProcess",
            ExclusionKind::Extension => "ExclusionExtension",
        }
    }
}

const DISABLED: &str = "([byte[]](3,0,0,0,0,0,0,0,0,0,0,0))";
/// Runs a Windows command-line tool and returns its exit code. Its messages are dropped because
/// Windows PowerShell turns a tool's error output into a terminating error under 'Stop', before
/// the exit code can be checked.
const NATIVE: &str = "function Native([scriptblock]$b) { $ErrorActionPreference = 'Continue'; & $b 2>&1 | Out-Null; $LASTEXITCODE }\n";
const ENABLED: &str = "([byte[]](2,0,0,0,0,0,0,0,0,0,0,0))";

/// The keys holding each startup item's on/off switch, as the check-up reads them.
const STARTUP_APPROVED: [&str; 5] = [
    r"HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
    r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
    r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32",
    r"HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder",
    r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder",
];

/// The policy key Windows Update's settings live under, as reg.exe writes it.
pub const UPDATE_POLICY: &str = r"HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate";

/// Said when a fix read back from a file isn't one the check-up would give.
const APPLY_CHANGED: &str = "Bootmend didn't make this change, because its details were altered after it was set up.";
/// Said when a History entry, or the record kept for a change made as administrator, was altered.
const UNDO_CHANGED: &str = "Bootmend can't undo this, because its record was altered. A restore point can still take the PC back.";

/// Plain text: not empty, not too long, and no control characters.
fn text(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 400 && !s.chars().any(char::is_control)
}

/// A registry path below a known key: no empty parts, and no "." or "..", which PowerShell
/// follows even in a literal path.
fn below(s: &str) -> bool {
    text(s) && s.split('\\').all(|p| !p.is_empty() && p != "." && p != "..")
}

fn starts_ci(s: &str, prefix: &str) -> bool {
    s.get(..prefix.len()).is_some_and(|h| h.eq_ignore_ascii_case(prefix))
}

/// A Windows service name: no slashes, which Windows doesn't allow in one.
fn service(name: &str) -> bool {
    text(name) && name.chars().count() <= 256 && !name.contains(['/', '\\'])
}

impl Fix {
    /// Short button label: a verb naming the outcome.
    pub fn label(&self) -> &'static str {
        match self {
            Fix::DisableStartup { .. } => "Don't start with Windows",
            Fix::EnableStartup { .. } => "Start with Windows",
            Fix::RemoveExclusion { .. } => "Remove from skip list",
            Fix::EnablePua => "Turn on blocking",
            Fix::DisableRemoteAssistance => "Turn off Remote Assistance",
            Fix::DisableFirewallRule { .. } => "Turn off this rule",
            Fix::ServiceManual { .. } => "Start only when needed",
            Fix::DisableSmb1 => "Turn off SMB1",
            Fix::EnableSystemProtection => "Turn on System Restore",
            Fix::AllowAutoUpdates => "Let Windows update itself",
            Fix::EnableService { .. } => "Switch back on",
            Fix::DisableAccount { .. } => "Switch off this account",
            Fix::EnableCloudProtection => "Turn on cloud protection",
            Fix::EnableSampleSubmission => "Turn on sample submission",
            Fix::EnableTrim => "Turn on TRIM",
            Fix::ResumeEncryption { .. } => "Resume encryption",
            Fix::SystemManagedPagefile => "Let Windows manage it",
            Fix::EnableDriverBlocklist => "Turn on the blocklist",
            Fix::EnableLsaProtection => "Turn on LSA protection",
            Fix::EnableCrashFiles => "Turn on crash files",
            Fix::TurnOffProxy { .. } => "Turn off the proxy",
            Fix::DnsAutomatic { .. } => "Use automatic DNS",
            Fix::RemoveBrowserPolicies { .. } => "Remove the locked settings",
            Fix::EnableRecovery => "Switch on Windows Recovery",
            Fix::RemoveUpdateBlock { .. } => "Unblock Windows Update",
            Fix::RepairWindowsFiles => "Repair Windows files",
            Fix::RollBackDriver { .. } => "Go back to the previous driver",
        }
    }

    /// One sentence for the confirmation dialog: what changes, and that it can be undone.
    pub fn describe(&self) -> String {
        match self {
            Fix::DisableStartup { name, .. } => format!("{} will no longer open when you sign in. You can still open it yourself, and you can undo this from History.", name.trim_end_matches(".lnk")),
            Fix::EnableStartup { name, .. } => format!("{} will open again when you sign in.", name.trim_end_matches(".lnk")),
            Fix::RemoveExclusion { value, .. } => format!("Windows Security will check {value} again. If an app you trust stops working afterwards, undo this from History."),
            Fix::EnablePua => "Windows Security will block apps that are not viruses but bundle adware, toolbars or crypto miners. You can undo this from History.".into(),
            Fix::DisableRemoteAssistance => "Nobody will be able to send this PC a Remote Assistance invitation. You can turn it back on from History.".into(),
            Fix::DisableFirewallRule { display, .. } => format!("The firewall rule \"{display}\" will be switched off, not deleted. You can switch it back on from History."),
            Fix::ServiceManual { display, .. } => format!("{display} will start only when an app asks for it, instead of every time Windows starts. You can undo this from History."),
            Fix::DisableSmb1 => "The old SMB1 file-sharing protocol will be removed. Very old network drives may stop working; you can turn it back on from History. A restart finishes the change.".into(),
            Fix::EnableSystemProtection => "Windows will start saving restore points for drive C before updates and driver installs, using a small share of the drive. Undoing this later deletes the saved restore points.".into(),
            Fix::AllowAutoUpdates => "The policy that stops automatic updates will be removed, so Windows installs security fixes by itself again. You can put the policy back from History.".into(),
            Fix::EnableService { display, .. } => format!("{display} will start the way Windows sets it up. A restart finishes the change, and you can switch it off again from History."),
            Fix::DisableAccount { name } => format!("The {name} account will be switched off, not deleted. You can switch it back on from History."),
            Fix::EnableCloudProtection => "Windows Security will check new and unknown files with Microsoft's cloud. You can undo this from History.".into(),
            Fix::EnableSampleSubmission => "Windows Security will send suspicious programs to Microsoft to check. Files likely to hold personal information still ask first. You can undo this from History.".into(),
            Fix::EnableTrim => "Windows will tell the SSD which space is free again. You can undo this from History.".into(),
            Fix::ResumeEncryption { drive } => format!("Protection on {drive} will be switched back on. Have your recovery key to hand in case Windows asks for it when the PC next starts: it's saved in your Microsoft account at account.microsoft.com/devices/recoverykey. You can pause it again from History."),
            Fix::SystemManagedPagefile => "Windows will create a page file and choose its size itself. A restart finishes the change, and you can undo it from History.".into(),
            Fix::EnableDriverBlocklist => "Windows will block drivers on Microsoft's list of unsafe ones. If an old game's anti-cheat or a hardware tool stops working, undo this from History. A restart finishes the change.".into(),
            Fix::EnableLsaProtection => "Windows will guard the part of it that holds your sign-in details. Rarely, an old fingerprint reader or smart-card add-on stops working; if so, undo this from History. A restart finishes the change.".into(),
            Fix::EnableCrashFiles => "Windows will save the details of a crash again, the way it does out of the box, so the cause can be found. A restart finishes the change, and you can undo it from History.".into(),
            Fix::TurnOffProxy { .. } => "Web traffic will go straight to the internet again. If a VPN or another tool stops working, switch the proxy back on from History. Restart your browser afterwards.".into(),
            Fix::DnsAutomatic { adapter, servers } => format!("{adapter} will use the DNS servers your router hands out. To go back to {}, undo this from History.", servers.join(", ")),
            Fix::RemoveBrowserPolicies { browser, .. } => format!("The locked settings will be removed from {browser}, after saving a copy so you can put them back from History. Restart {browser} afterwards."),
            Fix::EnableRecovery => "Windows' recovery tools will be switched back on, so it can repair itself or reset if it ever fails to start. You can switch them off again from History.".into(),
            Fix::RemoveUpdateBlock { .. } => "The settings blocking Windows Update will be removed, after saving a copy so you can put them back from History. Windows then gets updates from Microsoft again.".into(),
            Fix::RepairWindowsFiles => "Windows will replace its damaged files with fresh copies from Microsoft, so it needs the internet. It can take up to 30 minutes, and you can keep using your PC. Your own files and apps aren't touched, so there's nothing to undo.".into(),
            Fix::RollBackDriver { name, .. } => format!("Windows will put back the driver the {name} used before its last update. The screen may flicker, and the PC may need a restart. This can't be undone from History, and Windows Update may offer the newer driver again."),
        }
    }

    /// What Bootmend is doing while the fix runs, said warmly and plainly, without a full stop.
    pub fn doing(&self) -> String {
        match self {
            Fix::DisableStartup { name, .. } => format!("Stopping {} from opening when you sign in", name.trim_end_matches(".lnk")),
            Fix::EnableStartup { name, .. } => format!("Letting {} open when you sign in again", name.trim_end_matches(".lnk")),
            Fix::RemoveExclusion { value, .. } => format!("Asking Windows Security to check {value} again"),
            Fix::EnablePua => "Asking Windows Security to block adware and unwanted apps".into(),
            Fix::DisableRemoteAssistance => "Closing the door on Remote Assistance invitations".into(),
            Fix::DisableFirewallRule { display, .. } => format!("Switching off the firewall rule \"{display}\""),
            Fix::ServiceManual { display, .. } => format!("Letting {display} start only when it's needed"),
            Fix::DisableSmb1 => "Retiring SMB1, an old way of sharing files".into(),
            Fix::EnableSystemProtection => "Turning System Restore back on, so there's a way back".into(),
            Fix::AllowAutoUpdates => "Letting Windows fetch its own updates again".into(),
            Fix::EnableService { display, .. } => format!("Switching {display} back on"),
            Fix::DisableAccount { name } => format!("Switching off the {name} account"),
            Fix::EnableCloudProtection => "Letting Windows Security check new threats with Microsoft".into(),
            Fix::EnableSampleSubmission => "Letting Windows Security send suspicious programs to Microsoft".into(),
            Fix::EnableTrim => "Telling Windows to keep the SSD's free space tidy".into(),
            Fix::ResumeEncryption { drive } => format!("Switching protection on {drive} back on"),
            Fix::SystemManagedPagefile => "Handing the page file back to Windows".into(),
            Fix::EnableDriverBlocklist => "Turning on Microsoft's list of unsafe drivers".into(),
            Fix::EnableLsaProtection => "Putting a guard on your sign-in details".into(),
            Fix::EnableCrashFiles => "Asking Windows to keep notes the next time something crashes".into(),
            Fix::TurnOffProxy { .. } => "Sending web traffic straight to the internet again".into(),
            Fix::DnsAutomatic { adapter, .. } => format!("Letting {adapter} use your router's DNS again"),
            Fix::RemoveBrowserPolicies { browser, .. } => format!("Unlocking {browser}'s settings, with a copy kept"),
            Fix::EnableRecovery => "Switching Windows' repair tools back on".into(),
            Fix::RemoveUpdateBlock { .. } => "Unblocking Windows Update".into(),
            Fix::RepairWindowsFiles => "Repairing Windows' own files, which can take up to 30 minutes".into(),
            Fix::RollBackDriver { name, .. } => format!("Going back to the previous driver for the {name}"),
        }
    }

    /// Whether History can take it back.
    pub fn undoable(&self) -> bool {
        !matches!(self, Fix::RepairWindowsFiles | Fix::RollBackDriver { .. })
    }

    /// Whether "Fix the safe ones for me" may include it. Going back to an older driver is a
    /// judgement call that can't be undone, so it's only offered on its own to-do.
    pub fn safe(&self) -> bool {
        !matches!(self, Fix::RollBackDriver { .. })
    }

    /// The longest it usually takes, for the fixes that take minutes rather than seconds.
    pub fn up_to_minutes(&self) -> Option<u32> {
        match self {
            Fix::RepairWindowsFiles => Some(30),
            Fix::RollBackDriver { .. } => Some(5),
            _ => None,
        }
    }

    /// Generous, so a slow PC isn't cut off part-way.
    fn timeout(&self) -> Duration {
        self.up_to_minutes().map_or(Duration::from_secs(300), |m| Duration::from_secs(u64::from(m) * 60 * 3))
    }

    /// Per-user startup entries can be changed without administrator rights; everything else needs them.
    pub fn needs_admin(&self) -> bool {
        match self {
            Fix::DisableStartup { approved_key, .. } | Fix::EnableStartup { approved_key, .. } => starts_ci(approved_key, "HKLM"),
            Fix::TurnOffProxy { winhttp, .. } => !winhttp.is_empty(),
            Fix::RemoveBrowserPolicies { key, .. } | Fix::RemoveUpdateBlock { key, .. } => starts_ci(key, "HKLM"),
            _ => true,
        }
    }

    /// Whether every detail has the shape the check-up gives it. History, and the queue saved before
    /// Bootmend restarts as administrator, are files any app the user runs can change, so a fix is
    /// checked before it runs or is undone.
    pub fn check(&self) -> bool {
        let settings = |s: &[String]| !s.is_empty() && s.iter().all(|x| below(x));
        match self {
            Fix::DisableStartup { name, approved_key } | Fix::EnableStartup { name, approved_key } => {
                text(name) && STARTUP_APPROVED.iter().any(|k| k.eq_ignore_ascii_case(approved_key))
            }
            Fix::RemoveExclusion { value, .. } => text(value),
            Fix::DisableFirewallRule { name, .. } | Fix::RollBackDriver { device: name, .. } => text(name),
            Fix::ServiceManual { name, .. } => service(name),
            Fix::EnableService { name, start, .. } => service(name) && ["Automatic", "Manual", "Delayed"].contains(&start.as_str()),
            // Windows' rules for account names, which also keep out PowerShell's wildcards.
            Fix::DisableAccount { name } => text(name) && name.chars().count() <= 20 && !name.contains(|c| "\"/\\[]:;|=,+*?<>@".contains(c)),
            Fix::ResumeEncryption { drive } => matches!(drive.as_bytes(), [l, b':'] if l.is_ascii_alphabetic()),
            Fix::TurnOffProxy { pac, winhttp, .. } => [pac, winhttp].iter().all(|s| s.is_empty() || text(s)),
            Fix::DnsAutomatic { adapter, servers } => text(adapter) && !servers.is_empty() && servers.iter().all(|s| s.parse::<std::net::IpAddr>().is_ok()),
            Fix::RemoveBrowserPolicies { key, settings: s, .. } => {
                [r"HKLM\SOFTWARE\Policies\", r"HKCU\SOFTWARE\Policies\"].iter().any(|p| starts_ci(key, p) && below(&key[p.len()..])) && settings(s)
            }
            Fix::RemoveUpdateBlock { key, settings: s } => key.eq_ignore_ascii_case(UPDATE_POLICY) && settings(s),
            _ => true,
        }
    }

    /// Where the change's record and saved copies are kept, as reg.exe writes it. A change that
    /// needs administrator rights keeps them under HKLM, which only administrators can change.
    fn store(&self, id: &str) -> String {
        format!(r"{}\SOFTWARE\Bootmend\Changes\{id}", if self.needs_admin() { "HKLM" } else { "HKCU" })
    }

    /// PowerShell that applies the change, and PowerShell that reverses it. `id` names the change.
    /// A change made as administrator first records the fix under HKLM, and its undo runs only
    /// while that record still holds the same fix, since History is a file any app the user runs can change.
    pub fn scripts(&self, id: &str) -> (String, String) {
        let store = self.store(id);
        let (mut apply, mut undo) = self.steps(&store);
        if !self.undoable() {
            return (apply, undo);
        }
        if self.needs_admin() {
            let key = ps_quote(&store[r"HKLM\".len()..]);
            let fix = ps_quote(&serde_json::to_string(self).expect("a fix is plain data"));
            apply = format!("$r = [Microsoft.Win32.Registry]::LocalMachine.CreateSubKey({key}); $r.SetValue('Fix', {fix}); $r.Close()\n{apply}");
            undo = format!(
                "$r = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey({key})\nif (-not $r -or $r.GetValue('Fix') -cne {fix}) {{ throw {} }}\n$r.Close()\n{undo}",
                ps_quote(UNDO_CHANGED)
            );
        }
        if self.needs_admin() || matches!(self, Fix::RemoveBrowserPolicies { .. } | Fix::RemoveUpdateBlock { .. }) {
            let s = ps_quote(&format!("Registry::{store}"));
            undo = format!("{undo}\nif (Test-Path -LiteralPath {s}) {{ Remove-Item -LiteralPath {s} -Recurse }}");
        }
        (apply, undo)
    }

    /// The change itself and its reverse. `store` is where saved copies go.
    fn steps(&self, store: &str) -> (String, String) {
        match self {
            Fix::DisableStartup { name, approved_key } | Fix::EnableStartup { name, approved_key } => {
                let (k, n) = (ps_quote(approved_key), ps_quote(name));
                let set = |v: &str| {
                    format!("if (-not (Test-Path {k})) {{ New-Item {k} -Force | Out-Null }}; Set-ItemProperty -Path {k} -Name {n} -Value {v} -Type Binary")
                };
                if matches!(self, Fix::DisableStartup { .. }) {
                    (set(DISABLED), set(ENABLED))
                } else {
                    (set(ENABLED), set(DISABLED))
                }
            }
            Fix::RemoveExclusion { kind, value } => {
                let (p, v) = (kind.param(), ps_quote(value));
                (format!("Remove-MpPreference -{p} {v}"), format!("Add-MpPreference -{p} {v}"))
            }
            Fix::EnablePua => ("Set-MpPreference -PUAProtection Enabled".into(), "Set-MpPreference -PUAProtection Disabled".into()),
            Fix::DisableRemoteAssistance => {
                let k = r"'HKLM:\SYSTEM\CurrentControlSet\Control\Remote Assistance'";
                (
                    format!("Set-ItemProperty {k} -Name fAllowToGetHelp -Value 0 -Type DWord"),
                    format!("Set-ItemProperty {k} -Name fAllowToGetHelp -Value 1 -Type DWord"),
                )
            }
            Fix::DisableFirewallRule { name, .. } => {
                let n = ps_quote(name);
                // -Name treats [ ] * ? as wildcards, so the rule is matched exactly instead.
                let rule = format!("Get-NetFirewallRule | Where-Object Name -CEQ {n}");
                (format!("{rule} | Disable-NetFirewallRule"), format!("{rule} | Enable-NetFirewallRule"))
            }
            Fix::ServiceManual { name, .. } => {
                let n = ps_quote(name);
                (format!("Set-Service -Name {n} -StartupType Manual"), format!("Set-Service -Name {n} -StartupType Automatic"))
            }
            Fix::DisableSmb1 => (
                "Disable-WindowsOptionalFeature -Online -FeatureName SMB1Protocol -NoRestart | Out-Null".into(),
                "Enable-WindowsOptionalFeature -Online -FeatureName SMB1Protocol -NoRestart | Out-Null".into(),
            ),
            Fix::EnableSystemProtection => {
                ("Enable-ComputerRestore -Drive \"$env:SystemDrive\\\"".into(), "Disable-ComputerRestore -Drive \"$env:SystemDrive\\\"".into())
            }
            Fix::AllowAutoUpdates => {
                let k = r"'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU'";
                (format!("Remove-ItemProperty -Path {k} -Name NoAutoUpdate"), format!("Set-ItemProperty -Path {k} -Name NoAutoUpdate -Value 1 -Type DWord"))
            }
            Fix::EnableService { name, display, start } => {
                let n = ps_quote(name);
                let apply = match start.as_str() {
                    "Delayed" => {
                        format!("{NATIVE}if (Native {{ sc.exe config {n} start= delayed-auto }}) {{ throw {} }}", ps_quote(&format!("Windows would not change {display}")))
                    }
                    "Manual" => format!("Set-Service -Name {n} -StartupType Manual"),
                    _ => format!("Set-Service -Name {n} -StartupType Automatic"),
                };
                (apply, format!("Set-Service -Name {n} -StartupType Disabled"))
            }
            // Only the Guest and built-in Administrator accounts, the two the check-up offers to switch
            // off, and never the account signed in now, whatever a changed file asks for.
            Fix::DisableAccount { name } => {
                let guard = format!(
                    "$u = Get-LocalUser -Name {}\nif ($u.SID.Value -notmatch '-50[01]$' -or $u.SID.Value -eq [Security.Principal.WindowsIdentity]::GetCurrent().User.Value) {{ throw 'Bootmend only switches the Guest and built-in Administrator accounts on or off' }}",
                    ps_quote(name)
                );
                (format!("{guard}\nDisable-LocalUser -SID $u.SID"), format!("{guard}\nEnable-LocalUser -SID $u.SID"))
            }
            // With tamper protection on, Windows Security can ignore the change without an error.
            Fix::EnableCloudProtection => (
                "Set-MpPreference -MAPSReporting Advanced\nif ((Get-MpPreference).MAPSReporting -eq 0) { throw \"Windows Security didn't accept the change. Switch it on in Windows Security instead.\" }".into(),
                "Set-MpPreference -MAPSReporting Disabled".into(),
            ),
            Fix::EnableSampleSubmission => (
                "Set-MpPreference -SubmitSamplesConsent SendSafeSamples\nif ((Get-MpPreference).SubmitSamplesConsent -ne 1) { throw \"Windows Security didn't accept the change. Switch it on in Windows Security instead.\" }".into(),
                "Set-MpPreference -SubmitSamplesConsent NeverSend".into(),
            ),
            Fix::EnableTrim => {
                let set = |v: u8, what: &str| format!("{NATIVE}if (Native {{ fsutil behavior set DisableDeleteNotify {v} }}) {{ throw 'Windows would not switch TRIM {what}' }}");
                (set(0, "on"), set(1, "off"))
            }
            Fix::ResumeEncryption { drive } => {
                let d = ps_quote(drive);
                (format!("Resume-BitLocker -MountPoint {d} | Out-Null"), format!("Suspend-BitLocker -MountPoint {d} -RebootCount 0 | Out-Null"))
            }
            // Switching to automatic replaces the list of page files, so the old list is saved for the undo.
            Fix::SystemManagedPagefile => {
                let k = r"'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management'";
                let s = ps_quote(&format!("Registry::{store}"));
                let set = |v: &str| format!("Get-CimInstance Win32_ComputerSystem | Set-CimInstance -Property @{{ AutomaticManagedPagefile = ${v} }}");
                (
                    format!(
                        "Set-ItemProperty -LiteralPath {s} -Name PagingFiles -Value ([string[]]@((Get-ItemProperty {k}).PagingFiles | Where-Object {{ $_ }})) -Type MultiString\n{}",
                        set("true")
                    ),
                    // Read the copy first: if it's gone, the undo stops before anything has changed.
                    format!(
                        "$lines = [string[]](Get-ItemProperty -LiteralPath {s} -Name PagingFiles).PagingFiles\n{}\nSet-ItemProperty {k} -Name PagingFiles -Value $lines -Type MultiString",
                        set("false")
                    ),
                )
            }
            Fix::EnableDriverBlocklist => {
                let k = r"'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Config'";
                let set = |v: u8| format!("if (-not (Test-Path {k})) {{ New-Item {k} -Force | Out-Null }}\nSet-ItemProperty {k} -Name VulnerableDriverBlocklistEnable -Value {v} -Type DWord");
                (set(1), set(0))
            }
            // 2 switches it on without locking it into the firmware, as Windows Security's switch does.
            Fix::EnableLsaProtection => {
                let k = r"'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa'";
                (
                    format!("Set-ItemProperty {k} -Name RunAsPPL -Value 2 -Type DWord\nSet-ItemProperty {k} -Name RunAsPPLBoot -Value 2 -Type DWord"),
                    format!(
                        "Set-ItemProperty {k} -Name RunAsPPL -Value 0 -Type DWord\nif ((Get-ItemProperty {k}).PSObject.Properties.Name -contains 'RunAsPPLBoot') {{ Remove-ItemProperty {k} -Name RunAsPPLBoot }}"
                    ),
                )
            }
            Fix::EnableCrashFiles => {
                let set = |v: u8| format!(r"Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl' -Name CrashDumpEnabled -Value {v} -Type DWord");
                (set(7), set(0))
            }
            Fix::TurnOffProxy { enabled, pac, winhttp } => {
                let k = r"'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'";
                let (mut apply, mut undo) = (vec![], vec![]);
                if *enabled {
                    apply.push(format!("Set-ItemProperty {k} -Name ProxyEnable -Value 0 -Type DWord"));
                    undo.push(format!("Set-ItemProperty {k} -Name ProxyEnable -Value 1 -Type DWord"));
                }
                if !pac.is_empty() {
                    apply.push(format!("Remove-ItemProperty {k} -Name AutoConfigURL"));
                    undo.push(format!("Set-ItemProperty {k} -Name AutoConfigURL -Value {}", ps_quote(pac)));
                }
                if !winhttp.is_empty() {
                    apply.push(format!("{NATIVE}if (Native {{ netsh.exe winhttp reset proxy }}) {{ throw 'Windows would not reset its own proxy' }}"));
                    undo.push(format!(
                        "{NATIVE}if (Native {{ netsh.exe winhttp set proxy {} }}) {{ throw 'Windows would not set its own proxy back' }}",
                        ps_quote(&format!("proxy-server={winhttp}"))
                    ));
                }
                (apply.join("\n"), undo.join("\n"))
            }
            Fix::DnsAutomatic { adapter, servers } => {
                let a = ps_quote(adapter);
                let list = servers.iter().map(|x| ps_quote(x)).collect::<Vec<_>>().join(",");
                (format!("Set-DnsClientServerAddress -InterfaceAlias {a} -ResetServerAddresses"), format!("Set-DnsClientServerAddress -InterfaceAlias {a} -ServerAddresses {list}"))
            }
            Fix::RemoveBrowserPolicies { key, settings, .. } => remove_policies(store, key, settings),
            Fix::EnableRecovery => (
                format!("{NATIVE}if (Native {{ reagentc.exe /enable }}) {{ throw 'Windows could not switch its recovery tools back on' }}"),
                format!("{NATIVE}if (Native {{ reagentc.exe /disable }}) {{ throw 'Windows could not switch its recovery tools off' }}"),
            ),
            Fix::RemoveUpdateBlock { key, settings } => remove_policies(store, key, settings),
            // Microsoft's order: DISM mends the store sfc copies from. sfc's exit code means
            // nothing, so Windows' own health record has the last word. 3010 asks for a restart.
            Fix::RepairWindowsFiles => (
                format!(
                    r#"{NATIVE}$c = Native {{ DISM.exe /Online /Cleanup-Image /RestoreHealth }}
if ($c -in -2146498529, -2146498298, -2146498283) {{ throw "Windows couldn't download fresh copies of its files. Check the internet connection, then try again." }}
if ($c -ne 0 -and $c -ne 3010) {{ throw ("Windows couldn't repair its files (error 0x{{0:X8}})" -f $c) }}
$null = Native {{ sfc.exe /scannow }}
if ("$((Repair-WindowsImage -Online -CheckHealth).ImageHealthState)" -ne 'Healthy') {{ throw "Windows repaired what it could, but some of its files are still damaged." }}"#
                ),
                String::new(),
            ),
            // Device Manager's "Roll Back Driver", without its questions. Windows keeps one earlier
            // driver per device; 259 (ERROR_NO_MORE_ITEMS) means it has none.
            Fix::RollBackDriver { device, .. } => (ROLL_BACK.replace("{DEVICE}", &ps_quote(device)), String::new()),
        }
    }
}

const ROLL_BACK: &str = r#"Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class BootmendRollBack {
    [StructLayout(LayoutKind.Sequential)]
    struct DevInfo { public int Size; public Guid Class; public int Inst; public IntPtr Reserved; }
    [DllImport("setupapi.dll", SetLastError = true)]
    static extern IntPtr SetupDiCreateDeviceInfoList(IntPtr cls, IntPtr hwnd);
    [DllImport("setupapi.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool SetupDiOpenDeviceInfoW(IntPtr set, string id, IntPtr hwnd, int flags, ref DevInfo info);
    [DllImport("setupapi.dll")]
    static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);
    [DllImport("newdev.dll", SetLastError = true)]
    static extern bool DiRollbackDriver(IntPtr set, ref DevInfo info, IntPtr hwnd, int flags, out bool restart);
    // 0 when the earlier driver is back, else the Windows error code.
    public static int Run(string id) {
        IntPtr set = SetupDiCreateDeviceInfoList(IntPtr.Zero, IntPtr.Zero);
        if (set == new IntPtr(-1)) return Marshal.GetLastWin32Error();
        try {
            DevInfo info = new DevInfo();
            info.Size = Marshal.SizeOf(info);
            if (!SetupDiOpenDeviceInfoW(set, id, IntPtr.Zero, 0, ref info)) return Marshal.GetLastWin32Error();
            bool restart;
            // 1 is ROLLBACK_FLAG_NO_UI.
            return DiRollbackDriver(set, ref info, IntPtr.Zero, 1, out restart) ? 0 : Marshal.GetLastWin32Error();
        } finally { SetupDiDestroyDeviceInfoList(set); }
    }
}
'@
$e = [BootmendRollBack]::Run({DEVICE})
if ($e -eq 259) { throw "Windows no longer has the earlier driver, so it can't go back to it" }
if ($e -eq -536870389) { throw "That device isn't plugged in any more" }
if ($e -ne 0) { throw ("Windows couldn't go back to the earlier driver (error {0})" -f $e) }"#;

/// Saves a copy of a policy key in the change's record, then removes the named settings from it:
/// values, or subkeys (lists, Firefox's groups). A name with a backslash is in a subkey
/// ("AU\UseWUServer"). Undo copies the saved key back. The copy sits in the same hive as the key,
/// so a copy of machine-wide settings is one only administrators can change.
fn remove_policies(store: &str, key: &str, settings: &[String]) -> (String, String) {
    let copy = ps_quote(&format!(r"{store}\Backup"));
    let (reg, path) = (ps_quote(key), ps_quote(&format!("Registry::{key}")));
    let names = settings.iter().map(|x| ps_quote(x)).collect::<Vec<_>>().join(", ");
    let apply = format!(
        r#"{NATIVE}if (Native {{ reg.exe copy {reg} {copy} /s /f }}) {{ throw 'Could not save a copy of the settings first, so nothing was changed' }}
$p = {path}
foreach ($n in @({names})) {{
  $i = $n.LastIndexOf('\'); $k = $(if ($i -ge 0) {{ "$p\$($n.Substring(0, $i))" }} else {{ $p }}); $v = $n.Substring($i + 1)
  if (Test-Path -LiteralPath "$k\$v") {{ Remove-Item -LiteralPath "$k\$v" -Recurse }}
  if ((Get-ItemProperty -LiteralPath $k).PSObject.Properties.Name -contains $v) {{ Remove-ItemProperty -LiteralPath $k -Name $v }}
}}"#
    );
    let undo = format!("{NATIVE}if (Native {{ reg.exe copy {copy} {reg} /s /f }}) {{ throw 'Could not put the settings back from the saved copy' }}");
    (apply, undo)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub when: String,
    pub title: String,
    pub fix: Fix,
    /// Names the change's record and saved copies. The undo is rebuilt from `fix`, never read from
    /// the file, which any app the user runs can change.
    #[serde(default)]
    pub id: String,
    pub undone: Option<String>,
}

/// A new name for a change: when it was made, and a random part.
fn new_id() -> String {
    format!("{}-{:012x}", chrono::Local::now().format("%Y%m%d%H%M%S"), crate::util::unique() & 0xFFFF_FFFF_FFFF)
}

/// Letters, digits and dashes only, so a changed History can't point at another key.
fn id_ok(id: &str) -> bool {
    (1..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Removes the records and saved copies kept for undoing changes. Those for changes made as
/// administrator can only be removed from an administrator window.
pub fn forget_changes() -> Result<()> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    delete_key(HKEY_CURRENT_USER, r"SOFTWARE\Bootmend")?;
    if crate::sys::elevated() {
        delete_key(HKEY_LOCAL_MACHINE, r"SOFTWARE\Bootmend")?;
    }
    Ok(())
}

/// Deletes a key and everything in it. A key that isn't there is already gone.
fn delete_key(root: windows::Win32::System::Registry::HKEY, key: &str) -> Result<()> {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::RegDeleteTreeW;
    // SAFETY: a predefined root key and a string Windows only reads.
    let r = unsafe { RegDeleteTreeW(root, &windows::core::HSTRING::from(key)) };
    if r == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    r.ok().with_context(|| format!("removing {key} from the registry"))
}

fn history_path() -> Result<std::path::PathBuf> {
    Ok(data_dir("")?.join("history.json"))
}

pub fn history() -> Vec<HistoryEntry> {
    history_path().and_then(|p| read_history(&p)).unwrap_or_else(|e| {
        log!("error", "couldn't read History: {e:#}");
        vec![]
    })
}

/// History as a change starts. If it can't be read, nothing changes, since saving would wipe the undo steps in it.
fn history_for_change(what: &str) -> Result<Vec<HistoryEntry>> {
    history_path().and_then(|p| read_history(&p)).map_err(|e| {
        log!("history", "{what} stopped because History couldn't be read: {e:#}");
        anyhow!("Bootmend couldn't read History, so nothing was changed")
    })
}

/// No file means nothing has been changed yet. A file that isn't valid is moved aside, so its undo
/// steps can still be recovered. Anything else, such as another program holding the file, is an error.
fn read_history(p: &Path) -> Result<Vec<HistoryEntry>> {
    let b = match std::fs::read(p) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e).with_context(|| format!("reading {}", p.display())),
    };
    serde_json::from_slice(&b).or_else(|e| {
        let aside = p.with_file_name(format!("history-unreadable-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        std::fs::rename(p, &aside).with_context(|| format!("{} isn't valid ({e}) and couldn't be moved aside", p.display()))?;
        log!("history", "history file unreadable ({e}); kept it as {} and started a new one", aside.display());
        Ok(vec![])
    })
}

fn save_history(h: &[HistoryEntry]) -> Result<()> {
    write_history(&history_path()?, h)
}

/// Written to a temporary file first, so a crash or power cut mid-save never leaves a half-written history.
fn write_history(p: &Path, h: &[HistoryEntry]) -> Result<()> {
    let tmp = p.with_extension("json.tmp");
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut f, &serde_json::to_vec_pretty(h)?)?;
        // On disk before the swap, or a power cut could leave the new name pointing at nothing.
        f.sync_all()
    };
    write().with_context(|| format!("saving {}", tmp.display()))?;
    std::fs::rename(&tmp, p).with_context(|| format!("saving {}", p.display()))
}

fn run_ps(body: &str) -> Result<()> {
    ps_output(body, Duration::from_secs(300)).map(|_| ())
}

/// Runs `body` and returns what it printed, or the reason it stopped.
fn ps_output(body: &str, timeout: Duration) -> Result<String> {
    let script =
        format!("$ErrorActionPreference = 'Stop'\ntry {{\n{body}\n'BOOTMEND-OK'\n}} catch {{ Write-Output ('BOOTMEND-ERR ' + $_.Exception.Message); exit 1 }}");
    let out = powershell(&script, timeout)?;
    if out.stdout.contains("BOOTMEND-OK") {
        return Ok(out.stdout);
    }
    let msg = out
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("BOOTMEND-ERR "))
        .map(str::trim)
        .unwrap_or(if out.timed_out { "it took too long" } else { "Windows did not say why" })
        .to_string();
    Err(anyhow!("{msg}"))
}

/// Apply a fix and record how to undo it.
pub fn apply(fix: &Fix, title: &str) -> Result<()> {
    apply_with(fix, title, history_for_change("a fix")?, &mut |h| save_history(h), &|s| ps_output(s, fix.timeout()).map(|_| ()))
}

type Save<'a> = &'a mut dyn FnMut(&[HistoryEntry]) -> Result<()>;
type Run<'a> = &'a dyn Fn(&str) -> Result<()>;

/// The undo step is saved before anything changes, so Bootmend never makes a change it can't take back.
fn apply_with(fix: &Fix, title: &str, mut h: Vec<HistoryEntry>, save: Save, run: Run) -> Result<()> {
    if !fix.check() {
        log!("fix", "\"{}\" wasn't run because its details aren't ones the check-up gives", title);
        return Err(anyhow!(APPLY_CHANGED));
    }
    let id = new_id();
    let (apply, _) = fix.scripts(&id);
    log!("fix", "applying \"{}\"", title);
    let t0 = Instant::now();
    h.push(HistoryEntry { when: crate::util::now_stamp(), title: title.to_string(), fix: fix.clone(), id, undone: None });
    save(&h).map_err(|e| {
        log!("fix", "\"{}\" wasn't run because its undo step couldn't be saved: {e:#}", title);
        anyhow!("Bootmend couldn't save how to undo it, so nothing was changed")
    })?;
    if let Err(e) = run(&apply) {
        log!("fix", "\"{}\" failed: {e:#}", title);
        h.pop();
        if let Err(s) = save(&h) {
            log!("fix", "\"{}\" failed, and its undo step couldn't be taken out of History: {s:#}", title);
        }
        return Err(e);
    }
    log!("fix", "\"{}\" done in {:.1}s", title, t0.elapsed().as_secs_f32());
    Ok(())
}

/// Save a System Restore point. Windows normally allows one a day, so this checks that one was really made.
pub fn restore_point(description: &str) -> Result<()> {
    let d = ps_quote(description);
    log!("restore", "creating a restore point");
    let t0 = Instant::now();
    let r = run_ps(&format!(
        "$t = Get-Date\nCheckpoint-Computer -Description {d} -RestorePointType MODIFY_SETTINGS -WarningAction SilentlyContinue\n$l = Get-ComputerRestorePoint | Select-Object -Last 1\n$w = if ($l) {{ [Management.ManagementDateTimeConverter]::ToDateTime($l.CreationTime) }} else {{ $null }}\nif (-not $w -or $w -lt $t.AddHours(-24)) {{ throw \"Windows didn't save a restore point. System Restore may be off for this drive.\" }}\nif ($w -lt $t.AddMinutes(-1)) {{ throw 'Windows skipped it because it already saved a restore point in the last 24 hours' }}"
    ));
    match &r {
        Ok(()) => log!("restore", "restore point saved in {:.1}s", t0.elapsed().as_secs_f32()),
        Err(e) => log!("restore", "restore point failed: {e:#}"),
    }
    r
}

/// A point Windows can take the PC back to.
#[derive(Debug, Clone, PartialEq)]
pub struct RestorePoint {
    /// "yyyy-MM-dd HH:mm"
    pub when: String,
    pub what: String,
}

/// Windows' restore points, newest first. Only administrators can read them.
pub fn restore_points() -> Result<Vec<RestorePoint>> {
    let out = ps_output(
        "Get-ComputerRestorePoint | ForEach-Object { ([Management.ManagementDateTimeConverter]::ToDateTime($_.CreationTime)).ToString('yyyy-MM-dd HH:mm') + \"`t\" + $_.Description }",
        Duration::from_secs(60),
    )?;
    Ok(parse_points(&out))
}

fn parse_points(text: &str) -> Vec<RestorePoint> {
    let mut v: Vec<RestorePoint> = text
        .lines()
        .filter_map(|l| l.trim_end().split_once('\t'))
        .filter(|(w, _)| chrono::NaiveDateTime::parse_from_str(w, "%Y-%m-%d %H:%M").is_ok())
        .map(|(w, d)| RestorePoint { when: w.to_string(), what: d.trim().to_string() })
        .collect();
    v.sort_by(|a, b| b.when.cmp(&a.when));
    v
}

/// Undo the history entry at `index`.
pub fn undo(index: usize) -> Result<()> {
    undo_with(index, history_for_change("an undo")?, &mut |h| save_history(h), &run_ps)
}

/// Recorded as undone before the undo runs, so History never offers to undo a change twice.
fn undo_with(index: usize, mut h: Vec<HistoryEntry>, save: Save, run: Run) -> Result<()> {
    let e = h.get_mut(index).ok_or_else(|| anyhow!("that change is no longer in the history"))?;
    if e.undone.is_some() {
        return Err(anyhow!("that change was already undone"));
    }
    if !e.fix.undoable() {
        return Err(anyhow!("there's nothing to undo for that change"));
    }
    if !id_ok(&e.id) || !e.fix.check() {
        log!("undo", "\"{}\" wasn't undone because its entry in History was altered", e.title);
        return Err(anyhow!(UNDO_CHANGED));
    }
    let (title, script) = (e.title.clone(), e.fix.scripts(&e.id).1);
    log!("undo", "undoing \"{}\"", title);
    e.undone = Some(crate::util::now_stamp());
    save(&h).map_err(|e| {
        log!("undo", "\"{}\" wasn't undone because History couldn't be updated: {e:#}", title);
        anyhow!("Bootmend couldn't update History, so nothing was changed")
    })?;
    if let Err(e) = run(&script) {
        log!("undo", "\"{}\" failed: {e:#}", title);
        h[index].undone = None;
        if let Err(s) = save(&h) {
            log!("undo", "\"{}\" failed, and History still shows it as undone: {s:#}", title);
        }
        return Err(e);
    }
    log!("undo", "\"{}\" undone", title);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn steam() -> Fix {
        Fix::DisableStartup { name: "Steam".into(), approved_key: STARTUP_APPROVED[0].into() }
    }

    fn applied() -> Vec<HistoryEntry> {
        vec![HistoryEntry { when: "2026-11-15 18:00".into(), title: "Steam".into(), fix: steam(), id: "20261115180000-abc".into(), undone: None }]
    }

    /// Details a changed History or queue file could hold, none of which the check-up ever gives.
    fn tampered() -> Vec<Fix> {
        let policies = |key: &str, settings: &[&str]| Fix::RemoveBrowserPolicies {
            browser: "Chrome".into(),
            key: key.into(),
            settings: settings.iter().map(|s| s.to_string()).collect(),
        };
        vec![
            Fix::DisableStartup { name: "x".into(), approved_key: r"HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\x".into() },
            Fix::EnableStartup { name: "a\nb".into(), approved_key: STARTUP_APPROVED[1].into() },
            Fix::RemoveExclusion { kind: ExclusionKind::Path, value: String::new() },
            Fix::ServiceManual { name: r"..\WinDefend".into(), display: "x".into() },
            Fix::EnableService { name: "wuauserv".into(), display: "x".into(), start: "Disabled".into() },
            Fix::DisableAccount { name: "*".into() },
            Fix::DisableAccount { name: "a,b".into() },
            Fix::ResumeEncryption { drive: r"C:\Windows".into() },
            Fix::DnsAutomatic { adapter: "Wi-Fi".into(), servers: vec!["dns.example".into()] },
            Fix::DnsAutomatic { adapter: "Wi-Fi".into(), servers: vec![] },
            Fix::TurnOffProxy { enabled: true, pac: "a\u{0}b".into(), winhttp: String::new() },
            policies(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run", &["x"]),
            policies(r"HKLM\SOFTWARE\Policies\..\Microsoft\Windows\CurrentVersion\Run", &["x"]),
            policies(r"HKLM\SOFTWARE\Policies\", &["x"]),
            policies(r"HKLM\SOFTWARE\Policies\Google\Chrome", &[r"..\..\Microsoft"]),
            policies(r"HKLM\SOFTWARE\Policies\Google\Chrome", &[r"a\\b"]),
            policies(r"HKLM\SOFTWARE\Policies\Google\Chrome", &[]),
            Fix::RemoveUpdateBlock { key: r"HKLM\SYSTEM\CurrentControlSet\Services".into(), settings: vec!["x".into()] },
            Fix::RollBackDriver { device: String::new(), name: "x".into() },
        ]
    }

    #[test]
    fn only_fixes_shaped_like_the_check_ups_own_are_accepted() {
        for fx in every_fix() {
            assert!(fx.check(), "{fx:?}");
        }
        for key in STARTUP_APPROVED {
            assert!(Fix::EnableStartup { name: "App".into(), approved_key: key.to_lowercase() }.check(), "{key}");
        }
        assert!(Fix::RemoveBrowserPolicies {
            browser: "Firefox".into(),
            key: r"HKCU\SOFTWARE\Policies\Mozilla\Firefox".into(),
            settings: vec!["Homepage".into()]
        }
        .check());
        for fx in tampered() {
            assert!(!fx.check(), "{fx:?}");
        }
    }

    #[test]
    fn a_tampered_fix_is_never_run() {
        for fx in tampered() {
            let (ran, saved) = (Cell::new(false), Cell::new(false));
            let r = apply_with(
                &fx,
                "x",
                vec![],
                &mut |_| {
                    saved.set(true);
                    Ok(())
                },
                &|_| {
                    ran.set(true);
                    Ok(())
                },
            );
            assert_eq!(r.unwrap_err().to_string(), APPLY_CHANGED, "{fx:?}");
            assert!(!ran.get() && !saved.get(), "{fx:?}");
        }
    }

    /// History is a file any app the user runs can change, so the undo comes from the fix itself.
    #[test]
    fn an_undo_is_rebuilt_from_the_fix_never_read_from_history() {
        let json = r#"[{"when":"2026-11-15 18:00","title":"Steam","fix":{"DisableStartup":{"name":"Steam","approved_key":"HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run"}},"id":"20261115180000-abc","undo_script":"Start-Process evil.exe","undone":null}]"#;
        let h: Vec<HistoryEntry> = serde_json::from_str(json).unwrap();
        let script = std::cell::RefCell::new(String::new());
        undo_with(0, h, &mut |_| Ok(()), &|s| {
            *script.borrow_mut() = s.to_string();
            Ok(())
        })
        .unwrap();
        assert_eq!(*script.borrow(), steam().scripts("20261115180000-abc").1);
        assert!(!script.borrow().contains("evil"));
        assert!(!serde_json::to_string(&applied()).unwrap().contains("undo_script"));
    }

    #[test]
    fn a_tampered_history_entry_is_never_undone() {
        let mut entries: Vec<HistoryEntry> = tampered().into_iter().filter(Fix::undoable).map(|fix| HistoryEntry { fix, ..applied().remove(0) }).collect();
        for id in ["", r"x\..\y", "x y", &"a".repeat(41)] {
            entries.push(HistoryEntry { id: id.to_string(), ..applied().remove(0) });
        }
        for e in entries {
            let ran = Cell::new(false);
            let r = undo_with(0, vec![e.clone()], &mut |_| Ok(()), &|_| {
                ran.set(true);
                Ok(())
            });
            assert_eq!(r.unwrap_err().to_string(), UNDO_CHANGED, "{e:?}");
            assert!(!ran.get(), "{e:?}");
        }
    }

    /// A change made as administrator records the fix where only administrators can write, and its
    /// undo checks that record first, so changing History can't change what an undo does.
    #[test]
    fn an_administrator_change_is_undone_only_while_its_record_matches() {
        let fx = Fix::RemoveExclusion { kind: ExclusionKind::Path, value: "C:\\it's here".into() };
        let (a, u) = fx.scripts("20261121-ab");
        let record = ps_quote(&serde_json::to_string(&fx).unwrap());
        assert!(
            a.starts_with(r"$r = [Microsoft.Win32.Registry]::LocalMachine.CreateSubKey('SOFTWARE\Bootmend\Changes\20261121-ab'); $r.SetValue('Fix', "),
            "{a}"
        );
        assert!(a.contains(&record) && a.ends_with("Remove-MpPreference -ExclusionPath 'C:\\it''s here'"), "{a}");
        let (check, change) = (u.find(&format!("-cne {record}")).unwrap(), u.find("Add-MpPreference").unwrap());
        assert!(check < change && u.contains(&ps_quote(UNDO_CHANGED)), "the record is checked before anything changes: {u}");
        assert!(u.trim_end().ends_with(r"Remove-Item -LiteralPath 'Registry::HKLM\SOFTWARE\Bootmend\Changes\20261121-ab' -Recurse }"), "{u}");
        // Per-user changes need no record, and nothing to undo needs none either.
        let (a, u) = steam().scripts("x");
        assert!(!a.contains("Bootmend") && !u.contains("Bootmend"), "{a} / {u}");
        assert!(!Fix::RepairWindowsFiles.scripts("x").0.contains("Bootmend"));
    }

    #[test]
    fn the_account_fix_only_touches_guest_and_the_built_in_administrator() {
        let (a, u) = Fix::DisableAccount { name: "Guest".into() }.scripts("x");
        for s in [&a, &u] {
            let guard = s.find("-notmatch '-50[01]$'").expect("guarded");
            let act = s.find("-LocalUser -SID $u.SID").expect("acts on the account checked");
            assert!(guard < act && s.contains("GetCurrent().User.Value"), "{s}");
        }
        assert!(a.contains("Get-LocalUser -Name 'Guest'") && a.contains("Disable-LocalUser") && u.contains("Enable-LocalUser"), "{a} / {u}");
    }

    /// Runs the record lines of a change made as administrator against a stand-in key under HKCU:
    /// the undo goes ahead while the record matches, and stops when it was altered or removed.
    #[test]
    #[ignore = "writes to the registry; run with --ignored"]
    fn the_record_check_stops_an_undo_whose_record_was_altered() {
        let fx = Fix::RemoveExclusion { kind: ExclusionKind::Path, value: r"C:\Games 'n' more".into() };
        let (a, u) = fx.scripts("20261121-ab");
        let stand_in = format!(r"Software\bootmend-selftest-record-{:x}", crate::util::unique());
        let here = |s: &str| s.replace("LocalMachine", "CurrentUser").replace(r"SOFTWARE\Bootmend\Changes", &stand_in);
        let record = here(a.lines().next().unwrap());
        let check = here(&u.lines().take(3).collect::<Vec<_>>().join("\n"));
        assert!(record.contains(&stand_in) && check.contains(&stand_in) && check.contains("-cne"), "{record}\n{check}");
        let key = ps_quote(&format!(r"HKCU:\{stand_in}\20261121-ab"));
        run_ps(&record).unwrap();
        let matching = run_ps(&check);
        run_ps(&format!("Set-ItemProperty -LiteralPath {key} -Name Fix -Value ((Get-ItemProperty -LiteralPath {key}).Fix -replace 'Games', 'Malware')"))
            .unwrap();
        let altered = run_ps(&check);
        run_ps(&format!("Remove-Item -LiteralPath {} -Recurse", ps_quote(&format!(r"HKCU:\{stand_in}")))).unwrap();
        let missing = run_ps(&check);
        assert!(matching.is_ok(), "{matching:?}");
        assert_eq!(altered.unwrap_err().to_string(), UNDO_CHANGED);
        assert_eq!(missing.unwrap_err().to_string(), UNDO_CHANGED);
    }

    #[test]
    #[ignore = "writes to the registry; run with --ignored"]
    fn removing_bootmends_records_leaves_nothing_behind() {
        use windows::Win32::System::Registry::HKEY_CURRENT_USER;
        let key = format!(r"Software\bootmend-selftest-records-{}", crate::util::unique());
        run_ps(&format!("New-Item {} -Force | Out-Null", ps_quote(&format!(r"HKCU:\{key}\Changes\x\Backup")))).unwrap();
        delete_key(HKEY_CURRENT_USER, &key).unwrap();
        let left = powershell(&format!("Test-Path {}", ps_quote(&format!(r"HKCU:\{key}"))), Duration::from_secs(60)).unwrap();
        assert_eq!(left.stdout.trim(), "False");
        delete_key(HKEY_CURRENT_USER, &key).unwrap();
    }

    #[test]
    fn an_unreadable_history_is_kept_aside_not_overwritten() {
        let d = std::env::temp_dir().join(format!("bootmend-history-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("history.json");
        std::fs::write(&p, "[{\"when\": \"2026-11-1").unwrap();
        let read = read_history(&p).unwrap();
        let aside: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().path()).collect();
        let kept = aside.iter().find(|f| f.file_name().unwrap().to_string_lossy().starts_with("history-unreadable-")).map(std::fs::read_to_string);
        write_history(&p, &applied()).unwrap();
        let back = read_history(&p).unwrap();
        let files = std::fs::read_dir(&d).unwrap().count();
        std::fs::remove_dir_all(&d).unwrap();
        assert!(read.is_empty());
        assert_eq!(kept.unwrap().unwrap(), "[{\"when\": \"2026-11-1", "the unreadable file is kept, untouched");
        assert_eq!(back.len(), 1);
        assert_eq!(files, 2, "the new history and the kept copy, with no temporary file left behind");
    }

    /// Windows removes the newer driver, so History lists the change with nothing to undo, and it
    /// never joins the safe ones.
    #[test]
    fn going_back_to_an_earlier_driver_names_the_device_and_has_nothing_to_undo() {
        let fx = Fix::RollBackDriver { device: r"PCI\VEN_1002&DEV_73DF\6&15bb'039d".into(), name: "AMD Radeon RX 6750 XT".into() };
        let (a, u) = fx.scripts("x");
        assert!(a.contains(r"[BootmendRollBack]::Run('PCI\VEN_1002&DEV_73DF\6&15bb''039d')"), "{a}");
        assert!(
            a.contains("DiRollbackDriver(set, ref info, IntPtr.Zero, 1,") && a.contains("259"),
            "no questions, and plain words when there's nothing to go back to: {a}"
        );
        assert!(u.is_empty() && !fx.undoable() && fx.needs_admin() && !fx.safe() && Fix::RepairWindowsFiles.safe());
        assert_eq!(fx.up_to_minutes(), Some(5));
        assert!(fx.describe().contains("AMD Radeon RX 6750 XT") && fx.doing().contains("AMD Radeon RX 6750 XT"));
    }

    /// It only puts back Windows' own copies, so History lists it with nothing to undo.
    #[test]
    fn repairing_windows_files_is_listed_but_has_nothing_to_undo() {
        let fx = Fix::RepairWindowsFiles;
        let (a, u) = fx.scripts("x");
        let (dism, sfc, check) = (a.find("/RestoreHealth").unwrap(), a.find("sfc.exe /scannow").unwrap(), a.find("-CheckHealth").unwrap());
        assert!(dism < sfc && sfc < check, "DISM first, then sfc, then Windows' own verdict: {a}");
        assert!(a.contains("3010") && a.contains("-2146498529"), "a restart-needed result is fine; missing source files get plain words: {a}");
        assert!(u.is_empty() && !fx.undoable() && fx.needs_admin());
        assert!(every_fix().iter().filter(|f| **f != fx && !matches!(f, Fix::RollBackDriver { .. })).all(Fix::undoable));
        assert!(fx.timeout() >= Duration::from_secs(45 * 60) && Fix::EnablePua.timeout() == Duration::from_secs(300));
        assert_eq!((fx.up_to_minutes(), Fix::EnablePua.up_to_minutes()), (Some(30), None));

        let mut saved = vec![];
        apply_with(
            &fx,
            "Some of Windows' own files are damaged",
            vec![],
            &mut |h| {
                saved = h.to_vec();
                Ok(())
            },
            &|_| Ok(()),
        )
        .unwrap();
        assert_eq!(saved.len(), 1, "listed in History");
        let ran = Cell::new(false);
        let r = undo_with(0, saved.clone(), &mut |_| Ok(()), &|_| {
            ran.set(true);
            Ok(())
        });
        assert!(r.is_err() && !ran.get(), "nothing to undo, so nothing runs");
    }

    #[test]
    fn restore_points_are_listed_newest_first() {
        let out =
            "2026-10-02 09:15\tWindows Update\r\n2026-11-16 13:42\tSaved with Bootmend\r\n\r\nnot a point\r\n2026-11-01 18:00\tInstalled Steam\t(app)\r\n";
        let points = parse_points(out);
        let got: Vec<_> = points.iter().map(|p| (p.when.as_str(), p.what.as_str())).collect();
        assert_eq!(got, [("2026-11-16 13:42", "Saved with Bootmend"), ("2026-11-01 18:00", "Installed Steam\t(app)"), ("2026-10-02 09:15", "Windows Update")]);
        assert!(parse_points("").is_empty());
    }

    #[test]
    fn a_history_that_cant_be_opened_is_an_error_not_an_empty_one() {
        let d = std::env::temp_dir().join(format!("bootmend-history-locked-{}", std::process::id()));
        std::fs::create_dir_all(d.join("history.json")).unwrap();
        // A folder in the file's place can't be read, like a file another program has locked.
        let held = read_history(&d.join("history.json"));
        let missing = read_history(&d.join("nothing-yet.json"));
        std::fs::remove_dir_all(&d).unwrap();
        assert!(held.is_err(), "{held:?}");
        assert!(missing.unwrap().is_empty(), "no file yet just means nothing has been changed");
    }

    #[test]
    fn a_fix_whose_undo_step_cannot_be_saved_is_never_run() {
        let ran = Cell::new(false);
        let r = apply_with(&steam(), "Steam", vec![], &mut |_| Err(anyhow!("disk full")), &|_| {
            ran.set(true);
            Ok(())
        });
        assert!(r.is_err());
        assert!(!ran.get(), "nothing may change when it couldn't be undone");
    }

    #[test]
    fn a_fix_that_fails_leaves_nothing_to_undo() {
        // What's on disk, starting from the history passed in.
        let mut saved = vec![];
        let r = apply_with(
            &steam(),
            "Steam",
            vec![],
            &mut |h| {
                saved = h.to_vec();
                Ok(())
            },
            &|_| Err(anyhow!("access denied")),
        );
        assert!(r.is_err());
        assert!(saved.is_empty());
    }

    #[test]
    fn a_fix_that_works_can_be_undone_from_history() {
        let mut saved = vec![];
        apply_with(
            &steam(),
            "Steam",
            vec![],
            &mut |h| {
                saved = h.to_vec();
                Ok(())
            },
            &|_| Ok(()),
        )
        .unwrap();
        assert_eq!(saved.len(), 1);
        assert!(saved[0].undone.is_none() && id_ok(&saved[0].id));
    }

    #[test]
    fn an_undo_that_cannot_be_recorded_is_never_run() {
        let ran = Cell::new(false);
        let r = undo_with(0, applied(), &mut |_| Err(anyhow!("disk full")), &|_| {
            ran.set(true);
            Ok(())
        });
        assert!(r.is_err());
        assert!(!ran.get(), "History would still offer an undo that already happened");
    }

    #[test]
    fn an_undo_that_fails_can_be_tried_again() {
        let mut saved = applied();
        let r = undo_with(
            0,
            applied(),
            &mut |h| {
                saved = h.to_vec();
                Ok(())
            },
            &|_| Err(anyhow!("access denied")),
        );
        assert!(r.is_err());
        assert!(saved[0].undone.is_none());
    }

    #[test]
    fn an_undo_that_works_is_marked_done() {
        let mut saved = vec![];
        undo_with(
            0,
            applied(),
            &mut |h| {
                saved = h.to_vec();
                Ok(())
            },
            &|s| if s == steam().scripts("20261115180000-abc").1 { Ok(()) } else { Err(anyhow!("wrong script")) },
        )
        .unwrap();
        assert!(saved[0].undone.is_some());
    }

    #[test]
    fn startup_scripts_flip_the_approved_flag() {
        let f = steam();
        let (a, u) = f.scripts("x");
        assert!(a.contains("(3,0,") && u.contains("(2,0,"));
        assert!(!f.needs_admin());
    }

    #[test]
    fn values_are_quoted() {
        let f = Fix::RemoveExclusion { kind: ExclusionKind::Path, value: "C:\\it's here".into() };
        let (a, u) = f.steps("x");
        assert_eq!(a, "Remove-MpPreference -ExclusionPath 'C:\\it''s here'");
        assert!(u.starts_with("Add-MpPreference"));
        assert!(f.needs_admin());
    }

    #[test]
    fn restore_and_update_fixes_can_be_undone() {
        let (a, u) = Fix::EnableSystemProtection.steps("x");
        assert!(a.starts_with("Enable-ComputerRestore") && u.starts_with("Disable-ComputerRestore"), "{a} / {u}");
        assert!(a.contains(r#""$env:SystemDrive\""#), "{a}");
        let (a, u) = Fix::AllowAutoUpdates.steps("x");
        assert!(a.contains("Remove-ItemProperty") && a.contains(r"WindowsUpdate\AU") && u.contains("-Value 1"));
        assert!(Fix::AllowAutoUpdates.needs_admin());
    }

    /// Runs the real scripts against a throwaway key under HKCU, then removes it.
    #[test]
    #[ignore = "writes to the registry; run with --ignored"]
    fn startup_fix_and_undo_on_a_scratch_key() {
        let key = r"HKCU:\Software\bootmend-selftest\Run";
        let f = Fix::DisableStartup { name: "Sample app".into(), approved_key: key.into() };
        let (apply, undo) = f.scripts("x");
        let first = |k: &str| {
            let o = powershell(&format!("(Get-ItemProperty -Path {} -Name 'Sample app').'Sample app'[0]", ps_quote(k)), Duration::from_secs(60)).unwrap();
            o.stdout.trim().to_string()
        };
        run_ps(&apply).unwrap();
        assert_eq!(first(key), "3");
        run_ps(&undo).unwrap();
        assert_eq!(first(key), "2");
        run_ps(r"Remove-Item -Path 'HKCU:\Software\bootmend-selftest' -Recurse").unwrap();
    }

    #[test]
    fn new_fixes_record_an_exact_undo() {
        let svc = |start: &str| Fix::EnableService { name: "UsoSvc".into(), display: "Update Orchestrator Service".into(), start: start.into() }.scripts("x");
        let (a, u) = svc("Delayed");
        assert!(a.contains("sc.exe config 'UsoSvc' start= delayed-auto") && u.contains("Set-Service -Name 'UsoSvc' -StartupType Disabled"), "{a} / {u}");
        assert!(svc("Manual").0.contains("-StartupType Manual"));
        let (a, u) = Fix::EnableSampleSubmission.scripts("x");
        assert!(a.contains("SendSafeSamples") && a.contains("throw") && u.contains("NeverSend"), "checks Windows Security took it");
        let (a, u) = Fix::EnableLsaProtection.scripts("x");
        assert!(a.contains("RunAsPPL -Value 2") && a.contains("RunAsPPLBoot -Value 2") && u.contains("RunAsPPL -Value 0") && u.contains("Remove-ItemProperty"));
        let (a, u) = Fix::ResumeEncryption { drive: "C:".into() }.steps("x");
        assert!(a.starts_with("Resume-BitLocker -MountPoint 'C:'") && u.contains("Suspend-BitLocker -MountPoint 'C:' -RebootCount 0"));
        let (a, u) = Fix::TurnOffProxy { enabled: true, pac: "http://x/p.pac".into(), winhttp: "10.0.0.1:3128".into() }.scripts("x");
        assert!(a.contains("ProxyEnable -Value 0") && a.contains("AutoConfigURL") && a.contains("winhttp reset proxy"), "{a}");
        assert!(u.contains("ProxyEnable -Value 1") && u.contains("'http://x/p.pac'") && u.contains("'proxy-server=10.0.0.1:3128'"), "{u}");
        let (a, u) = Fix::TurnOffProxy { enabled: false, pac: "http://x/p.pac".into(), winhttp: String::new() }.scripts("x");
        assert!(!a.contains("ProxyEnable") && !a.contains("winhttp") && !u.contains("ProxyEnable"), "only what was on: {a} / {u}");
        assert!(!Fix::TurnOffProxy { enabled: true, pac: String::new(), winhttp: String::new() }.needs_admin(), "your own proxy setting");
        let (a, u) = Fix::DnsAutomatic { adapter: "Wi-Fi".into(), servers: vec!["185.10.10.10".into(), "1.1.1.1".into()] }.scripts("x");
        assert!(a.contains("-InterfaceAlias 'Wi-Fi' -ResetServerAddresses") && u.contains("-ServerAddresses '185.10.10.10','1.1.1.1'"), "{u}");
        let p = Fix::RemoveBrowserPolicies {
            browser: "Chrome".into(),
            key: r"HKCU\SOFTWARE\Policies\Google\Chrome".into(),
            settings: vec!["HomepageLocation".into()],
        };
        let (a, u) = p.scripts("id1");
        let copy = r"'HKCU\SOFTWARE\Bootmend\Changes\id1\Backup'";
        assert!(a.contains(&format!(r"reg.exe copy 'HKCU\SOFTWARE\Policies\Google\Chrome' {copy} /s /f")) && a.contains("'HomepageLocation'"), "{a}");
        assert!(u.contains(&format!(r"reg.exe copy {copy} 'HKCU\SOFTWARE\Policies\Google\Chrome' /s /f")), "the undo copies the same key back: {u}");
        assert!(u.trim_end().ends_with(r"Remove-Item -LiteralPath 'Registry::HKCU\SOFTWARE\Bootmend\Changes\id1' -Recurse }"), "{u}");
        assert!(!p.needs_admin());
        let (a, u) = Fix::SystemManagedPagefile.scripts("id2");
        let copy = r"-LiteralPath 'Registry::HKLM\SOFTWARE\Bootmend\Changes\id2' -Name PagingFiles";
        assert!(a.contains(&format!("Set-ItemProperty {copy}")) && a.contains("AutomaticManagedPagefile = $true"), "{a}");
        assert!(u.contains(&format!("Get-ItemProperty {copy}")) && u.contains("Set-ItemProperty 'HKLM:"), "puts back the exact page files: {u}");
        let (read, change) = (u.find(&format!("Get-ItemProperty {copy}")).unwrap(), u.find("AutomaticManagedPagefile").unwrap());
        assert!(read < change, "a missing copy stops the undo before anything changes: {u}");
        let (a, u) = Fix::EnableCrashFiles.scripts("x");
        assert!(a.contains("CrashDumpEnabled -Value 7") && u.contains("CrashDumpEnabled -Value 0"), "{a} / {u}");
        let (a, u) = Fix::EnableRecovery.scripts("x");
        assert!(a.contains("reagentc.exe /enable") && a.contains("throw") && u.contains("reagentc.exe /disable"), "{a} / {u}");
        let w = Fix::RemoveUpdateBlock {
            key: r"HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate".into(),
            settings: vec!["WUServer".into(), r"AU\UseWUServer".into()],
        };
        let (a, u) = w.scripts("id3");
        let copy = r"'HKLM\SOFTWARE\Bootmend\Changes\id3\Backup'";
        assert!(a.contains(copy) && a.contains(r"'AU\UseWUServer'") && u.contains(&format!("reg.exe copy {copy}")), "{a} / {u}");
        assert!(w.needs_admin());
        for fx in [Fix::EnableCloudProtection, Fix::EnableTrim, Fix::EnableDriverBlocklist] {
            let (a, u) = fx.scripts("x");
            assert!(!a.is_empty() && !u.is_empty() && a != u, "{fx:?}");
        }
    }

    /// The line shown while a fix runs reads as a sentence in progress, names what it acts on,
    /// and stays short enough for one line beside the progress bar.
    #[test]
    fn every_fix_says_what_it_is_doing() {
        let mut fixes = every_fix();
        fixes.push(Fix::EnableStartup { name: "Steam.lnk".into(), approved_key: STARTUP_APPROVED[3].into() });
        for fx in fixes {
            let d = fx.doing();
            assert!(d.chars().next().is_some_and(char::is_uppercase), "{fx:?}: {d}");
            assert!(!d.ends_with('.') && !d.contains(".lnk"), "{fx:?}: {d}");
            assert!(d.split_whitespace().next().is_some_and(|w| w.ends_with("ing")), "{fx:?}: {d}");
            assert!(d.chars().count() <= 70, "{fx:?}: {d}");
        }
        let named = Fix::DisableFirewallRule { name: "r".into(), display: "Rule".into() }.doing();
        assert!(named.contains("Rule"), "{named}");
    }

    fn every_fix() -> Vec<Fix> {
        vec![
            Fix::DisableStartup { name: "App".into(), approved_key: STARTUP_APPROVED[0].into() },
            Fix::EnableStartup { name: "App".into(), approved_key: STARTUP_APPROVED[2].into() },
            Fix::RemoveExclusion { kind: ExclusionKind::Path, value: r"C:\Games".into() },
            Fix::EnablePua,
            Fix::DisableRemoteAssistance,
            Fix::DisableFirewallRule { name: "r".into(), display: "Rule".into() },
            Fix::ServiceManual { name: "svc".into(), display: "Service".into() },
            Fix::DisableSmb1,
            Fix::EnableSystemProtection,
            Fix::AllowAutoUpdates,
            Fix::EnableService { name: "UsoSvc".into(), display: "Update Orchestrator Service".into(), start: "Delayed".into() },
            Fix::EnableService { name: "W32Time".into(), display: "Windows Time".into(), start: "Manual".into() },
            Fix::DisableAccount { name: "Guest".into() },
            Fix::EnableCloudProtection,
            Fix::EnableSampleSubmission,
            Fix::EnableTrim,
            Fix::ResumeEncryption { drive: "C:".into() },
            Fix::SystemManagedPagefile,
            Fix::EnableDriverBlocklist,
            Fix::EnableCrashFiles,
            Fix::EnableLsaProtection,
            Fix::TurnOffProxy { enabled: true, pac: "http://x/p.pac".into(), winhttp: "10.0.0.1:3128".into() },
            Fix::DnsAutomatic { adapter: "Wi-Fi".into(), servers: vec!["185.10.10.10".into()] },
            Fix::RemoveBrowserPolicies {
                browser: "Chrome".into(),
                key: r"HKLM\SOFTWARE\Policies\Google\Chrome".into(),
                settings: vec!["HomepageLocation".into()],
            },
            Fix::EnableRecovery,
            Fix::RemoveUpdateBlock {
                key: r"HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate".into(),
                settings: vec!["WUServer".into(), r"AU\UseWUServer".into()],
            },
            Fix::RepairWindowsFiles,
            Fix::RollBackDriver { device: r"PCI\VEN_1002&DEV_73DF\6&15bb039d&0&00000019".into(), name: "AMD Radeon RX 6750 XT".into() },
        ]
    }

    /// Parses every apply and undo script with PowerShell's own parser. Nothing runs.
    #[test]
    #[ignore = "starts PowerShell; run with --ignored"]
    fn every_fix_script_parses() {
        let mut checks = String::new();
        for fx in every_fix() {
            let (a, u) = fx.scripts("20261121-ab");
            for s in [a, u] {
                checks += &format!(
                    "$t = $null; $e = $null; [void][System.Management.Automation.Language.Parser]::ParseInput({}, [ref]$t, [ref]$e); if ($e) {{ 'BAD ' + {} + ': ' + $e[0].Message }}\n",
                    ps_quote(&s),
                    ps_quote(&format!("{fx:?}"))
                );
            }
        }
        let out = powershell(&checks, Duration::from_secs(60)).unwrap();
        assert!(!out.stdout.contains("BAD "), "{}", out.stdout);
    }

    /// Removes a forced setting from a throwaway policy key under HKCU, then puts it back from the copy.
    #[test]
    #[ignore = "writes to the registry; run with --ignored"]
    fn browser_policy_fix_and_undo_on_a_scratch_key() {
        let key = r"HKCU\Software\bootmend-selftest\Policies\Chrome";
        let setup = r#"$k = 'HKCU:\Software\bootmend-selftest\Policies\Chrome'; New-Item $k -Force | Out-Null; Set-ItemProperty $k -Name HomepageLocation -Value 'http://x.example'; Set-ItemProperty $k -Name Keep -Value 1; New-Item "$k\ExtensionInstallForcelist" -Force | Out-Null; Set-ItemProperty "$k\ExtensionInstallForcelist" -Name 1 -Value 'abc;https://x'"#;
        run_ps(setup).unwrap();
        let state = || {
            let o = powershell(
                r#"$k = 'HKCU:\Software\bootmend-selftest\Policies\Chrome'; '{0}|{1}|{2}' -f (Get-ItemProperty $k).HomepageLocation, (Get-ItemProperty $k).Keep, (Test-Path "$k\ExtensionInstallForcelist")"#,
                Duration::from_secs(60),
            )
            .unwrap();
            o.stdout.trim().to_string()
        };
        assert_eq!(state(), "http://x.example|1|True");
        let f = Fix::RemoveBrowserPolicies {
            browser: "Chrome".into(),
            key: key.into(),
            settings: vec!["HomepageLocation".into(), "ExtensionInstallForcelist".into()],
        };
        let id = format!("selftest-{:x}", crate::util::unique());
        let (apply, undo) = f.scripts(&id);
        run_ps(&apply).unwrap();
        assert_eq!(state(), "|1|False", "only the forced settings go");
        run_ps(&undo).unwrap();
        assert_eq!(state(), "http://x.example|1|True");
        run_ps(r"Remove-Item -Path 'HKCU:\Software\bootmend-selftest' -Recurse").unwrap();
        assert_copy_gone(&id);
    }

    /// The undo removes the change's saved copy; empty parent keys go too, so the test leaves nothing.
    fn assert_copy_gone(id: &str) {
        let left = powershell(&format!(r"Test-Path 'HKCU:\SOFTWARE\Bootmend\Changes\{id}'"), Duration::from_secs(60)).unwrap();
        run_ps(r"foreach ($k in 'HKCU:\SOFTWARE\Bootmend\Changes', 'HKCU:\SOFTWARE\Bootmend') { if ((Test-Path $k) -and -not (Get-ChildItem $k) -and -not (Get-Item $k).Property) { Remove-Item $k } }").unwrap();
        assert_eq!(left.stdout.trim(), "False");
    }

    /// Removes update-blocking values, one of them in a subkey, from a throwaway key under HKCU, then puts them back.
    #[test]
    #[ignore = "writes to the registry; run with --ignored"]
    fn update_block_fix_and_undo_on_a_scratch_key() {
        let setup = r#"$k = 'HKCU:\Software\bootmend-selftest-wu\WindowsUpdate'; New-Item "$k\AU" -Force | Out-Null; Set-ItemProperty $k -Name WUServer -Value 'http://127.0.0.1:8530'; Set-ItemProperty $k -Name Keep -Value 1; Set-ItemProperty "$k\AU" -Name UseWUServer -Value 1 -Type DWord; Set-ItemProperty "$k\AU" -Name AUOptions -Value 4 -Type DWord"#;
        run_ps(setup).unwrap();
        let state = || {
            let o = powershell(
                r#"$k = 'HKCU:\Software\bootmend-selftest-wu\WindowsUpdate'; '{0}|{1}|{2}|{3}' -f (Get-ItemProperty $k).WUServer, (Get-ItemProperty $k).Keep, (Get-ItemProperty "$k\AU").UseWUServer, (Get-ItemProperty "$k\AU").AUOptions"#,
                Duration::from_secs(60),
            )
            .unwrap();
            o.stdout.trim().to_string()
        };
        assert_eq!(state(), "http://127.0.0.1:8530|1|1|4");
        let f = Fix::RemoveUpdateBlock {
            key: r"HKCU\Software\bootmend-selftest-wu\WindowsUpdate".into(),
            settings: vec!["WUServer".into(), r"AU\UseWUServer".into()],
        };
        let id = format!("selftest-{:x}", crate::util::unique());
        let (apply, undo) = f.scripts(&id);
        run_ps(&apply).unwrap();
        assert_eq!(state(), "|1||4", "only the blocking values go");
        run_ps(&undo).unwrap();
        assert_eq!(state(), "http://127.0.0.1:8530|1|1|4");
        run_ps(r"Remove-Item -Path 'HKCU:\Software\bootmend-selftest-wu' -Recurse").unwrap();
        assert_copy_gone(&id);
    }
}
