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
    let mut cmd = Command::new(crate::sys::program(program));
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).creation_flags(CREATE_NO_WINDOW);
    crate::sys::prepare(&mut cmd)?;
    let mut child = cmd.spawn().inspect_err(|e| log!("run", "couldn't start {program}: {e}")).with_context(|| format!("starting {program}"))?;
    let job = Job::hold(&child, program);
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
            if !job.as_ref().is_some_and(|j| j.stop(program)) {
                if let Err(e) = child.kill() {
                    log!("run", "couldn't stop {program} after it took too long: {e}");
                }
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
    let out = Output { stdout: decode(t_out.join()), stderr: decode(t_err.join()), code: status.and_then(|s| s.code()), timed_out };
    let took = start.elapsed().as_secs_f32();
    let cmd = format!("{program} {}", args.join(" "));
    if timed_out {
        log!("run", "{cmd} was stopped after {took:.1} s for taking too long");
    } else if out.code == Some(0) {
        log!("run", "{cmd} finished in {took:.1} s");
    } else {
        let err = out.stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("no error text");
        log!("run", "{cmd} ended with code {:?} after {took:.1} s: {err}", out.code);
    }
    Ok(out)
}

/// A Windows job holding a program and everything it starts. Stopping only the program would
/// leave, say, DISM running under a stopped repair script, still holding the output pipe open,
/// so the timeout would wait for DISM after all.
struct Job(windows::Win32::Foundation::HANDLE);

impl Job {
    fn hold(child: &std::process::Child, program: &str) -> Option<Job> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};
        // SAFETY: a new unnamed job, and the child's handle stays open while `child` is alive.
        let job = match unsafe { CreateJobObjectW(None, windows::core::PCWSTR::null()) } {
            Ok(h) => Job(h),
            Err(e) => {
                log!("run", "couldn't make a job for {program}, so a timeout stops only {program}: {e}");
                return None;
            }
        };
        if let Err(e) = unsafe { AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())) } {
            log!("run", "couldn't put {program} in a job, so a timeout stops only {program}: {e}");
            return None;
        }
        Some(job)
    }

    fn stop(&self, program: &str) -> bool {
        // SAFETY: the handle is a job this struct owns.
        match unsafe { windows::Win32::System::JobObjects::TerminateJobObject(self.0, 1) } {
            Ok(()) => true,
            Err(e) => {
                log!("run", "couldn't stop what {program} started: {e}");
                false
            }
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle is a job this struct owns, closed once here.
        if let Err(e) = unsafe { windows::Win32::Foundation::CloseHandle(self.0) } {
            log!("run", "couldn't close a job handle: {e}");
        }
    }
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
        log!("run", "couldn't remove the temporary script {}: {e}", file.display());
    }
    out
}

pub(crate) fn unique() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    t ^ N.fetch_add(1, Ordering::Relaxed) as u128
}

/// Quote a value for a single-quoted PowerShell string literal. PowerShell also ends these strings
/// at the curly single quotes, so those are doubled too, as its own escaping does.
pub fn ps_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
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
    #[ignore = "starts PowerShell; run with --ignored"]
    fn a_timeout_stops_what_the_program_started_too() {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
        let file = std::env::temp_dir().join(format!("bootmend-tree-{}.txt", std::process::id()));
        // The way DISM runs under a repair script: PowerShell waits on a program it started.
        let script = format!(
            "$p = Start-Process ping.exe -ArgumentList '-n','30','127.0.0.1' -NoNewWindow -PassThru; Set-Content -LiteralPath {} -Value $p.Id; Start-Sleep 30",
            ps_quote(&file.to_string_lossy())
        );
        let t0 = Instant::now();
        let out = run_hidden("powershell.exe", &["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &script], Duration::from_secs(4)).unwrap();
        let took = t0.elapsed();
        assert!(out.timed_out);
        assert!(took < Duration::from_secs(15), "returned after {took:?}, not soon after the timeout");
        let pid: u32 = std::fs::read_to_string(&file).unwrap().trim().parse().unwrap();
        std::fs::remove_file(&file).unwrap();
        let mut sys = System::new();
        let pid = sysinfo::Pid::from_u32(pid);
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing());
        assert!(sys.process(pid).is_none(), "the program PowerShell started is still running");
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
    fn ps_quote_doubles_every_quote_powershell_ends_a_string_at() {
        for q in ['\'', '\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}'] {
            assert_eq!(ps_quote(&format!("a{q}b")), format!("'a{q}{q}b'"));
        }
    }

    #[test]
    #[ignore = "starts PowerShell; run with --ignored"]
    fn quoted_values_stay_text_in_powershell() {
        for s in ["it's", "O\u{2019}Neil", "x\u{2019}+$(Write-Output pwned)+\u{2019}", "\u{2018}a\u{201A}b\u{201B}", "$env:USERNAME `n ;& |", "line1\nline2"] {
            // Compared as code points, so the console's code page can't blur the result.
            let out = powershell(&format!("$s = {}\n[string]::Join(',', [int[]][char[]]$s)", ps_quote(s)), Duration::from_secs(60)).unwrap();
            let want: Vec<String> = s.encode_utf16().map(|u| u.to_string()).collect();
            assert_eq!(out.stdout.trim(), want.join(","), "{s:?}: {}", out.stderr);
        }
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
