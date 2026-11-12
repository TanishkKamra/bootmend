//! Virus checks through Microsoft Defender's own command-line scanner (MpCmdRun).

use crate::snapshot::ScanResult;
use anyhow::{anyhow, Context, Result};
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// Defender's quick scan. Follows Defender's settings, including its skip list.
    Quick,
    /// Every file on every drive. Follows Defender's settings.
    Full,
    /// One folder or drive, reporting only. Defender ignores its skip list for this kind.
    Folder,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Quick => "Quick check",
            Kind::Full => "Full check",
            Kind::Folder => "Folder check",
        }
    }
}

/// Microsoft recommends the newest copy under ProgramData's Platform folder, then Program Files.
pub fn mpcmdrun() -> Option<PathBuf> {
    let platform = std::env::var_os("ProgramData").map(|d| PathBuf::from(d).join(r"Microsoft\Windows Defender\Platform"));
    let newest = platform.and_then(|p| std::fs::read_dir(p).ok()).and_then(|rd| {
        rd.flatten().map(|e| e.path()).filter(|p| p.join("MpCmdRun.exe").is_file()).max_by(|a, b| {
            if crate::util::version_lt(&a.to_string_lossy(), &b.to_string_lossy()) {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        })
    });
    newest.map(|d| d.join("MpCmdRun.exe")).or_else(|| {
        let p = PathBuf::from(std::env::var_os("ProgramFiles")?).join(r"Windows Defender\MpCmdRun.exe");
        p.is_file().then_some(p)
    })
}

/// Parse MpCmdRun console output into threat names and affected files.
pub fn parse_output(text: &str) -> (Vec<String>, Vec<String>) {
    let mut threats = Vec::new();
    let mut files = Vec::new();
    for l in text.lines() {
        let t = l.trim();
        if let Some(v) = t.strip_prefix("Threat").and_then(|r| r.trim_start().strip_prefix(':')) {
            threats.push(v.trim().to_string());
        } else if let Some(v) = t.strip_prefix("file").and_then(|r| r.trim_start().strip_prefix(':')) {
            files.push(v.trim().to_string());
        }
    }
    threats.dedup();
    (threats, files)
}

/// Run a scan. Blocks until it finishes or `cancel` is set.
pub fn run(kind: Kind, target: Option<&Path>, exclusions: &[String], cancel: Arc<AtomicBool>) -> Result<ScanResult> {
    let exe = mpcmdrun().ok_or_else(|| anyhow!("Microsoft Defender's scanner was not found on this PC"))?;
    let mut args: Vec<String> = vec!["-Scan".into()];
    let target_s = target.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    match kind {
        Kind::Quick => args.extend(["-ScanType".into(), "1".into()]),
        Kind::Full => args.extend(["-ScanType".into(), "2".into()]),
        Kind::Folder => {
            if target_s.is_empty() {
                return Err(anyhow!("choose a folder or drive first"));
            }
            args.extend(["-ScanType".into(), "3".into(), "-File".into(), target_s.clone(), "-DisableRemediation".into()]);
        }
    }
    let started = chrono::Local::now();
    let t0 = Instant::now();
    let mut child = Command::new(&exe)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(crate::util::CREATE_NO_WINDOW)
        .spawn()
        .context("starting Microsoft Defender's scanner")?;
    let mut so = child.stdout.take().ok_or_else(|| anyhow!("no output from the scanner"))?;
    let reader = std::thread::spawn(move || {
        let mut b = Vec::new();
        so.read_to_end(&mut b).map(|_| b)
    });
    let mut stopped = false;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break Some(s);
        }
        if cancel.load(Ordering::Relaxed) {
            stopped = true;
            if let Err(e) = child.kill() {
                eprintln!("could not stop the scan: {e}");
            }
            // Quick and full scans keep running inside Defender's service unless cancelled there too.
            if kind != Kind::Folder {
                if let Err(e) = crate::util::run_hidden(&exe.to_string_lossy(), &["-Scan", "-Cancel"], Duration::from_secs(30)) {
                    eprintln!("could not cancel the Defender scan: {e}");
                }
            }
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    let text = match reader.join() {
        Ok(Ok(b)) => crate::util::decode_bytes(&b),
        _ => String::new(),
    };
    let (threats, files) = parse_output(&text);
    let code = status.and_then(|s| s.code());
    let skipped = match kind {
        Kind::Folder => None,
        _ => exclusions.iter().find(|e| e.len() <= 3 && e.contains(':')).cloned(),
    };
    let message = if stopped {
        "Stopped before it finished.".to_string()
    } else if !threats.is_empty() {
        format!("Found {} item{} worth a look.", threats.len(), if threats.len() == 1 { "" } else { "s" })
    } else if code == Some(0) {
        "Nothing harmful found.".to_string()
    } else if code == Some(2) {
        "Finished, but Defender reported some scanning errors. Nothing harmful was listed.".to_string()
    } else {
        format!("The scanner stopped with code {}.", code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".into()))
    };
    Ok(ScanResult {
        kind: kind.label().into(),
        target: if target_s.is_empty() { "This PC".into() } else { target_s },
        started: started.format("%Y-%m-%d %H:%M").to_string(),
        minutes: (t0.elapsed().as_secs_f64() / 60.0 * 10.0).round() / 10.0,
        ok: !stopped && threats.is_empty() && code == Some(0),
        skipped_by_exclusions: skipped.is_some(),
        threats,
        files,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threats_are_parsed() {
        let out = "Scan starting...\nScan finished.\nScanning C:\\x found 1 threats.\n\nThreat                  : Virus:DOS/EICAR_Test_File\nResources               : 1 total\n    file                : C:\\x\\eicar.com\n";
        let (t, f) = parse_output(out);
        assert_eq!(t, vec!["Virus:DOS/EICAR_Test_File"]);
        assert_eq!(f, vec!["C:\\x\\eicar.com"]);
    }
}
