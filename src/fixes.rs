//! Opt-in fixes. Every fix shows what it will do, asks first, and stores an undo step.

use crate::util::{data_dir, powershell, ps_quote};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Fix {
    DisableStartup { name: String, approved_key: String },
    EnableStartup { name: String, approved_key: String },
    RemoveExclusion { kind: ExclusionKind, value: String },
    EnablePua,
    DisableRemoteAssistance,
    DisableFirewallRule { name: String, display: String },
    ServiceManual { name: String, display: String },
    DisableSmb1,
    EnableSystemProtection,
    AllowAutoUpdates,
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
        }
    }

    /// One sentence for the confirmation dialog: what changes, and that it can be undone.
    pub fn describe(&self) -> String {
        match self {
            Fix::DisableStartup { name, .. } => format!("{name} will no longer open when you sign in. You can still open it yourself, and you can undo this from History."),
            Fix::EnableStartup { name, .. } => format!("{name} will open again when you sign in."),
            Fix::RemoveExclusion { value, .. } => format!("Windows Security will check {value} again. If an app you trust stops working afterwards, undo this from History."),
            Fix::EnablePua => "Windows Security will block apps that are not viruses but bundle adware, toolbars or crypto miners. You can undo this from History.".into(),
            Fix::DisableRemoteAssistance => "Nobody will be able to send this PC a Remote Assistance invitation. You can turn it back on from History.".into(),
            Fix::DisableFirewallRule { display, .. } => format!("The firewall rule \"{display}\" will be switched off, not deleted. You can switch it back on from History."),
            Fix::ServiceManual { display, .. } => format!("{display} will start only when an app asks for it, instead of every time Windows starts. You can undo this from History."),
            Fix::DisableSmb1 => "The old SMB1 file-sharing protocol will be removed. Very old network drives may stop working; you can turn it back on from History. A restart finishes the change.".into(),
            Fix::EnableSystemProtection => "Windows will start saving restore points for drive C before updates and driver installs, using a small share of the drive. Undoing this later deletes the saved restore points.".into(),
            Fix::AllowAutoUpdates => "The policy that stops automatic updates will be removed, so Windows installs security fixes by itself again. You can put the policy back from History.".into(),
        }
    }

    /// Per-user startup entries can be changed without administrator rights; everything else needs them.
    pub fn needs_admin(&self) -> bool {
        match self {
            Fix::DisableStartup { approved_key, .. } | Fix::EnableStartup { approved_key, .. } => approved_key.starts_with("HKLM"),
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
        }
    }
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
            eprintln!("history file unreadable ({e}); starting a new one");
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
        format!("$ErrorActionPreference = 'Stop'\ntry {{\n{body}\n'RIGSCAN-OK'\n}} catch {{ Write-Output ('RIGSCAN-ERR ' + $_.Exception.Message); exit 1 }}");
    let out = powershell(&script, Duration::from_secs(300))?;
    if out.stdout.contains("RIGSCAN-OK") {
        return Ok(());
    }
    let msg = out
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("RIGSCAN-ERR "))
        .map(str::trim)
        .unwrap_or(if out.timed_out { "it took too long" } else { "Windows did not say why" })
        .to_string();
    Err(anyhow!("{msg}"))
}

/// Apply a fix and record how to undo it.
pub fn apply(fix: &Fix, title: &str) -> Result<()> {
    let (apply, undo) = fix.scripts();
    run_ps(&apply)?;
    let mut h = history();
    h.push(HistoryEntry { when: crate::util::now_stamp(), title: title.to_string(), fix: fix.clone(), undo_script: undo, undone: None });
    save_history(&h)
}

/// Save a System Restore point. Windows normally allows one a day, so this checks that one was really made.
pub fn restore_point(description: &str) -> Result<()> {
    let d = ps_quote(description);
    run_ps(&format!(
        "$t = Get-Date\nCheckpoint-Computer -Description {d} -RestorePointType MODIFY_SETTINGS -WarningAction SilentlyContinue\n$l = Get-ComputerRestorePoint | Select-Object -Last 1\nif (-not $l -or [Management.ManagementDateTimeConverter]::ToDateTime($l.CreationTime) -lt $t.AddMinutes(-1)) {{ throw 'Windows skipped it because it already saved a restore point in the last 24 hours' }}"
    ))
}

/// Undo the history entry at `index`.
pub fn undo(index: usize) -> Result<()> {
    let mut h = history();
    let e = h.get_mut(index).ok_or_else(|| anyhow!("that change is no longer in the history"))?;
    if e.undone.is_some() {
        return Err(anyhow!("that change was already undone"));
    }
    run_ps(&e.undo_script)?;
    e.undone = Some(crate::util::now_stamp());
    save_history(&h)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let key = r"HKCU:\Software\rigscan-selftest\Run";
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
        run_ps(r"Remove-Item -Path 'HKCU:\Software\rigscan-selftest' -Recurse").unwrap();
    }
}
