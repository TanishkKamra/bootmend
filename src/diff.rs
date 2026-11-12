//! What changed between two checks of the same PC.

use crate::snapshot::Snapshot;
use crate::util::J;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone)]
pub struct Change {
    pub group: &'static str,
    pub kind: Kind,
    pub text: String,
    /// True for changes worth a second look, such as a new admin account or skip-list entry.
    pub notable: bool,
}

fn keyed(items: impl IntoIterator<Item = (String, String)>) -> BTreeMap<String, String> {
    items.into_iter().collect()
}

fn compare(out: &mut Vec<Change>, group: &'static str, old: BTreeMap<String, String>, new: BTreeMap<String, String>, notable_added: bool, show_changed: bool) {
    for (k, v) in &new {
        match old.get(k) {
            None => out.push(Change { group, kind: Kind::Added, text: if v.is_empty() { k.clone() } else { format!("{k} {v}") }, notable: notable_added }),
            Some(o) if show_changed && o != v => out.push(Change { group, kind: Kind::Changed, text: format!("{k}: {o} to {v}"), notable: false }),
            _ => {}
        }
    }
    for k in old.keys().filter(|k| !new.contains_key(*k)) {
        out.push(Change { group, kind: Kind::Removed, text: k.clone(), notable: false });
    }
}

pub fn diff(old: &Snapshot, new: &Snapshot) -> Vec<Change> {
    let mut out = Vec::new();
    let apps = |s: &Snapshot| keyed(s.software.a("apps").into_iter().filter_map(|a| Some((a.s("Name")?, a.s("Version").unwrap_or_default()))));
    compare(&mut out, "Apps", apps(old), apps(new), false, true);

    let ext = |s: &Snapshot| {
        keyed(s.extensions.iter().map(|e| (format!("{} ({})", e.name, e.browser), if e.from_store { String::new() } else { "from outside the store".into() })))
    };
    compare(&mut out, "Browser extensions", ext(old), ext(new), true, false);

    let startup = |s: &Snapshot| keyed(s.software.a("startup").into_iter().filter_map(|x| Some((x.s("Name")?, String::new()))));
    compare(&mut out, "Starts with Windows", startup(old), startup(new), true, false);

    let svc = |s: &Snapshot| keyed(s.software.a("services").into_iter().filter_map(|x| Some((x.s("Display").or(x.s("Name"))?, String::new()))));
    compare(&mut out, "Background services", svc(old), svc(new), true, false);

    let tasks = |s: &Snapshot| {
        keyed(s.software.a("tasks").into_iter().filter_map(|x| Some((format!("{}{}", x.s("Path").unwrap_or_default(), x.s("Name")?), String::new()))))
    };
    compare(&mut out, "Scheduled tasks", tasks(old), tasks(new), true, false);

    let excl = |s: &Snapshot| {
        let d = s.security.g("defender");
        keyed(
            ["ExclusionPaths", "ExclusionProcesses", "ExclusionExtensions"]
                .iter()
                .flat_map(|k| d.strs(k))
                .filter(|v| !v.starts_with("N/A"))
                .map(|v| (v, String::new())),
        )
    };
    compare(&mut out, "Windows Security skip list", excl(old), excl(new), true, false);

    let admins = |s: &Snapshot| keyed(s.security.strs("admins").into_iter().map(|a| (a, String::new())));
    compare(&mut out, "Administrator accounts", admins(old), admins(new), true, false);

    let ports =
        |s: &Snapshot| keyed(s.network.a("listening").into_iter().filter_map(|x| Some((format!("Port {}", x.i("Port")?), x.s("Process").unwrap_or_default()))));
    compare(&mut out, "Open network ports", ports(old), ports(new), true, false);

    let fw = |s: &Snapshot| keyed(s.security.a("firewallRules").into_iter().filter_map(|x| Some((x.s("DisplayName")?, String::new()))));
    compare(&mut out, "Firewall allow rules", fw(old), fw(new), false, false);

    let new_crashes = new.health.a("unexpectedShutdowns").into_iter().filter(|e| e.s("Date").is_some_and(|d| d > old.taken)).count();
    if new_crashes > 0 {
        out.push(Change {
            group: "Crashes",
            kind: Kind::Added,
            text: format!("{new_crashes} unexpected shutdown{} since the last check", if new_crashes == 1 { "" } else { "s" }),
            notable: true,
        });
    }

    for d in &new.drives {
        if let Some(o) = old.drives.iter().find(|o| o.model == d.model) {
            for (label, a, b) in [
                ("reallocated sectors", o.reallocated, d.reallocated),
                ("pending sectors", o.pending, d.pending),
                ("media errors", o.media_errors, d.media_errors),
            ] {
                if let (Some(a), Some(b)) = (a, b) {
                    if b > a {
                        out.push(Change {
                            group: "Drive health",
                            kind: Kind::Changed,
                            text: format!("{}: {label} went from {a} to {b}", d.model),
                            notable: true,
                        });
                    }
                }
            }
            if let (Some(a), Some(b)) = (o.life_used_pct, d.life_used_pct) {
                if b > a {
                    out.push(Change { group: "Drive health", kind: Kind::Changed, text: format!("{}: wear went from {a}% to {b}%", d.model), notable: false });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_added_removed_and_updated() {
        let a = Snapshot {
            software: json!({"apps": [{"Name": "7-Zip", "Version": "24.08"}, {"Name": "Old App", "Version": "1"}]}),
            security: json!({"defender": {"ExclusionPaths": ["N/A: Must be an administrator to view exclusions"]}, "admins": ["PC\\me"]}),
            ..Default::default()
        };
        let mut b = a.clone();
        b.software = json!({"apps": [{"Name": "7-Zip", "Version": "25.01"}, {"Name": "New App", "Version": "2"}]});
        b.security = json!({"defender": {"ExclusionPaths": ["C:\\Games"]}, "admins": ["PC\\me", "PC\\helper"]});
        let d = diff(&a, &b);
        assert!(d.iter().any(|c| c.kind == Kind::Changed && c.text.contains("24.08 to 25.01")));
        assert!(d.iter().any(|c| c.kind == Kind::Added && c.text.starts_with("New App")));
        assert!(d.iter().any(|c| c.kind == Kind::Removed && c.text == "Old App"));
        assert!(d.iter().any(|c| c.group == "Windows Security skip list" && c.notable));
        assert!(d.iter().any(|c| c.group == "Administrator accounts" && c.text == "PC\\helper"));
    }
}
