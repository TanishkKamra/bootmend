//! Runs every check in parallel on background threads and reports progress to the UI.

pub mod dumps;
pub mod extensions;
pub mod smart;
pub mod winget;

use crate::snapshot::{Snapshot, FORMAT_VERSION};
use crate::util::{powershell_live, ps_quote, J};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::mpsc::Sender;
use std::time::Duration;

const COMMON: &str = include_str!("ps/common.ps1");

/// A step in a check, as shown on the progress screen.
pub struct Step {
    pub key: &'static str,
    pub label: &'static str,
    script: Option<&'static str>,
    timeout_s: u64,
}

pub const STEPS: [Step; 8] = [
    Step { key: "system", label: "Hardware and Windows details", script: Some(include_str!("ps/system.ps1")), timeout_s: 180 },
    Step { key: "storage", label: "Drives and free space", script: Some(include_str!("ps/storage.ps1")), timeout_s: 150 },
    Step { key: "network", label: "Network adapters", script: Some(include_str!("ps/network.ps1")), timeout_s: 90 },
    Step { key: "security", label: "Protection settings", script: Some(include_str!("ps/security.ps1")), timeout_s: 150 },
    Step { key: "software", label: "Apps, startup items and services", script: Some(include_str!("ps/software.ps1")), timeout_s: 180 },
    Step { key: "health", label: "Crash and performance history", script: Some(include_str!("ps/health.ps1")), timeout_s: 150 },
    Step { key: "extensions", label: "Browser extensions", script: None, timeout_s: 0 },
    Step { key: "drives", label: "Drive health counters", script: None, timeout_s: 0 },
];

pub enum Progress {
    /// A step has started on one of its parts, said in words for the details panel.
    Doing {
        key: &'static str,
        text: String,
    },
    StepDone {
        key: &'static str,
        ok: bool,
    },
    Finished(Box<Snapshot>),
    Failed(String),
}

fn send(tx: &Sender<Progress>, p: Progress) {
    // The window may have closed mid-check; there is nobody left to tell.
    let _ = tx.send(p);
}

/// What each part of the check-up is doing, by its name in the scripts.
pub fn part_line(name: &str) -> Option<&'static str> {
    Some(match name {
        "device" => "Reading the PC's model, TPM and Secure Boot",
        "os" => "Reading the Windows version and licence",
        "cpu" => "Reading the processor",
        "memory" => "Reading the memory",
        "gpu" => "Reading the graphics card",
        "displays" => "Reading the screens",
        "directx" => "Asking DirectX about the graphics",
        "battery" => "Reading the battery report",
        "power" => "Reading the power plan",
        "hardware" => "Listing sound, camera and other devices",
        "winupdate" => "Reading Windows Update's history",
        "recovery" => "Checking Windows Recovery",
        "profile" => "Checking your user profile loaded properly",
        "disks" => "Listing the drives",
        "volumes" => "Measuring free space on each drive",
        "folderDrives" => "Finding which drive holds your folders",
        "ataSmart" => "Reading the drives' health tables",
        "trim" => "Checking how Windows looks after SSDs",
        "diskErrors" => "Reading drive errors from the last 90 days",
        "backup" => "Checking OneDrive's backup",
        "folders" => "Measuring big folders like Downloads",
        "adapters" => "Looking at the network adapters",
        "wifi" => "Reading the Wi-Fi connection",
        "proxy" => "Checking the proxy settings",
        "staticDns" => "Checking the DNS settings",
        "listening" => "Listing apps that accept network connections",
        "vpn" => "Looking for VPN and virtual adapters",
        "defender" => "Asking Windows Security for its settings",
        "firewall" => "Checking the firewall and its openings",
        "access" => "Checking remote access settings",
        "browserPolicies" => "Looking for browser settings set by a policy",
        "dailyAccount" => "Checking whether your account is an administrator",
        "drivers" => "Checking that drivers are signed",
        "deviceAccess" => "Reading which apps used the camera or microphone",
        "restore" => "Checking System Restore",
        "apps" => "Listing installed apps",
        "startup" => "Looking at what starts with Windows",
        "services" => "Going through background services",
        "tasks" => "Going through scheduled tasks",
        "perf" => "Reading how long the PC has been on",
        "crashControl" => "Checking how Windows saves crash details",
        "stability" => "Reading crashes from the last 90 days",
        "driverUpdates" => "Looking at recent driver changes",
        "imageHealth" => "Asking Windows if its own files are damaged",
        "boot" => "Reading how long Windows takes to start",
        _ => return None,
    })
}

/// Run one embedded PowerShell section and parse its JSON. `on_part` hears each part's name
/// as the section starts on it.
pub fn run_section(key: &str, body: &str, timeout: Duration, on_part: &mut dyn FnMut(&str)) -> Result<Value> {
    let tmp = crate::sys::tmp_dir()?;
    let script = format!("$env:BOOTMEND_TMP = {}\n{}\n{}", ps_quote(&tmp.to_string_lossy()), COMMON, body);
    let out = powershell_live(&script, timeout, on_part)?;
    if out.timed_out {
        return Err(anyhow!("{key} took too long and was stopped"));
    }
    let line = out.stdout.lines().rev().find(|l| l.trim_start().starts_with('{')).ok_or_else(|| {
        let err = out.stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("no output");
        anyhow!("{key} returned no data ({err})")
    })?;
    Ok(serde_json::from_str(line.trim())?)
}

/// Collect a full snapshot. Blocks; call from a background thread.
pub fn collect(tx: Sender<Progress>) {
    let started = std::time::Instant::now();
    log!("check", "started");
    match collect_inner(&tx) {
        Ok(s) => {
            log!("check", "finished in {:.0} s with {} parts it couldn't read", started.elapsed().as_secs_f32(), s.errors.len());
            send(&tx, Progress::Finished(Box::new(s)))
        }
        Err(e) => {
            log!("check", "didn't finish: {e:#}");
            send(&tx, Progress::Failed(format!("{e:#}")))
        }
    }
}

fn collect_inner(tx: &Sender<Progress>) -> Result<Snapshot> {
    let started = chrono::Local::now();
    let mut snap = Snapshot {
        version: FORMAT_VERSION,
        id: started.format("%Y%m%d-%H%M%S").to_string(),
        taken: started.format("%Y-%m-%d %H:%M").to_string(),
        elevated: crate::elevate::is_elevated(),
        ..Default::default()
    };

    let mut sections: Vec<(&'static str, Result<Value>)> = Vec::new();
    std::thread::scope(|sc| {
        let handles: Vec<_> = STEPS
            .iter()
            .filter_map(|st| st.script.map(|body| (st, body)))
            .map(|(st, body)| {
                let tx = tx.clone();
                sc.spawn(move || {
                    let t = std::time::Instant::now();
                    let mut say = |name: &str| match part_line(name) {
                        Some(text) => send(&tx, Progress::Doing { key: st.key, text: text.into() }),
                        None => log!("check", "{}: no words for the part {name}", st.key),
                    };
                    let r = run_section(st.key, body, Duration::from_secs(st.timeout_s), &mut say);
                    match &r {
                        Ok(_) => log!("check", "{} done in {:.1} s", st.key, t.elapsed().as_secs_f32()),
                        Err(e) => log!("check", "{} incomplete after {:.1} s: {e:#}", st.key, t.elapsed().as_secs_f32()),
                    }
                    send(&tx, Progress::StepDone { key: st.key, ok: r.is_ok() });
                    (st.key, r)
                })
            })
            .collect();
        let ext = sc.spawn(|| {
            let e = extensions::collect(&|text| send(tx, Progress::Doing { key: "extensions", text }));
            log!("check", "extensions done: {} found", e.len());
            send(tx, Progress::StepDone { key: "extensions", ok: true });
            e
        });
        for h in handles {
            match h.join() {
                Ok(r) => sections.push(r),
                Err(_) => {
                    log!("check", "a step crashed");
                    snap.errors.push("A check stopped unexpectedly".into())
                }
            }
        }
        match ext.join() {
            Ok(e) => snap.extensions = e,
            Err(_) => {
                log!("check", "reading browser extensions crashed");
                snap.errors.push("Reading browser extensions stopped unexpectedly".into())
            }
        }
    });

    for (key, r) in sections {
        let v = match r {
            Ok(v) => v,
            Err(e) => {
                snap.errors.push(format!("{e:#}"));
                Value::Null
            }
        };
        for e in v.strs("errors") {
            log!("check", "{key} couldn't read a part: {e}");
            snap.errors.push(format!("{key}: {e}"));
        }
        match key {
            "system" => snap.system = v,
            "storage" => snap.storage = v,
            "network" => snap.network = v,
            "security" => snap.security = v,
            "software" => snap.software = v,
            "health" => snap.health = v,
            _ => {}
        }
    }

    snap.drives =
        smart::drive_health(&snap.storage, &|model| send(tx, Progress::Doing { key: "drives", text: format!("Reading the health counters of {model}") }));
    log!("check", "drive health read for {} drives", snap.drives.len());
    send(tx, Progress::StepDone { key: "drives", ok: true });

    snap.host = snap.system.g("device").s("Hostname").or_else(|| std::env::var("COMPUTERNAME").ok()).unwrap_or_else(|| "this-pc".into());
    // Carry forward results that cost time or internet to get, so a re-check doesn't lose them.
    if let Some(prev) = crate::snapshot::latest(&snap.host) {
        snap.dumps = prev.dumps.into_iter().filter(|d| std::path::Path::new(&d.file).exists()).collect();
        snap.scans = prev.scans;
    }
    crate::snapshot::save(&snap)?;
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_of_the_check_up_has_words_for_the_details_panel() {
        let names: Vec<&str> =
            STEPS.iter().filter_map(|s| s.script).flat_map(|s| s.split("Try-Section '").skip(1)).filter_map(|s| s.split('\'').next()).collect();
        assert!(names.len() > 40, "found the parts: {names:?}");
        let missing: Vec<&&str> = names.iter().filter(|n| part_line(n).is_none()).collect();
        assert!(missing.is_empty(), "no words for {missing:?}");
    }

    #[test]
    #[ignore = "starts PowerShell; run with --ignored"]
    fn each_part_is_heard_as_it_starts_and_the_data_still_arrives() {
        let mut heard = vec![];
        let body = "Try-Section 'cpu' { $R.a = 1 }\nTry-Section 'os' { $R.b = 2 }\nEmit";
        let v = run_section("test", body, Duration::from_secs(60), &mut |p| heard.push(p.to_string())).unwrap();
        assert_eq!(heard, ["cpu", "os"]);
        assert_eq!((v["a"].as_i64(), v["b"].as_i64()), (Some(1), Some(2)));
    }
}
