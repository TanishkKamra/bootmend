//! Saving a check as a self-contained web page or PDF.

mod html;
pub mod redact;

use crate::snapshot::Snapshot;
use crate::util::{data_dir, run_hidden};
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use html::render;

fn save_html(s: &Snapshot, hide_personal: bool, out: &Path) -> Result<()> {
    let snap = if hide_personal { redact::redact(s) } else { s.clone() };
    std::fs::write(out, render(&snap, hide_personal)).with_context(|| format!("saving {}", out.display()))
}

pub fn write_html(s: &Snapshot, hide_personal: bool, out: &Path) -> Result<()> {
    let r = save_html(s, hide_personal, out);
    match &r {
        Ok(()) => log!("report", "web page report written to {} (personal details hidden: {})", out.display(), hide_personal),
        Err(e) => log!("report", "web page report failed: {e:#}"),
    }
    r
}

fn edge() -> Option<PathBuf> {
    ["ProgramFiles(x86)", "ProgramFiles"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|b| PathBuf::from(b).join(r"Microsoft\Edge\Application\msedge.exe"))
        .find(|p| p.is_file())
}

/// Render the HTML report to PDF with Microsoft Edge in headless mode.
pub fn write_pdf(s: &Snapshot, hide_personal: bool, out: &Path) -> Result<()> {
    let edge = edge().ok_or_else(|| {
        log!("report", "PDF report failed: Microsoft Edge was not found");
        anyhow!("Saving as PDF uses Microsoft Edge, which wasn't found. Save as a web page instead and print it from any browser.")
    })?;
    let tmp = data_dir("tmp")?;
    let html = tmp.join(format!("report-{}.html", s.id));
    save_html(s, hide_personal, &html).inspect_err(|e| log!("report", "PDF report failed: {e:#}"))?;
    let profile = tmp.join("edge-print-profile");
    let url = format!("file:///{}", html.to_string_lossy().replace('\\', "/"));
    let r = run_hidden(
        &edge.to_string_lossy(),
        &[
            "--headless=new",
            "--disable-gpu",
            "--no-first-run",
            // Otherwise Edge may start downloading its own updates and leave them in the temp folder.
            "--disable-component-update",
            "--no-pdf-header-footer",
            &format!("--user-data-dir={}", profile.display()),
            &format!("--print-to-pdf={}", out.display()),
            &url,
        ],
        Duration::from_secs(120),
    );
    if let Err(e) = std::fs::remove_file(&html) {
        log!("report", "could not remove temporary report: {e}");
    }
    // Edge fills its profile with caches; the next PDF makes a fresh one.
    if let Err(e) = std::fs::remove_dir_all(&profile) {
        log!("report", "could not remove Edge's temporary profile: {e}");
    }
    r.inspect_err(|e| log!("report", "PDF report failed: {e:#}"))?;
    if out.is_file() {
        log!("report", "PDF report written to {} (personal details hidden: {})", out.display(), hide_personal);
        Ok(())
    } else {
        log!("report", "PDF report failed: Edge did not produce the file");
        Err(anyhow!("Edge did not produce the PDF"))
    }
}

pub fn default_name(s: &Snapshot, ext: &str) -> String {
    format!("bootmend-{}-{}.{ext}", s.host.replace(|c: char| !c.is_ascii_alphanumeric(), "-"), s.id)
}
