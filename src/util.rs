//! Small shared helpers: lenient JSON access, hidden process runs, app folders.

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
static NULL: Value = Value::Null;

/// Lenient accessors over the JSON that PowerShell emits. PowerShell renders missing
/// values as `null`, `{}` or `""`, and numbers sometimes as strings, so every accessor
/// treats those as "no value" instead of failing.
pub trait J {
    fn g(&self, key: &str) -> &Value;
    fn s(&self, key: &str) -> Option<String>;
    fn f(&self, key: &str) -> Option<f64>;
    fn i(&self, key: &str) -> Option<i64> {
        self.f(key).map(|v| v.round() as i64)
    }
    fn b(&self, key: &str) -> Option<bool>;
    fn a(&self, key: &str) -> Vec<&Value>;
    fn strs(&self, key: &str) -> Vec<String> {
        self.a(key).into_iter().filter_map(as_str).collect()
    }
}

pub fn as_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => {
            let t = s.replace('\u{0}', "").trim().to_string();
            (!t.is_empty()).then_some(t)
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "Yes".into() } else { "No".into() }),
        _ => None,
    }
}

impl J for Value {
    fn g(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&NULL)
    }
    fn s(&self, key: &str) -> Option<String> {
        as_str(self.g(key))
    }
    fn f(&self, key: &str) -> Option<f64> {
        match self.g(key) {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    fn b(&self, key: &str) -> Option<bool> {
        match self.g(key) {
            Value::Bool(b) => Some(*b),
            Value::Number(n) => n.as_i64().map(|x| x != 0),
            Value::String(s) => match s.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Some(true),
                "false" | "0" | "no" | "off" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }
    fn a(&self, key: &str) -> Vec<&Value> {
        match self.g(key) {
            Value::Array(v) => v.iter().filter(|x| !x.is_null()).collect(),
            Value::Null => vec![],
            Value::Object(m) if m.is_empty() => vec![],
            other => vec![other],
        }
    }
}

/// The app's name wherever Windows shows it: the window, its folder, Task Scheduler and notifications.
pub const APP: &str = "Bootmend";
/// The app was called rigscan before; its saved checks lived in a folder by that name.
const OLD_DIR: &str = "rigscan";

/// Moves the old app's folder (saved checks, settings, undo history) under the new name, once.
/// Never touches a folder that already exists under the new name. True if something moved.
pub fn move_old_data(base: &Path) -> Result<bool> {
    let (old, new) = (base.join(OLD_DIR), base.join(APP));
    if new.exists() || !old.is_dir() {
        return Ok(false);
    }
    std::fs::rename(&old, &new).with_context(|| format!("moving {} to {}", old.display(), new.display()))?;
    Ok(true)
}

/// `%LOCALAPPDATA%\Bootmend\<sub>`, created on first use.
pub fn data_dir(sub: &str) -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or_else(|| anyhow!("LOCALAPPDATA is not set"))?;
    let p = PathBuf::from(base).join(APP).join(sub);
    std::fs::create_dir_all(&p).with_context(|| format!("creating {}", p.display()))?;
    Ok(p)
}

pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
    pub timed_out: bool,
}

/// Run a program without flashing a console window, with a hard timeout.
pub fn run_hidden(program: &str, args: &[&str], timeout: Duration) -> Result<Output> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .with_context(|| format!("starting {program}"))?;
    // Drain pipes on threads so a chatty child can't block on a full pipe.
    let mut so = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let mut se = child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?;
    let t_out = std::thread::spawn(move || {
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut so, &mut b).map(|_| b)
    });
    let t_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut se, &mut b).map(|_| b)
    });
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break Some(st);
        }
        if start.elapsed() > timeout {
            timed_out = true;
            if let Err(e) = child.kill() {
                eprintln!("could not stop {program} after timeout: {e}");
            }
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let decode = |r: std::thread::Result<std::io::Result<Vec<u8>>>| -> String {
        match r {
            Ok(Ok(b)) => decode_bytes(&b),
            _ => String::new(),
        }
    };
    Ok(Output { stdout: decode(t_out.join()), stderr: decode(t_err.join()), code: status.and_then(|s| s.code()), timed_out })
}

/// PowerShell and Windows tools mix UTF-8, UTF-16LE and the OEM code page. Handle the first two.
pub fn decode_bytes(b: &[u8]) -> String {
    if b.len() >= 2 && b[0] == 0xFF && b[1] == 0xFE {
        let u: Vec<u16> = b[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&u);
    }
    let s = String::from_utf8_lossy(b);
    s.trim_start_matches('\u{feff}').to_string()
}

/// Run a PowerShell script (passed as text) hidden, returning stdout.
pub fn powershell(script: &str, timeout: Duration) -> Result<Output> {
    let dir = data_dir("tmp")?;
    let file = dir.join(format!("run-{}-{}.ps1", std::process::id(), unique()));
    // UTF-8 with BOM so Windows PowerShell 5.1 reads non-ASCII text correctly.
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(script.as_bytes());
    std::fs::write(&file, bytes).with_context(|| format!("writing {}", file.display()))?;
    let f = file.to_string_lossy().to_string();
    let out = run_hidden("powershell.exe", &["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", &f], timeout);
    if let Err(e) = std::fs::remove_file(&file) {
        eprintln!("could not remove temp script {}: {e}", file.display());
    }
    out
}

fn unique() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    t ^ N.fetch_add(1, Ordering::Relaxed) as u128
}

/// Quote a value for a single-quoted PowerShell string literal.
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Open a URL, settings URI or folder with the shell (no console window).
/// Starts a program by name with arguments. Windows finds installed apps such as chrome.exe on its own.
pub fn shell_run(program: &str, params: &str) -> Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(program), &HSTRING::from(params), None, SW_SHOWNORMAL) };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(anyhow!("Windows could not start {program}"))
    }
}

pub fn shell_open(target: &str) -> Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL) };
    // ShellExecute returns a value > 32 on success.
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(anyhow!("Windows could not open {target}"))
    }
}

pub fn now_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// A saved "2026-11-12 20:15" stamp as people say it: "today at 20:15", "yesterday at …", or "3 Oct 2026 at …".
pub fn when_words(stamp: &str, today: chrono::NaiveDate) -> String {
    let Ok(t) = chrono::NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d %H:%M") else { return stamp.to_string() };
    let day = match (today - t.date()).num_days() {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        _ => t.format("%-d %b %Y").to_string(),
    };
    format!("{day} at {}", t.format("%H:%M"))
}

/// Compare dotted version strings numerically ("7.12" < "7.13", "24.08" < "24.9").
pub fn version_lt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.split(|c: char| !c.is_ascii_digit()).filter(|x| !x.is_empty()).map(|x| x.parse().unwrap_or(0)).collect() };
    let (va, vb) = (parse(a), parse(b));
    for i in 0..va.len().max(vb.len()) {
        let (x, y) = (*va.get(i).unwrap_or(&0), *vb.get(i).unwrap_or(&0));
        if x != y {
            return x < y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn old_saved_checks_move_to_the_new_folder_once() {
        let base = std::env::temp_dir().join(format!("bootmend-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("rigscan").join("snapshots")).unwrap();
        std::fs::write(base.join("rigscan").join("snapshots").join("a.json"), "{}").unwrap();
        assert!(move_old_data(&base).unwrap(), "the first run moves the old folder");
        assert!(base.join(APP).join("snapshots").join("a.json").exists());
        assert!(!base.join("rigscan").exists());
        assert!(!move_old_data(&base).unwrap(), "nothing left to move");
        std::fs::create_dir_all(base.join("rigscan")).unwrap();
        assert!(!move_old_data(&base).unwrap(), "an existing new folder is never overwritten");
        assert!(base.join("rigscan").exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn lenient_accessors_treat_empty_objects_as_missing() {
        let v = json!({"a": {}, "b": "", "c": "12.5", "d": 3, "e": [1, null, 2], "f": "x"});
        assert_eq!(v.s("a"), None);
        assert_eq!(v.s("b"), None);
        assert_eq!(v.f("c"), Some(12.5));
        assert_eq!(v.i("d"), Some(3));
        assert_eq!(v.a("e").len(), 2);
        assert_eq!(v.a("f").len(), 1, "a lone value is a one-item list");
        assert!(v.a("missing").is_empty());
    }

    #[test]
    fn stamps_read_as_today_or_yesterday() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        assert_eq!(when_words("2026-11-12 20:15", today), "today at 20:15");
        assert_eq!(when_words("2026-11-11 08:05", today), "yesterday at 08:05");
        assert_eq!(when_words("2026-10-03 09:00", today), "3 Oct 2026 at 09:00");
        assert_eq!(when_words("not a date", today), "not a date");
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(version_lt("7.12", "7.13"));
        assert!(!version_lt("7.13", "7.13"));
        assert!(version_lt("24.08", "24.09"));
        assert!(!version_lt("25.01", "24.09"));
        assert!(version_lt("6.0.28", "6.0.36"));
    }

    #[test]
    fn ps_quote_escapes_single_quotes() {
        assert_eq!(ps_quote("it's"), "'it''s'");
    }

    #[test]
    fn utf16_output_is_decoded() {
        let b = [0xFF, 0xFE, b'h', 0, b'i', 0];
        assert_eq!(decode_bytes(&b), "hi");
    }
}
