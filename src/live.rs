//! What's using the processor, memory and drive right now, grouped by app. Sampled every two seconds
//! while the page is open, and stopped when it closes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct AppUse {
    pub name: String,
    /// Share of the whole processor, 0 to 100.
    pub cpu: f32,
    pub mem: u64,
    /// Bytes per second read from and written to drives.
    pub disk: f64,
    pub processes: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Live {
    pub cpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub apps: Vec<AppUse>,
}

/// One process as sampled. `group` is the key apps are grouped by.
pub struct Proc {
    pub group: String,
    pub name: String,
    pub cpu_cores: f32,
    pub mem: u64,
    pub disk: f64,
}

/// Adds up processes that belong to the same app (a browser runs dozens). `cores` turns per-core
/// percentages into a share of the whole processor.
pub fn group(procs: Vec<Proc>, cores: usize) -> Vec<AppUse> {
    let mut by: HashMap<String, AppUse> = HashMap::new();
    for p in procs {
        let e = by.entry(p.group).or_insert_with(|| AppUse { name: p.name, cpu: 0.0, mem: 0, disk: 0.0, processes: 0 });
        e.cpu += p.cpu_cores / cores.max(1) as f32;
        e.mem += p.mem;
        e.disk += p.disk;
        e.processes += 1;
    }
    let mut v: Vec<AppUse> = by.into_values().collect();
    v.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.mem.cmp(&a.mem)));
    v
}

/// One plain sentence on what, if anything, is slowing the PC down.
pub fn verdict(l: &Live) -> String {
    let top = |key: fn(&AppUse) -> f64| l.apps.iter().max_by(|a, b| key(a).total_cmp(&key(b)));
    if l.cpu > 85.0 {
        if let Some(a) = top(|a| a.cpu as f64) {
            return format!("The processor is busy. {} is using the most of it.", a.name);
        }
    }
    if l.mem_total > 0 && l.mem_used as f64 / l.mem_total as f64 > 0.85 {
        if let Some(a) = top(|a| a.mem as f64) {
            return format!("Memory is nearly full, so Windows is moving things to the drive. Closing {} would free the most.", a.name);
        }
    }
    if let Some(a) = top(|a| a.disk).filter(|a| a.disk > 20e6) {
        return format!("{} is reading or writing a lot on the drive, which can make other apps wait.", a.name);
    }
    "Nothing is holding the PC back right now.".into()
}

/// The "File description" Windows shows in Task Manager, such as "Google Chrome".
pub fn file_description(path: &Path) -> Option<String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let p = HSTRING::from(path.as_os_str());
    let size = unsafe { GetFileVersionInfoSizeW(&p, None) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(&p, None, size, buf.as_mut_ptr() as _) }.ok()?;
    let query = |sub: &str| -> Option<(*const u8, u32)> {
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let ok = unsafe { VerQueryValueW(buf.as_ptr() as _, PCWSTR(HSTRING::from(sub).as_ptr()), &mut ptr, &mut len) }.as_bool();
        (ok && !ptr.is_null() && len > 0).then_some((ptr as *const u8, len))
    };
    let (t, _) = query(r"\VarFileInfo\Translation")?;
    let (lang, cp) = unsafe { (*(t as *const u16), *(t as *const u16).add(1)) };
    let (s, len) = query(&format!(r"\StringFileInfo\{lang:04x}{cp:04x}\FileDescription"))?;
    let chars = unsafe { std::slice::from_raw_parts(s as *const u16, len as usize) };
    let text = String::from_utf16_lossy(chars).trim_end_matches('\0').trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Samples in the background until dropped.
pub struct Monitor {
    pub data: Arc<Mutex<Option<Live>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        log!("live", "live monitor stopped");
    }
}

impl Monitor {
    pub fn start(on_sample: impl Fn() + Send + 'static) -> Monitor {
        let data = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        log!("live", "live monitor started");
        let (d, s) = (data.clone(), stop.clone());
        std::thread::spawn(move || sample_loop(d, s, on_sample));
        Monitor { data, stop }
    }
}

fn sample_loop(data: Arc<Mutex<Option<Live>>>, stop: Arc<AtomicBool>, on_sample: impl Fn()) {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let kind = ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage().with_exe(UpdateKind::OnlyIfNotSet);
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    sys.refresh_cpu_usage();
    let mut names: HashMap<PathBuf, String> = HashMap::new();
    let mut last = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        // Short sleeps so closing the page stops sampling quickly.
        for _ in 0..20 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let secs = last.elapsed().as_secs_f64().max(0.1);
        last = Instant::now();
        let procs: Vec<Proc> = sys
            .processes()
            .iter()
            .filter(|(pid, _)| pid.as_u32() != 0)
            .map(|(_, p)| {
                let raw = p.name().to_string_lossy().trim_end_matches(".exe").to_string();
                let name = match p.exe() {
                    Some(exe) if !exe.as_os_str().is_empty() => {
                        names.entry(exe.to_path_buf()).or_insert_with(|| file_description(exe).unwrap_or_else(|| raw.clone())).clone()
                    }
                    _ => raw.clone(),
                };
                let d = p.disk_usage();
                Proc { group: raw.to_lowercase(), name, cpu_cores: p.cpu_usage(), mem: p.memory(), disk: (d.read_bytes + d.written_bytes) as f64 / secs }
            })
            .collect();
        let live = Live { cpu: sys.global_cpu_usage(), mem_used: sys.used_memory(), mem_total: sys.total_memory(), apps: group(procs, sys.cpus().len()) };
        if let Ok(mut g) = data.lock() {
            *g = Some(live);
        }
        on_sample();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(group: &str, cpu: f32, mem: u64) -> Proc {
        Proc { group: group.into(), name: group.to_uppercase(), cpu_cores: cpu, mem, disk: 0.0 }
    }

    #[test]
    fn processes_of_one_app_are_added_up() {
        let g = group(vec![p("chrome", 40.0, 100), p("chrome", 40.0, 200), p("code", 20.0, 50)], 4);
        assert_eq!(g[0].name, "CHROME");
        assert_eq!((g[0].cpu, g[0].mem, g[0].processes), (20.0, 300, 2));
        assert_eq!(g[1].cpu, 5.0);
    }

    #[test]
    fn verdict_names_the_cause() {
        let apps = group(vec![p("game", 300.0, 2_000), p("chrome", 10.0, 9_000)], 4);
        let busy = Live { cpu: 95.0, mem_used: 1, mem_total: 10, apps: apps.clone() };
        assert!(verdict(&busy).contains("GAME"), "{}", verdict(&busy));
        let full = Live { cpu: 20.0, mem_used: 95, mem_total: 100, apps: apps.clone() };
        assert!(verdict(&full).contains("CHROME"), "{}", verdict(&full));
        let calm = Live { cpu: 10.0, mem_used: 40, mem_total: 100, apps };
        assert!(verdict(&calm).starts_with("Nothing"));
    }

    #[test]
    fn reads_task_manager_names() {
        let notepad = PathBuf::from(std::env::var("SystemRoot").unwrap()).join(r"System32\notepad.exe");
        let d = file_description(&notepad);
        assert!(d.as_deref().is_some_and(|d| d.to_lowercase().contains("notepad")), "{d:?}");
    }
}
