//! App updates through winget, Windows' own package manager. Only runs when asked,
//! because it contacts Microsoft's community app catalog.

use crate::snapshot::AppUpgrade;
use crate::util::run_hidden;
use anyhow::{anyhow, Result};
use std::time::Duration;

pub fn available() -> bool {
    std::env::var_os("LOCALAPPDATA").map(|l| std::path::PathBuf::from(l).join(r"Microsoft\WindowsApps\winget.exe").exists()).unwrap_or(false)
}

/// Parse `winget upgrade` table output. Column starts come from the header line.
pub fn parse_upgrades(text: &str) -> Vec<AppUpgrade> {
    let mut out = Vec::new();
    let mut cols: Option<[usize; 4]> = None;
    for line in text.split(['\n', '\r']) {
        let chars: Vec<char> = line.chars().collect();
        let find = |w: &str| line.find(w).map(|b| line[..b].chars().count());
        if let (Some(i), Some(v), Some(a)) = (find(" Id "), find(" Version "), find(" Available ")) {
            if line.trim_start().starts_with("Name") {
                cols = Some([0, i + 1, v + 1, a + 1]);
                continue;
            }
        }
        let Some([_, i, v, a]) = cols else { continue };
        if line.trim_start().starts_with("---") || chars.len() <= a {
            if !line.trim().is_empty() && !line.trim_start().starts_with("---") {
                cols = None; // a footer line such as "3 upgrades available." ends the table
            }
            continue;
        }
        let take = |from: usize, to: usize| chars[from.min(chars.len())..to.min(chars.len())].iter().collect::<String>().trim().to_string();
        let rest = take(a, chars.len());
        let available = rest.split_whitespace().next().unwrap_or("").to_string();
        let row = AppUpgrade { name: take(0, i), id: take(i, v), installed: take(v, a), available };
        if !row.id.is_empty() && !row.available.is_empty() && !row.id.contains(' ') {
            out.push(row);
        }
    }
    out
}

pub fn check() -> Result<Vec<AppUpgrade>> {
    let out = run_hidden(
        "winget",
        &["upgrade", "--include-unknown", "--source", "winget", "--accept-source-agreements", "--disable-interactivity"],
        Duration::from_secs(180),
    )?;
    if out.timed_out {
        return Err(anyhow!("winget took too long to answer"));
    }
    let list = parse_upgrades(&out.stdout);
    if list.is_empty() && out.code != Some(0) && !out.stdout.contains("No installed package") {
        return Err(anyhow!("winget could not check for updates (code {:?})", out.code));
    }
    Ok(list)
}

/// Update one app silently through winget.
pub fn upgrade(id: &str) -> Result<String> {
    if id.is_empty() || id.chars().any(|c| c.is_whitespace() || c == '"') {
        return Err(anyhow!("that app id does not look right"));
    }
    let out = run_hidden(
        "winget",
        &[
            "upgrade",
            "--id",
            id,
            "--exact",
            "--silent",
            "--source",
            "winget",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ],
        Duration::from_secs(900),
    )?;
    if out.code == Some(0) {
        Ok(format!("{id} is up to date"))
    } else {
        let last = out.stdout.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no details").trim().to_string();
        Err(anyhow!("winget could not update {id}: {last}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\r   - \r   \\ \r\nName                       Id                    Version      Available    Source\n------------------------------------------------------------------------------------\nMozilla Firefox (x64 en-US) Mozilla.Firefox      131.0        132.0.1      winget\n7-Zip 24.08 (x64)          7zip.7zip             24.08        25.01        winget\nVisual Studio Code         Microsoft.VisualStud… 1.94.0       1.95.1       winget\n3 upgrades available.\n";

    #[test]
    fn upgrade_table_is_parsed() {
        let v = parse_upgrades(SAMPLE);
        assert_eq!(v.len(), 3, "{v:?}");
        assert_eq!(v[1].id, "7zip.7zip");
        assert_eq!(v[1].installed, "24.08");
        assert_eq!(v[1].available, "25.01");
        assert_eq!(v[2].id, "Microsoft.VisualStud…");
    }

    #[test]
    fn empty_output_gives_nothing() {
        assert!(parse_upgrades("No installed package found matching input criteria.").is_empty());
    }
}
