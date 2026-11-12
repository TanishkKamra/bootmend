//! Browser extensions, read straight from each browser's profile folders. Nothing is changed.

use crate::snapshot::Extension;
use crate::util::{version_lt, J};
use serde_json::Value;
use std::path::{Path, PathBuf};

const ALL_SITES: [&str; 5] = ["<all_urls>", "*://*/*", "http://*/*", "https://*/*", "*://*"];

fn read_json(p: &Path) -> Option<Value> {
    let b = std::fs::read(p).ok()?;
    serde_json::from_slice(&b).ok()
}

pub fn collect() -> Vec<Extension> {
    let mut out = Vec::new();
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from);
    if let Some(l) = &local {
        for (name, dir) in [
            ("Chrome", l.join(r"Google\Chrome\User Data")),
            ("Edge", l.join(r"Microsoft\Edge\User Data")),
            ("Brave", l.join(r"BraveSoftware\Brave-Browser\User Data")),
            ("Vivaldi", l.join(r"Vivaldi\User Data")),
        ] {
            chromium(name, &dir, &mut out);
        }
    }
    if let Some(r) = &roaming {
        let opera = r.join(r"Opera Software\Opera Stable");
        if opera.join("Extensions").is_dir() {
            chromium_profile("Opera", "Default", &opera, &mut out);
        }
        firefox(&r.join(r"Mozilla\Firefox\Profiles"), &mut out);
    }
    out.sort_by(|a, b| (&a.browser, a.name.to_lowercase()).cmp(&(&b.browser, b.name.to_lowercase())));
    out
}

fn chromium(browser: &str, user_data: &Path, out: &mut Vec<Extension>) {
    let Ok(rd) = std::fs::read_dir(user_data) else { return };
    let names = read_json(&user_data.join("Local State"));
    for e in rd.flatten() {
        let dir_name = e.file_name().to_string_lossy().to_string();
        if !(dir_name == "Default" || dir_name.starts_with("Profile ")) || !e.path().is_dir() {
            continue;
        }
        let shown = names.as_ref().and_then(|n| n.g("profile").g("info_cache").g(&dir_name).s("name")).unwrap_or_else(|| dir_name.clone());
        let start = out.len();
        chromium_profile(browser, &shown, &e.path(), out);
        for x in &mut out[start..] {
            x.profile_dir = dir_name.clone();
        }
    }
}

/// The browser's program and the address of an extension's own page in it.
pub fn manage_target(e: &Extension) -> Option<(&'static str, String)> {
    let id = &e.id;
    Some(match e.browser.as_str() {
        "Chrome" => ("chrome.exe", format!("chrome://extensions/?id={id}")),
        "Edge" => ("msedge.exe", format!("edge://extensions/?id={id}")),
        "Brave" => ("brave.exe", format!("brave://extensions/?id={id}")),
        "Vivaldi" => ("vivaldi.exe", format!("vivaldi://extensions/?id={id}")),
        "Opera" => ("opera.exe", "opera://extensions".to_string()),
        "Firefox" => ("firefox.exe", "about:addons".to_string()),
        _ => return None,
    })
}

/// Opens the extension's page in its browser and profile, where it can be switched off or removed.
pub fn open_manage(e: &Extension) -> anyhow::Result<()> {
    let (exe, url) = manage_target(e).ok_or_else(|| anyhow::anyhow!("rigscan doesn't know how to open {}", e.browser))?;
    let params = if e.profile_dir.is_empty() { url } else { format!("--profile-directory=\"{}\" {url}", e.profile_dir) };
    crate::util::shell_run(exe, &params).map_err(|_| anyhow::anyhow!("Windows couldn't open {}. Open it yourself and look under Extensions.", e.browser))
}

/// Extension settings live in both Preferences and Secure Preferences; merge them.
fn ext_settings(profile: &Path) -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    for f in ["Preferences", "Secure Preferences"] {
        if let Some(Value::Object(s)) = read_json(&profile.join(f)).map(|v| v.g("extensions").g("settings").clone()) {
            for (k, v) in s {
                match (m.get_mut(&k), v) {
                    (Some(Value::Object(a)), Value::Object(b)) => a.extend(b),
                    (_, v) => {
                        m.insert(k, v);
                    }
                }
            }
        }
    }
    m
}

fn localized(manifest: &Value, dir: &Path, raw: &str) -> String {
    let Some(key) = raw.strip_prefix("__MSG_").and_then(|k| k.strip_suffix("__")) else { return raw.to_string() };
    let mut locales = vec![];
    if let Some(d) = manifest.s("default_locale") {
        locales.push(d);
    }
    locales.extend(["en".to_string(), "en_US".to_string()]);
    for loc in locales {
        if let Some(Value::Object(msgs)) = read_json(&dir.join("_locales").join(&loc).join("messages.json")) {
            if let Some((_, v)) = msgs.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
                if let Some(s) = v.s("message") {
                    return s;
                }
            }
        }
    }
    raw.to_string()
}

fn newest_version_dir(ext_dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(ext_dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.join("manifest.json").is_file()).max_by(|a, b| {
        let (x, y) = (a.file_name().unwrap_or_default().to_string_lossy(), b.file_name().unwrap_or_default().to_string_lossy());
        if version_lt(&x, &y) {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    })
}

fn permissions(manifest: &Value) -> Vec<String> {
    let mut p: Vec<String> = manifest.strs("permissions");
    p.extend(manifest.strs("host_permissions"));
    p.extend(manifest.strs("optional_host_permissions"));
    for cs in manifest.a("content_scripts") {
        p.extend(cs.strs("matches"));
    }
    p.sort();
    p.dedup();
    p
}

fn from_manifest(browser: &str, profile: &str, id: &str, dir: &Path, settings: Option<&Value>) -> Option<Extension> {
    let manifest = read_json(&dir.join("manifest.json"))?;
    if manifest.get("theme").is_some() {
        return None;
    }
    let name = localized(&manifest, dir, &manifest.s("name").unwrap_or_else(|| id.to_string()));
    let empty = Value::Null;
    let st = settings.unwrap_or(&empty);
    let disabled_reasons = match st.g("disable_reasons") {
        Value::Array(a) => !a.is_empty(),
        Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
        _ => false,
    };
    let enabled = st.i("state") != Some(0) && !disabled_reasons;
    let update = manifest.s("update_url").unwrap_or_default();
    let from_store = st.b("from_webstore") == Some(true)
        || update.contains("clients2.google.com")
        || update.contains("edge.microsoft.com")
        || update.contains("extension-updates.opera.com");
    let perms = permissions(&manifest);
    Some(Extension {
        browser: browser.into(),
        profile: profile.into(),
        profile_dir: String::new(),
        id: id.into(),
        name,
        version: manifest.s("version").unwrap_or_default(),
        enabled,
        from_store,
        all_sites: perms.iter().any(|p| ALL_SITES.contains(&p.as_str())),
        permissions: perms,
    })
}

fn chromium_profile(browser: &str, profile: &str, dir: &Path, out: &mut Vec<Extension>) {
    let settings = ext_settings(dir);
    let mut seen = std::collections::HashSet::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("Extensions")) {
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            if id.len() != 32 {
                continue;
            }
            let st = settings.get(&id);
            // Location 5 and 10 are extensions built into the browser itself.
            if matches!(st.and_then(|s| s.i("location")), Some(5) | Some(10)) {
                continue;
            }
            if let Some(x) = newest_version_dir(&e.path()).and_then(|v| from_manifest(browser, profile, &id, &v, st)) {
                seen.insert(id);
                out.push(x);
            }
        }
    }
    // Unpacked or command-line extensions live outside the Extensions folder (locations 4 and 8).
    for (id, st) in &settings {
        if seen.contains(id) || !matches!(st.i("location"), Some(4) | Some(8)) {
            continue;
        }
        if let Some(p) = st.s("path").map(PathBuf::from).filter(|p| p.is_absolute()) {
            if let Some(mut x) = from_manifest(browser, profile, id, &p, Some(st)) {
                x.from_store = false;
                out.push(x);
            }
        }
    }
}

fn firefox(profiles: &Path, out: &mut Vec<Extension>) {
    let Ok(rd) = std::fs::read_dir(profiles) else { return };
    for e in rd.flatten() {
        let Some(v) = read_json(&e.path().join("extensions.json")) else { continue };
        let profile = e.file_name().to_string_lossy().split_once('.').map(|(_, n)| n.to_string()).unwrap_or_default();
        for a in v.a("addons") {
            let loc = a.s("location").unwrap_or_default();
            if a.s("type").as_deref() != Some("extension") || loc.starts_with("app-builtin") || loc.starts_with("app-system") {
                continue;
            }
            let mut perms = a.g("userPermissions").strs("permissions");
            perms.extend(a.g("userPermissions").strs("origins"));
            out.push(Extension {
                browser: "Firefox".into(),
                profile: profile.clone(),
                profile_dir: String::new(),
                id: a.s("id").unwrap_or_default(),
                name: a.g("defaultLocale").s("name").unwrap_or_else(|| a.s("id").unwrap_or_default()),
                version: a.s("version").unwrap_or_default(),
                enabled: a.b("active") == Some(true) && a.b("userDisabled") != Some(true),
                from_store: a.s("sourceURI").is_some_and(|u| u.contains("addons.mozilla.org")),
                all_sites: perms.iter().any(|p| ALL_SITES.contains(&p.as_str())),
                permissions: perms,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_browser_opens_its_own_extension_page() {
        let e = |browser: &str| Extension { browser: browser.into(), id: "abc".into(), ..Default::default() };
        assert_eq!(manage_target(&e("Chrome")), Some(("chrome.exe", "chrome://extensions/?id=abc".to_string())));
        assert_eq!(manage_target(&e("Edge")).unwrap().1, "edge://extensions/?id=abc");
        assert_eq!(manage_target(&e("Firefox")).unwrap().1, "about:addons");
        assert_eq!(manage_target(&e("Netscape")), None);
    }

    #[test]
    fn permissions_include_hosts_and_content_scripts() {
        let m = json!({"permissions": ["storage", "tabs"], "host_permissions": ["<all_urls>"], "content_scripts": [{"matches": ["https://*/*"]}]});
        let p = permissions(&m);
        assert!(p.contains(&"tabs".to_string()));
        assert!(p.contains(&"<all_urls>".to_string()));
        assert!(p.contains(&"https://*/*".to_string()));
    }
}
