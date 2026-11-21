//! Saving a check as a self-contained web page or PDF.

mod html;
pub mod redact;

use crate::snapshot::Snapshot;
use crate::util::run_hidden;
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
    crate::sys::program_files().into_iter().map(|b| b.join(r"Microsoft\Edge\Application\msedge.exe")).find(|p| p.is_file())
}

/// A file:/// link to `path` that Edge opens. Anything a link treats specially, such as a space,
/// "#" or "%" in a folder name, is written as %XX, so it stays part of the name.
fn file_url(path: &Path) -> String {
    let mut url = String::from("file:///");
    for b in path.to_string_lossy().replace('\\', "/").bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => url.push(b as char),
            _ => url.push_str(&format!("%{b:02X}")),
        }
    }
    url
}

/// Render the HTML report to PDF with Microsoft Edge in headless mode.
pub fn write_pdf(s: &Snapshot, hide_personal: bool, out: &Path) -> Result<()> {
    let edge = edge().ok_or_else(|| {
        log!("report", "PDF report failed: Microsoft Edge was not found");
        anyhow!("Saving as PDF uses Microsoft Edge, which wasn't found. Save as a web page instead and print it from any browser.")
    })?;
    // Edge reads its profile from here, so in an administrator window it must be a folder only administrators can change.
    let tmp = crate::sys::tmp_dir()?;
    let html = tmp.join(format!("report-{}.html", s.id));
    save_html(s, hide_personal, &html).inspect_err(|e| log!("report", "PDF report failed: {e:#}"))?;
    let profile = tmp.join("edge-print-profile");
    let url = file_url(&html);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_link_keeps_spaces_hashes_and_percent_signs_as_part_of_the_name() {
        let url = file_url(Path::new("C:\\Users\\Jo #1\\100% done\\r\u{e9}port.html"));
        assert_eq!(url, "file:///C:/Users/Jo%20%231/100%25%20done/r%C3%A9port.html");
    }
}
