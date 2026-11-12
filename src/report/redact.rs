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
}

impl Scrubber {
    fn new(words: Vec<String>) -> Self {
        let p = |r: &str, to: &'static str| (Regex::new(r).expect("valid redaction pattern"), to);
        Scrubber {
            words: words.into_iter().filter(|w| w.len() >= 3).collect(),
            patterns: vec![
                p(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}", "(email hidden)"),
                p(r"\b(?:\d{1,3}\.){3}\d{1,3}\b", "(address hidden)"),
                p(r"\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b", "(hidden)"),
                p(r"(?i)(\\Users\\)[^\\]+", "${1}(user)"),
            ],
        }
    }
    fn text(&self, s: &str) -> String {
        let mut out = s.to_string();
        for (re, to) in &self.patterns {
            out = re.replace_all(&out, *to).into_owned();
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
            Value::String(s) => *s = self.text(s),
            Value::Array(a) => a.iter_mut().for_each(|x| self.value(x)),
            Value::Object(m) => {
                for (k, x) in m.iter_mut() {
                    if KEYS.contains(&k.as_str()) && !x.is_null() {
                        *x = Value::String(HIDDEN.into());
                    } else {
                        self.value(x);
                    }
                }
            }
            _ => {}
        }
    }
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
}
