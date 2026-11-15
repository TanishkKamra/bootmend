//! Opt-in fixes. Every fix shows what it will do, asks first, and stores an undo step.

use crate::util::{data_dir, powershell, ps_quote};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
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
        }
    }

    /// Per-user startup entries can be changed without administrator rights; everything else needs them.
    pub fn needs_admin(&self) -> bool {
        match self {
            Fix::DisableStartup { approved_key, .. } | Fix::EnableStartup { approved_key, .. } => approved_key.starts_with("HKLM"),
            Fix::TurnOffProxy { winhttp, .. } => !winhttp.is_empty(),
            Fix::RemoveBrowserPolicies { key, .. } | Fix::RemoveUpdateBlock { key, .. } => key.starts_with("HKLM"),
            _ => true,
        }
    }

    /// PowerShell that applies the change, and PowerShell that reverses it.
    pub fn scripts(&self) -> (String, String) {
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
                (format!("Disable-NetFirewallRule -Name {n}"), format!("Enable-NetFirewallRule -Name {n}"))
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
            Fix::DisableAccount { name } => {
                let n = ps_quote(name);
                (format!("Disable-LocalUser -Name {n}"), format!("Enable-LocalUser -Name {n}"))
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
                let f = format!("$f = {}", ps_quote(&backup_file("pagefiles", "txt")));
                let set = |v: &str| format!("Get-CimInstance Win32_ComputerSystem | Set-CimInstance -Property @{{ AutomaticManagedPagefile = ${v} }}");
                (
                    format!("{f}\n[IO.File]::WriteAllLines($f, [string[]]@((Get-ItemProperty {k}).PagingFiles | Where-Object {{ $_ }}))\n{}", set("true")),
                    format!("{f}\n{}\nSet-ItemProperty {k} -Name PagingFiles -Value ([IO.File]::ReadAllLines($f)) -Type MultiString", set("false")),
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
            Fix::RemoveBrowserPolicies { browser, key, settings } => remove_policies(&format!("policies-{browser}"), key, settings),
            Fix::EnableRecovery => (
                format!("{NATIVE}if (Native {{ reagentc.exe /enable }}) {{ throw 'Windows could not switch its recovery tools back on' }}"),
                format!("{NATIVE}if (Native {{ reagentc.exe /disable }}) {{ throw 'Windows could not switch its recovery tools off' }}"),
            ),
            Fix::RemoveUpdateBlock { key, settings } => remove_policies("policies-windowsupdate", key, settings),
        }
    }
}

/// Saves a copy of a policy key, then removes the named settings from it: values, or subkeys (lists,
/// Firefox's groups). A name with a backslash is in a subkey ("AU\UseWUServer"). Undo imports the copy.
fn remove_policies(what: &str, key: &str, settings: &[String]) -> (String, String) {
    let hive = key.split('\\').next().unwrap_or_default();
    let f = ps_quote(&backup_file(&format!("{what}-{hive}"), "reg"));
    let (reg, path) = (ps_quote(key), ps_quote(&format!("Registry::{key}")));
    let names = settings.iter().map(|x| ps_quote(x)).collect::<Vec<_>>().join(", ");
    let apply = format!(
        r#"{NATIVE}$f = {f}
if (Native {{ reg.exe export {reg} $f /y }}) {{ throw 'Could not save a copy of the settings first, so nothing was changed' }}
$p = {path}
foreach ($n in @({names})) {{
  $i = $n.LastIndexOf('\'); $k = $(if ($i -ge 0) {{ "$p\$($n.Substring(0, $i))" }} else {{ $p }}); $v = $n.Substring($i + 1)
  if (Test-Path -LiteralPath "$k\$v") {{ Remove-Item -LiteralPath "$k\$v" -Recurse }}
  if ((Get-ItemProperty -LiteralPath $k).PSObject.Properties.Name -contains $v) {{ Remove-ItemProperty -LiteralPath $k -Name $v }}
}}"#
    );
    let undo = format!("{NATIVE}$f = {f}\nif (Native {{ reg.exe import $f }}) {{ throw 'Could not put the settings back from the saved copy' }}");
    (apply, undo)
}

/// Where a copy of settings is kept before they are removed, so undo can put them back.
fn backup_file(what: &str, ext: &str) -> String {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dir = data_dir("backups").unwrap_or_else(|_| std::env::temp_dir());
    dir.join(format!("{what}-{stamp}.{ext}")).display().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub when: String,
    pub title: String,
    pub fix: Fix,
    pub undo_script: String,
    pub undone: Option<String>,
}

fn history_path() -> Result<std::path::PathBuf> {
    Ok(data_dir("")?.join("history.json"))
}

pub fn history() -> Vec<HistoryEntry> {
    let Ok(p) = history_path() else { return vec![] };
    match std::fs::read(&p) {
        Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| {
            log!("history", "history file unreadable ({e}); starting a new one");
            vec![]
        }),
        Err(_) => vec![],
    }
}

fn save_history(h: &[HistoryEntry]) -> Result<()> {
    let p = history_path()?;
    std::fs::write(&p, serde_json::to_vec_pretty(h)?).with_context(|| format!("saving {}", p.display()))
}

fn run_ps(body: &str) -> Result<()> {
    let script =
        format!("$ErrorActionPreference = 'Stop'\ntry {{\n{body}\n'BOOTMEND-OK'\n}} catch {{ Write-Output ('BOOTMEND-ERR ' + $_.Exception.Message); exit 1 }}");
    let out = powershell(&script, Duration::from_secs(300))?;
    if out.stdout.contains("BOOTMEND-OK") {
        return Ok(());
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
    apply_with(fix, title, history(), &mut |h| save_history(h), &run_ps)
}

type Save<'a> = &'a mut dyn FnMut(&[HistoryEntry]) -> Result<()>;
type Run<'a> = &'a dyn Fn(&str) -> Result<()>;

/// The undo step is saved before anything changes, so Bootmend never makes a change it can't take back.
fn apply_with(fix: &Fix, title: &str, mut h: Vec<HistoryEntry>, save: Save, run: Run) -> Result<()> {
    let (apply, undo) = fix.scripts();
    log!("fix", "applying \"{}\"", title);
    let t0 = Instant::now();
    h.push(HistoryEntry { when: crate::util::now_stamp(), title: title.to_string(), fix: fix.clone(), undo_script: undo, undone: None });
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

/// Undo the history entry at `index`.
pub fn undo(index: usize) -> Result<()> {
    undo_with(index, history(), &mut |h| save_history(h), &run_ps)
}

/// Recorded as undone before the undo runs, so History never offers to undo a change twice.
fn undo_with(index: usize, mut h: Vec<HistoryEntry>, save: Save, run: Run) -> Result<()> {
    let e = h.get_mut(index).ok_or_else(|| anyhow!("that change is no longer in the history"))?;
    if e.undone.is_some() {
        return Err(anyhow!("that change was already undone"));
    }
    let (title, script) = (e.title.clone(), e.undo_script.clone());
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
        Fix::DisableStartup { name: "Steam".into(), approved_key: r"HKCU:\Software\X\Run".into() }
    }

    fn applied() -> Vec<HistoryEntry> {
        vec![HistoryEntry { when: "2026-11-15 18:00".into(), title: "Steam".into(), fix: steam(), undo_script: "undo".into(), undone: None }]
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
        assert!(saved[0].undone.is_none() && saved[0].undo_script == steam().scripts().1);
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
            &|s| if s == "undo" { Ok(()) } else { Err(anyhow!("wrong script")) },
        )
        .unwrap();
        assert!(saved[0].undone.is_some());
    }

    #[test]
    fn startup_scripts_flip_the_approved_flag() {
        let f = Fix::DisableStartup { name: "Steam".into(), approved_key: r"HKCU:\Software\X\Run".into() };
        let (a, u) = f.scripts();
        assert!(a.contains("(3,0,") && u.contains("(2,0,"));
        assert!(!f.needs_admin());
    }

    #[test]
    fn values_are_quoted() {
        let f = Fix::RemoveExclusion { kind: ExclusionKind::Path, value: "C:\\it's here".into() };
        let (a, u) = f.scripts();
        assert_eq!(a, "Remove-MpPreference -ExclusionPath 'C:\\it''s here'");
        assert!(u.starts_with("Add-MpPreference"));
        assert!(f.needs_admin());
    }

    #[test]
    fn restore_and_update_fixes_can_be_undone() {
        let (a, u) = Fix::EnableSystemProtection.scripts();
        assert!(a.starts_with("Enable-ComputerRestore") && u.starts_with("Disable-ComputerRestore"), "{a} / {u}");
        assert!(a.contains(r#""$env:SystemDrive\""#), "{a}");
        let (a, u) = Fix::AllowAutoUpdates.scripts();
        assert!(a.contains("Remove-ItemProperty") && a.contains(r"WindowsUpdate\AU") && u.contains("-Value 1"));
        assert!(Fix::AllowAutoUpdates.needs_admin());
    }

    /// Runs the real scripts against a throwaway key under HKCU, then removes it.
    #[test]
    #[ignore = "writes to the registry; run with --ignored"]
    fn startup_fix_and_undo_on_a_scratch_key() {
        let key = r"HKCU:\Software\bootmend-selftest\Run";
        let f = Fix::DisableStartup { name: "Sample app".into(), approved_key: key.into() };
        let (apply, undo) = f.scripts();
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
        let svc = |start: &str| Fix::EnableService { name: "UsoSvc".into(), display: "Update Orchestrator Service".into(), start: start.into() }.scripts();
        let (a, u) = svc("Delayed");
        assert!(a.contains("sc.exe config 'UsoSvc' start= delayed-auto") && u.contains("Set-Service -Name 'UsoSvc' -StartupType Disabled"), "{a} / {u}");
        assert!(svc("Manual").0.contains("-StartupType Manual"));
        let (a, u) = Fix::DisableAccount { name: "Guest".into() }.scripts();
        assert_eq!((a.as_str(), u.as_str()), ("Disable-LocalUser -Name 'Guest'", "Enable-LocalUser -Name 'Guest'"));
        let (a, u) = Fix::EnableSampleSubmission.scripts();
        assert!(a.contains("SendSafeSamples") && a.contains("throw") && u.contains("NeverSend"), "checks Windows Security took it");
        let (a, u) = Fix::EnableLsaProtection.scripts();
        assert!(a.contains("RunAsPPL -Value 2") && a.contains("RunAsPPLBoot -Value 2") && u.contains("RunAsPPL -Value 0") && u.contains("Remove-ItemProperty"));
        let (a, u) = Fix::ResumeEncryption { drive: "C:".into() }.scripts();
        assert!(a.starts_with("Resume-BitLocker -MountPoint 'C:'") && u.contains("Suspend-BitLocker -MountPoint 'C:' -RebootCount 0"));
        let (a, u) = Fix::TurnOffProxy { enabled: true, pac: "http://x/p.pac".into(), winhttp: "10.0.0.1:3128".into() }.scripts();
        assert!(a.contains("ProxyEnable -Value 0") && a.contains("AutoConfigURL") && a.contains("winhttp reset proxy"), "{a}");
        assert!(u.contains("ProxyEnable -Value 1") && u.contains("'http://x/p.pac'") && u.contains("'proxy-server=10.0.0.1:3128'"), "{u}");
        let (a, u) = Fix::TurnOffProxy { enabled: false, pac: "http://x/p.pac".into(), winhttp: String::new() }.scripts();
        assert!(!a.contains("ProxyEnable") && !a.contains("winhttp") && !u.contains("ProxyEnable"), "only what was on: {a} / {u}");
        assert!(!Fix::TurnOffProxy { enabled: true, pac: String::new(), winhttp: String::new() }.needs_admin(), "your own proxy setting");
        let (a, u) = Fix::DnsAutomatic { adapter: "Wi-Fi".into(), servers: vec!["185.10.10.10".into(), "1.1.1.1".into()] }.scripts();
        assert!(a.contains("-InterfaceAlias 'Wi-Fi' -ResetServerAddresses") && u.contains("-ServerAddresses '185.10.10.10','1.1.1.1'"), "{u}");
        let p = Fix::RemoveBrowserPolicies {
            browser: "Chrome".into(),
            key: r"HKCU\SOFTWARE\Policies\Google\Chrome".into(),
            settings: vec!["HomepageLocation".into()],
        };
        let (a, u) = p.scripts();
        assert!(a.contains("export") && a.contains("'HomepageLocation'") && u.contains("import"), "{a} / {u}");
        let backup = a.lines().find(|l| l.starts_with("$f = ")).expect("backup path");
        assert!(u.contains(backup), "the undo imports the same copy");
        assert!(!p.needs_admin());
        let (a, u) = Fix::SystemManagedPagefile.scripts();
        let backup = a.lines().find(|l| l.starts_with("$f = ")).expect("backup path");
        assert!(a.contains("WriteAllLines") && a.contains("AutomaticManagedPagefile = $true"), "{a}");
        assert!(u.contains(backup) && u.contains("ReadAllLines") && u.contains("PagingFiles"), "puts back the exact page files: {u}");
        let (a, u) = Fix::EnableCrashFiles.scripts();
        assert!(a.contains("CrashDumpEnabled -Value 7") && u.contains("CrashDumpEnabled -Value 0"), "{a} / {u}");
        let (a, u) = Fix::EnableRecovery.scripts();
        assert!(a.contains("reagentc.exe /enable") && a.contains("throw") && u.contains("reagentc.exe /disable"), "{a} / {u}");
        let w = Fix::RemoveUpdateBlock {
            key: r"HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate".into(),
            settings: vec!["WUServer".into(), r"AU\UseWUServer".into()],
        };
        let (a, u) = w.scripts();
        let backup = a.lines().find(|l| l.starts_with("$f = ")).expect("backup path");
        assert!(a.contains("export") && a.contains(r"'AU\UseWUServer'") && u.contains("import") && u.contains(backup), "{a} / {u}");
        assert!(w.needs_admin());
        for fx in [Fix::EnableCloudProtection, Fix::EnableTrim, Fix::EnableDriverBlocklist] {
            let (a, u) = fx.scripts();
            assert!(!a.is_empty() && !u.is_empty() && a != u, "{fx:?}");
        }
    }

    /// The line shown while a fix runs reads as a sentence in progress, names what it acts on,
    /// and stays short enough for one line beside the progress bar.
    #[test]
    fn every_fix_says_what_it_is_doing() {
        let mut fixes = every_fix();
        fixes.push(Fix::EnableStartup { name: "Steam.lnk".into(), approved_key: r"HKCU:\Software\X\Run".into() });
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
            Fix::DisableStartup { name: "App".into(), approved_key: r"HKCU:\Software\X\Run".into() },
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
        ]
    }

    /// Parses every apply and undo script with PowerShell's own parser. Nothing runs.
    #[test]
    #[ignore = "starts PowerShell; run with --ignored"]
    fn every_fix_script_parses() {
        let mut checks = String::new();
        for fx in every_fix() {
            let (a, u) = fx.scripts();
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
        let (apply, undo) = f.scripts();
        run_ps(&apply).unwrap();
        assert_eq!(state(), "|1|False", "only the forced settings go");
        run_ps(&undo).unwrap();
        assert_eq!(state(), "http://x.example|1|True");
        let backup = apply.lines().find_map(|l| l.strip_prefix("$f = ")).unwrap().trim_matches('\'').to_string();
        run_ps(r"Remove-Item -Path 'HKCU:\Software\bootmend-selftest' -Recurse").unwrap();
        std::fs::remove_file(backup).unwrap();
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
        let (apply, undo) = f.scripts();
        run_ps(&apply).unwrap();
        assert_eq!(state(), "|1||4", "only the blocking values go");
        run_ps(&undo).unwrap();
        assert_eq!(state(), "http://127.0.0.1:8530|1|1|4");
        let backup = apply.lines().find_map(|l| l.strip_prefix("$f = ")).unwrap().trim_matches('\'').to_string();
        run_ps(r"Remove-Item -Path 'HKCU:\Software\bootmend-selftest-wu' -Recurse").unwrap();
        std::fs::remove_file(backup).unwrap();
    }
}
