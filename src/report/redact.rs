//! "Hide personal details" for reports you plan to share: serial numbers, network
//! addresses, account and PC names, email addresses and Wi-Fi names are replaced.

use crate::snapshot::Snapshot;
use regex::Regex;
use serde_json::Value;

const HIDDEN: &str = "(hidden)";
const KEYS: [&str; 13] = ["Serial", "UUID", "MAC", "Hostname", "RegisteredOwner", "Workgroup", "SSID", "IPv4", "Gateway", "DNS", "Address", "admins", "Sample"];

struct Scrubber {
    words: Vec<String>,
    patterns: Vec<(Regex, &'static str)>,
    address: Regex,
}

impl Scrubber {
    fn new(words: Vec<String>) -> Self {
        let p = |r: &str, to: &'static str| (Regex::new(r).expect("valid redaction pattern"), to);
        Scrubber {
            words: words.into_iter().filter(|w| w.len() >= 3).collect(),
            patterns: vec![
                p(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}", "(email hidden)"),
                p(r"\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b", "(hidden)"),
                p(r"(?i)(\\Users\\)[^\\]+", "${1}(user)"),
            ],
            address: Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").expect("valid redaction pattern"),
        }
    }
    /// A version such as 2.10.91.91 looks like a network address but says nothing about the
    /// person, so `version` text keeps its numbers. Names, emails and paths in it are still hidden.
    fn text(&self, s: &str, version: bool) -> String {
        let mut out = s.to_string();
        for (re, to) in &self.patterns {
            out = re.replace_all(&out, *to).into_owned();
        }
        if !version {
            out = self.address.replace_all(&out, "(address hidden)").into_owned();
        }
        for w in &self.words {
            if let Ok(re) = Regex::new(&format!("(?i){}", regex::escape(w))) {
                out = re.replace_all(&out, "(hidden)").into_owned();
            }
        }
        out
    }
    fn value(&self, v: &mut Value) {
        match v {
            Value::String(s) => *s = self.text(s, false),
            Value::Array(a) => a.iter_mut().for_each(|x| self.value(x)),
            Value::Object(m) => {
                for (k, x) in m.iter_mut() {
                    if KEYS.contains(&k.as_str()) && !x.is_null() {
                        *x = Value::String(HIDDEN.into());
                    } else if let (true, Value::String(s)) = (version_key(k), &mut *x) {
                        *s = self.text(s, true);
                    } else {
                        self.value(x);
                    }
                }
            }
            _ => {}
        }
    }
}

/// "Version", "DriverVersion", and an app update's "installed" and "available" versions.
fn version_key(k: &str) -> bool {
    k.to_ascii_lowercase().ends_with("version") || k == "installed" || k == "available"
}

pub fn redact(s: &Snapshot) -> Snapshot {
    let mut words: Vec<String> = ["USERNAME", "COMPUTERNAME", "USERDOMAIN"].iter().filter_map(|k| std::env::var(k).ok()).collect();
    words.push(s.host.clone());
    let sc = Scrubber::new(words);
    let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
    sc.value(&mut v);
    // Local account names are personal too.
    if let Some(Value::Array(accts)) = v.pointer_mut("/security/accounts") {
        for a in accts.iter_mut() {
            if let Some(n) = a.get_mut("Name") {
                *n = Value::String(HIDDEN.into());
            }
        }
    }
    // So are the other administrators' names. The list keeps its length, so the advice about them still shows.
    if let Some(Value::Array(names)) = v.pointer_mut("/security/dailyAccount/OtherAdmins") {
        names.iter_mut().for_each(|n| *n = Value::String(HIDDEN.into()));
    }
    let mut out: Snapshot = serde_json::from_value(v).unwrap_or_default();
    out.host = "This PC".into();
    // Browser profile names are often a person's name.
    for e in &mut out.extensions {
        e.profile = HIDDEN.into();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn personal_details_are_hidden() {
        let ext = crate::snapshot::Extension { profile: "Alice work".into(), name: "uBlock".into(), ..Default::default() };
        let mut s = Snapshot { host: "GAMINGRIG".into(), extensions: vec![ext], ..Default::default() };
        s.system =
            json!({"device": {"Serial": "ABC123", "Model": "B450"}, "note": "C:\\Users\\alice\\Downloads mail me at a@b.com from 192.168.1.4 GAMINGRIG"});
        let r = redact(&s);
        assert_eq!(r.system["device"]["Serial"], "(hidden)");
        assert_eq!(r.system["device"]["Model"], "B450");
        let note = r.system["note"].as_str().unwrap();
        assert!(!note.contains("alice") && !note.contains("a@b.com") && !note.contains("192.168") && !note.contains("GAMINGRIG"), "{note}");
        assert_eq!(r.host, "This PC");
        assert_eq!(r.extensions[0].profile, HIDDEN);
        assert_eq!(r.extensions[0].name, "uBlock");
    }

    #[test]
    fn other_administrators_names_are_hidden_but_still_counted() {
        let s = Snapshot { security: json!({"dailyAccount": {"Name": "PC\\sam", "Admin": true, "OtherAdmins": ["Helper", "Grandad"]}}), ..Default::default() };
        let r = redact(&s);
        assert_eq!(r.security["dailyAccount"]["OtherAdmins"], json!([HIDDEN, HIDDEN]));
        assert_eq!(r.security["dailyAccount"]["Admin"], true);
    }

    #[test]
    fn version_numbers_are_not_mistaken_for_addresses() {
        let up = crate::snapshot::AppUpgrade { name: "Parsec".into(), installed: "1.0.0.0".into(), available: "2.12.5.0".into(), ..Default::default() };
        let mut s = Snapshot { upgrades: Some(vec![up]), ..Default::default() };
        s.software = json!({"apps": [{"Name": "Steam", "Version": "2.10.91.91", "Note": "talks to 10.0.0.2"}]});
        s.system = json!({"gpu": {"DriverVersion": "31.0.15.362"}});
        let r = redact(&s);
        assert_eq!(r.software["apps"][0]["Version"], "2.10.91.91");
        assert_eq!(r.software["apps"][0]["Note"], "talks to (address hidden)");
        assert_eq!(r.system["gpu"]["DriverVersion"], "31.0.15.362");
        s.software["apps"][0]["DisplayVersion"] = json!("1.2.3.4 for sam@x.org in C:\\Users\\sam");
        let free_text = redact(&s).software["apps"][0]["DisplayVersion"].clone();
        assert_eq!(free_text, "1.2.3.4 for (email hidden) in C:\\Users\\(user)", "only the numbers are spared");
        let up = &r.upgrades.unwrap()[0];
        assert_eq!((up.installed.as_str(), up.available.as_str()), ("1.0.0.0", "2.12.5.0"));
    }
}
