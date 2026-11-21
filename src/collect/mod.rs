//! Runs every check in parallel on background threads and reports progress to the UI.

pub mod dumps;
pub mod extensions;
pub mod smart;
pub mod winget;

use crate::snapshot::{Snapshot, FORMAT_VERSION};
use crate::util::{powershell, ps_quote, J};
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
    StepDone { key: &'static str, ok: bool },
    Finished(Box<Snapshot>),
    Failed(String),
}

fn send(tx: &Sender<Progress>, p: Progress) {
    // The window may have closed mid-check; there is nobody left to tell.
    let _ = tx.send(p);
}

/// Run one embedded PowerShell section and parse its JSON.
pub fn run_section(key: &str, body: &str, timeout: Duration) -> Result<Value> {
    let tmp = crate::sys::tmp_dir()?;
    let script = format!("$env:BOOTMEND_TMP = {}\n{}\n{}", ps_quote(&tmp.to_string_lossy()), COMMON, body);
    let out = powershell(&script, timeout)?;
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
                    let r = run_section(st.key, body, Duration::from_secs(st.timeout_s));
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
            let e = extensions::collect();
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

    snap.drives = smart::drive_health(&snap.storage);
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
