//! Saving a check as a self-contained web page or PDF.

mod html;
pub mod redact;

use crate::snapshot::Snapshot;
use crate::util::{data_dir, run_hidden};
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use html::render;

pub fn write_html(s: &Snapshot, hide_personal: bool, out: &Path) -> Result<()> {
    let snap = if hide_personal { redact::redact(s) } else { s.clone() };
    std::fs::write(out, render(&snap, hide_personal)).with_context(|| format!("saving {}", out.display()))
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
    let edge =
        edge().ok_or_else(|| anyhow!("Saving as PDF uses Microsoft Edge, which wasn't found. Save as a web page instead and print it from any browser."))?;
    let tmp = data_dir("tmp")?;
    let html = tmp.join(format!("report-{}.html", s.id));
    write_html(s, hide_personal, &html)?;
    let profile = tmp.join("edge-print-profile");
    let url = format!("file:///{}", html.to_string_lossy().replace('\\', "/"));
    let r = run_hidden(
        &edge.to_string_lossy(),
        &[
            "--headless=new",
            "--disable-gpu",
            "--no-first-run",
            "--no-pdf-header-footer",
            &format!("--user-data-dir={}", profile.display()),
            &format!("--print-to-pdf={}", out.display()),
            &url,
        ],
        Duration::from_secs(120),
    );
    if let Err(e) = std::fs::remove_file(&html) {
        eprintln!("could not remove temporary report: {e}");
    }
    r?;
    if out.is_file() {
        Ok(())
    } else {
        Err(anyhow!("Edge did not produce the PDF"))
    }
}

pub fn default_name(s: &Snapshot, ext: &str) -> String {
    format!("rigscan-{}-{}.{ext}", s.host.replace(|c: char| !c.is_ascii_alphanumeric(), "-"), s.id)
}
