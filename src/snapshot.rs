//! One complete check of a PC, saved as JSON so later checks can show what changed.

use crate::util::data_dir;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub version: u32,
    pub id: String,
    pub host: String,
    pub taken: String,
    pub elevated: bool,
    pub system: Value,
    pub storage: Value,
    pub network: Value,
    pub security: Value,
    pub software: Value,
    pub health: Value,
    pub drives: Vec<DriveHealth>,
    pub extensions: Vec<Extension>,
    pub dumps: Vec<DumpAnalysis>,
    /// `None` until the person asks rigscan to check for app updates (it uses the internet).
    pub upgrades: Option<Vec<AppUpgrade>>,
    pub scans: Vec<ScanResult>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DriveHealth {
    pub disk: i64,
    pub model: String,
    pub kind: String,
    /// NVMe "percentage used" or SATA SSD wear, 0 = new.
    pub life_used_pct: Option<f64>,
    pub power_on_hours: Option<f64>,
    pub tb_written: Option<f64>,
    pub temp_c: Option<f64>,
    pub reallocated: Option<f64>,
    pub pending: Option<f64>,
    pub uncorrectable: Option<f64>,
    pub media_errors: Option<f64>,
    pub unsafe_shutdowns: Option<f64>,
    pub spare_pct: Option<f64>,
    pub predict_failure: Option<bool>,
    pub source: String,
    pub attributes: Vec<SmartAttr>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SmartAttr {
    pub id: u8,
    pub name: String,
    pub value: u8,
    pub worst: u8,
    pub raw: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Extension {
    pub browser: String,
    pub profile: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub from_store: bool,
    pub permissions: Vec<String>,
    pub all_sites: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DumpAnalysis {
    pub file: String,
    pub date: String,
    pub code: String,
    pub name: String,
    pub module: Option<String>,
    pub image: Option<String>,
    pub process: Option<String>,
    pub bucket: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppUpgrade {
    pub name: String,
    pub id: String,
    pub installed: String,
    pub available: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanResult {
    pub kind: String,
    pub target: String,
    pub started: String,
    pub minutes: f64,
    pub threats: Vec<String>,
    pub files: Vec<String>,
    pub skipped_by_exclusions: bool,
    pub ok: bool,
    pub message: String,
}

fn host_dir(host: &str) -> Result<PathBuf> {
    let safe: String = host.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let d = data_dir("snapshots")?.join(if safe.is_empty() { "this-pc".into() } else { safe });
    std::fs::create_dir_all(&d).with_context(|| format!("creating {}", d.display()))?;
    Ok(d)
}

pub fn save(s: &Snapshot) -> Result<PathBuf> {
    let p = host_dir(&s.host)?.join(format!("{}.json", s.id));
    let json = serde_json::to_vec_pretty(s)?;
    std::fs::write(&p, json).with_context(|| format!("saving {}", p.display()))?;
    Ok(p)
}

/// All saved checks for this PC, oldest first.
pub fn list(host: &str) -> Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(host_dir(host)?)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    v.sort();
    Ok(v)
}

pub fn load(p: &PathBuf) -> Result<Snapshot> {
    let b = std::fs::read(p).with_context(|| format!("reading {}", p.display()))?;
    serde_json::from_slice(&b).with_context(|| format!("parsing {}", p.display()))
}

/// The newest saved check other than `except_id`.
pub fn previous(host: &str, except_id: &str) -> Option<Snapshot> {
    let files = match list(host) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("could not list saved checks: {e}");
            return None;
        }
    };
    files.iter().rev().filter(|p| !p.to_string_lossy().contains(except_id)).find_map(|p| match load(p) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("skipping unreadable saved check {}: {e}", p.display());
            None
        }
    })
}

pub fn latest(host: &str) -> Option<Snapshot> {
    previous(host, "\u{0}")
}
