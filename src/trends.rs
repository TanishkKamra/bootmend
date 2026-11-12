//! A few numbers followed across saved checks, so slow changes (a filling drive, a wearing battery,
//! more apps starting with Windows) show up before they become a problem.

use crate::rules::{evaluate, Finding, Level};
use crate::snapshot::{self, Snapshot};
use crate::util::J;

/// At most this many recent checks are drawn.
const SPAN: usize = 30;

#[derive(Debug, Clone, PartialEq)]
pub struct Trend {
    pub id: &'static str,
    pub title: &'static str,
    pub unit: &'static str,
    /// When each check was taken, and the value then. Oldest first.
    pub points: Vec<(String, f64)>,
    /// Whether going up is good news, which decides the wording of the change.
    pub up_is_good: bool,
}

/// The numbers followed for one check.
pub fn metrics(s: &Snapshot, findings: &[Finding]) -> Vec<(&'static str, f64)> {
    let mut m = vec![("suggestions", findings.iter().filter(|f| matches!(f.level, Level::DoFirst | Level::WorthDoing)).count() as f64)];
    if let Some(free) = s.storage.a("volumes").into_iter().find(|v| v.s("Letter").as_deref() == Some("C:")).and_then(|v| v.f("FreeGB")) {
        m.push(("free-c", free));
    }
    let startup = s.software.a("startup");
    if !startup.is_empty() {
        m.push(("startup", startup.iter().filter(|i| i.b("Enabled") != Some(false)).count() as f64));
    }
    let b = s.system.g("battery");
    if let (Some(design), Some(full)) = (b.f("DesignmWh"), b.f("FullChargemWh")) {
        if design > 0.0 {
            m.push(("battery", (full / design * 100.0).min(100.0)));
        }
    }
    m
}

/// Builds the trends from checks given oldest first. A trend needs at least two checks to show.
pub fn build(checks: &[(String, Vec<(&'static str, f64)>)]) -> Vec<Trend> {
    let defs: [(&'static str, &'static str, &'static str, bool); 4] = [
        ("free-c", "Free space on drive C", " GB", true),
        ("suggestions", "Do first and worth doing", "", false),
        ("startup", "Apps starting with Windows", "", false),
        ("battery", "Battery health", "%", true),
    ];
    defs.iter()
        .filter_map(|(id, title, unit, up)| {
            let points: Vec<(String, f64)> = checks.iter().filter_map(|(when, m)| m.iter().find(|(k, _)| k == id).map(|(_, v)| (when.clone(), *v))).collect();
            (points.len() >= 2).then_some(Trend { id, title, unit, points, up_is_good: *up })
        })
        .collect()
}

/// The change from the first to the last check, in words, and whether it's good news.
pub fn change_words(t: &Trend) -> (String, Option<bool>) {
    let (Some((since, a)), Some((_, b))) = (t.points.first(), t.points.last()) else { return (String::new(), None) };
    let d = b - a;
    let date = since.split(' ').next().unwrap_or(since);
    // Small wobbles (a few GB on a big drive) aren't news.
    let noise = t.points.iter().map(|p| p.1.abs()).fold(0.0, f64::max) * 0.02;
    if d.abs() < noise.max(0.5) {
        return (format!("About the same since {date}"), None);
    }
    let dir = if d > 0.0 { "Up" } else { "Down" };
    (format!("{dir} {:.0}{} since {date}", d.abs(), t.unit), Some((d > 0.0) == t.up_is_good))
}

/// Reads the recent saved checks for this PC. Slow-ish (it reads files), so it runs off the window's thread.
pub fn load(host: &str) -> Vec<Trend> {
    let files = snapshot::list(host).unwrap_or_default();
    let recent = &files[files.len().saturating_sub(SPAN)..];
    let checks: Vec<(String, Vec<(&'static str, f64)>)> = recent
        .iter()
        .filter_map(|p| snapshot::load(p).ok())
        .map(|s| {
            let f = evaluate(&s);
            (s.taken.clone(), metrics(&s, &f))
        })
        .collect();
    build(&checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(when: &str, free: f64, sugg: f64) -> (String, Vec<(&'static str, f64)>) {
        (when.into(), vec![("free-c", free), ("suggestions", sugg)])
    }

    #[test]
    fn trends_need_two_checks_and_keep_order() {
        assert!(build(&[check("2026-10-01 10:00", 100.0, 3.0)]).is_empty());
        let t = build(&[check("2026-10-01 10:00", 100.0, 3.0), check("2026-11-01 10:00", 60.0, 3.0)]);
        assert_eq!(t[0].id, "free-c");
        assert_eq!(t[0].points.iter().map(|p| p.1).collect::<Vec<_>>(), vec![100.0, 60.0]);
        assert!(t.iter().all(|x| x.id != "battery"));
    }

    #[test]
    fn change_is_worded_by_whether_it_is_good_news() {
        let t = build(&[check("2026-10-01 10:00", 100.0, 3.0), check("2026-11-01 10:00", 60.0, 1.0)]);
        assert_eq!(change_words(&t[0]), ("Down 40 GB since 2026-10-01".to_string(), Some(false)));
        assert_eq!(change_words(&t[1]), ("Down 2 since 2026-10-01".to_string(), Some(true)));
        let flat = build(&[check("2026-10-01 10:00", 50.0, 2.0), check("2026-11-01 10:00", 50.2, 2.0)]);
        assert_eq!(change_words(&flat[0]).1, None);
        let wobble = build(&[check("2026-10-01 10:00", 168.0, 2.0), check("2026-11-01 10:00", 167.0, 2.0)]);
        assert_eq!(change_words(&wobble[0]).1, None, "1 GB on a 168 GB drive is noise");
    }
}
