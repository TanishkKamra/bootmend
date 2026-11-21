//! Crash dump analysis with Microsoft's console debugger (cdb), when it is installed.

use crate::snapshot::DumpAnalysis;
use crate::util::{data_dir, run_hidden};
use anyhow::{anyhow, Result};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub fn cdb_path() -> Option<PathBuf> {
    crate::sys::program_files().into_iter().map(|b| b.join(r"Windows Kits\10\Debuggers\x64\cdb.exe")).find(|p| p.is_file())
}

/// Plain-language name and likely cause for common stop codes.
pub fn bugcheck_info(code: u32) -> (&'static str, &'static str) {
    match code {
        0x0 => ("No stop code", "Windows lost power or was forced off, so it could not record a reason."),
        0x0A => ("IRQL_NOT_LESS_OR_EQUAL", "A driver used memory it should not have. Usually a faulty driver, sometimes unstable memory."),
        0x19 => ("BAD_POOL_HEADER", "Windows' memory bookkeeping was damaged, usually by a driver."),
        0x1A => ("MEMORY_MANAGEMENT", "Windows found its memory in a broken state. Often failing or overclocked RAM."),
        0x1E => ("KMODE_EXCEPTION_NOT_HANDLED", "A driver hit an error it could not recover from."),
        0x3B => ("SYSTEM_SERVICE_EXCEPTION", "A driver or system service hit an error. Usually a driver."),
        0x50 => ("PAGE_FAULT_IN_NONPAGED_AREA", "Something read memory that was not there. Usually a driver, sometimes RAM."),
        0x77 => ("KERNEL_STACK_INPAGE_ERROR", "Windows could not read data back from the drive. Check the drive and its cable."),
        0x7A => ("KERNEL_DATA_INPAGE_ERROR", "Windows could not read data back from the drive. Check the drive and its cable."),
        0x7E => ("SYSTEM_THREAD_EXCEPTION_NOT_HANDLED", "A driver hit an error it could not recover from."),
        0x7F => ("UNEXPECTED_KERNEL_MODE_TRAP", "The processor hit a fault. Often hardware, overclocking or overheating."),
        0x9F => ("DRIVER_POWER_STATE_FAILURE", "A driver did not respond while the PC was going to sleep or waking up."),
        0xA0 => ("INTERNAL_POWER_ERROR", "Power management failed, often while sleeping or hibernating."),
        0xC2 => ("BAD_POOL_CALLER", "A driver misused Windows' memory."),
        0xC5 => ("DRIVER_CORRUPTED_EXPOOL", "A driver damaged Windows' memory."),
        0xD1 => ("DRIVER_IRQL_NOT_LESS_OR_EQUAL", "A driver used memory it should not have. Network and graphics drivers are common causes."),
        0xEF => ("CRITICAL_PROCESS_DIED", "A core Windows process stopped. Can be system file damage or a failing drive."),
        0xF4 => ("CRITICAL_OBJECT_TERMINATION", "A core process stopped, often because the drive stopped responding."),
        0xFC => ("ATTEMPTED_EXECUTE_OF_NOEXECUTE_MEMORY", "A driver tried to run code from protected memory."),
        0x101 => ("CLOCK_WATCHDOG_TIMEOUT", "One processor core stopped responding. Often overclocking, voltage or a BIOS issue."),
        0x109 => ("CRITICAL_STRUCTURE_CORRUPTION", "Core Windows data was changed. Often a faulty driver, anti-cheat or RAM."),
        0x116 => ("VIDEO_TDR_FAILURE", "The graphics card stopped responding and could not be reset. Graphics driver or GPU."),
        0x117 => ("VIDEO_TDR_TIMEOUT_DETECTED", "The graphics card stopped responding for a moment."),
        0x124 => ("WHEA_UNCORRECTABLE_ERROR", "The hardware itself reported a fatal error. Usually overclocking, heat, power or a failing part."),
        0x133 => ("DPC_WATCHDOG_VIOLATION", "A driver took too long. Storage and network drivers are common causes."),
        0x139 => ("KERNEL_SECURITY_CHECK_FAILURE", "Windows caught damaged data. Usually a driver, sometimes RAM."),
        0x13A => ("KERNEL_MODE_HEAP_CORRUPTION", "A driver damaged Windows' memory."),
        0x154 => ("UNEXPECTED_STORE_EXCEPTION", "Windows had trouble reading from the drive."),
        0x18B => ("SECURE_KERNEL_ERROR", "The protected part of Windows hit an error. Often a driver or firmware."),
        _ => ("Other stop code", "Look up the stop code on Microsoft's site for details."),
    }
}

fn bare(module: &str) -> String {
    module.to_ascii_lowercase().trim_end_matches(".sys").trim_end_matches(".exe").to_string()
}

/// Windows' own kernel, which crash files name when the real cause left no trace.
pub fn windows_core(module: &str) -> bool {
    matches!(bare(module).as_str(), "nt" | "ntoskrnl" | "ntkrnlmp" | "hal")
}

/// What sort of driver a crash file names, which decides the advice for fixing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Core,
    Graphics,
    Network,
    Storage,
    Defender,
    AntiCheat,
    Usb,
    Audio,
}

/// Kind and plain name for the driver files that most often show up in crash reports.
pub fn driver_info(module: &str) -> Option<(Kind, &'static str)> {
    use Kind::*;
    let m = bare(module);
    Some(match m.as_str() {
        _ if windows_core(&m) => (Core, "Windows core (the real cause is often hardware, RAM or a driver that left no trace)"),
        "nvlddmkm" => (Graphics, "NVIDIA graphics driver"),
        "amdkmdag" | "atikmdag" | "atikmpag" | "amdkmpfd" => (Graphics, "AMD graphics driver"),
        "igdkmd64" | "igdkmdn64" | "igdkmdnd64" => (Graphics, "Intel graphics driver"),
        "dxgkrnl" | "dxgmms2" | "watchdog" => (Graphics, "Windows graphics system (usually the graphics driver underneath)"),
        "tcpip" | "ndis" | "netio" => (Network, "Windows networking (often the network adapter driver)"),
        "storport" | "stornvme" | "storahci" | "iastorac" | "iastora" | "iastorvd" => (Storage, "Storage driver"),
        "wdfilter" | "wdboot" => (Defender, "Microsoft Defender"),
        "bedaisy" | "easyanticheat" | "vgk" | "faceit" => (AntiCheat, "Game anti-cheat driver"),
        "usbxhci" | "ucx01000" | "usbhub3" => (Usb, "USB driver"),
        "hdaudbus" | "portcls" | "ks" => (Audio, "Audio driver"),
        x if x.starts_with("rt") && (x.contains("640") || x.contains("wlan") || x.contains("68")) => (Network, "Realtek network driver"),
        x if x.starts_with("netwtw") || x.starts_with("netwbw") => (Network, "Intel Wi-Fi driver"),
        x if x.starts_with("e1") || x.starts_with("e2f") => (Network, "Intel Ethernet driver"),
        _ => return None,
    })
}

/// Plain name for the driver files that most often show up in crash reports.
pub fn module_hint(module: &str) -> Option<&'static str> {
    driver_info(module).map(|(_, name)| name)
}

/// Pull the useful fields out of `!analyze -v` output.
pub fn parse_analyze(text: &str) -> DumpAnalysis {
    let field = |name: &str| -> Option<String> {
        let re = Regex::new(&format!(r"(?m)^{name}:\s+(\S.*?)\s*$")).ok()?;
        re.captures(text).map(|c| c[1].to_string())
    };
    let code = field("BUGCHECK_CODE")
        .or_else(|| Regex::new(r"(?m)^BugCheck ([0-9A-Fa-f]+),").ok()?.captures(text).map(|c| c[1].to_string()))
        .and_then(|c| u32::from_str_radix(c.trim_start_matches("0x"), 16).ok());
    let probably = Regex::new(r"Probably caused by\s*:\s*(\S+)").ok().and_then(|r| r.captures(text)).map(|c| c[1].to_string());
    let (name, _) = bugcheck_info(code.unwrap_or(u32::MAX));
    let module = field("MODULE_NAME");
    let image = field("IMAGE_NAME").or(probably);
    let note = image.as_deref().and_then(module_hint).or_else(|| module.as_deref().and_then(module_hint)).map(String::from);
    DumpAnalysis {
        code: code.map(|c| format!("0x{c:X}")).unwrap_or_default(),
        name: name.into(),
        module,
        image,
        process: field("PROCESS_NAME"),
        bucket: field("FAILURE_BUCKET_ID"),
        note,
        ..Default::default()
    }
}

/// Analyze one dump. The first run downloads matching debug symbols from Microsoft.
pub fn analyze(cdb: &Path, dump: &Path) -> Result<DumpAnalysis> {
    log!("dumps", "analysing crash dump {}", dump.display());
    let t0 = Instant::now();
    let sym = format!("srv*{}*https://msdl.microsoft.com/download/symbols", data_dir("symbols")?.display());
    let d = dump.to_string_lossy().to_string();
    let out = run_hidden(&cdb.to_string_lossy(), &["-z", &d, "-y", &sym, "-c", "!analyze -v; q"], Duration::from_secs(600))
        .inspect_err(|e| log!("dumps", "could not run the debugger on {}: {e:#}", dump.display()))?;
    if out.timed_out {
        log!("dumps", "the debugger took too long on {}", dump.display());
        return Err(anyhow!("the debugger took too long on {}", dump.display()));
    }
    let mut a = parse_analyze(&out.stdout);
    if a.code.is_empty() {
        log!("dumps", "the debugger could not read {}", dump.display());
        return Err(anyhow!("the debugger could not read {}", dump.display()));
    }
    a.file = d;
    a.date = std::fs::metadata(dump)
        .and_then(|m| m.modified())
        .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    log!("dumps", "analysed {} in {:.0}s: {} {}", dump.display(), t0.elapsed().as_secs_f32(), a.code, a.name);
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "*******\nBUGCHECK_CODE:  d1\n\nBUGCHECK_P1: 0\n\nPROCESS_NAME:  System\n\nMODULE_NAME: nvlddmkm\n\nIMAGE_NAME:  nvlddmkm.sys\n\nFAILURE_BUCKET_ID:  AV_nvlddmkm!unknown_function\n";

    #[test]
    fn analyze_output_is_parsed() {
        let a = parse_analyze(SAMPLE);
        assert_eq!(a.code, "0xD1");
        assert_eq!(a.name, "DRIVER_IRQL_NOT_LESS_OR_EQUAL");
        assert_eq!(a.module.as_deref(), Some("nvlddmkm"));
        assert_eq!(a.image.as_deref(), Some("nvlddmkm.sys"));
        assert_eq!(a.process.as_deref(), Some("System"));
        assert_eq!(a.note.as_deref(), Some("NVIDIA graphics driver"));
        let k = parse_analyze("BUGCHECK_CODE:  50\n\nMODULE_NAME: nt\n\nIMAGE_NAME:  ntkrnlmp.exe\n");
        assert!(k.note.is_some_and(|n| n.starts_with("Windows core")), "kernel crashes need a note too");
    }

    #[test]
    fn old_style_output_is_parsed() {
        let a = parse_analyze("BugCheck 124, {0, ffff, 0, 0}\nProbably caused by : AuthenticAMD\n");
        assert_eq!(a.code, "0x124");
        assert_eq!(a.image.as_deref(), Some("AuthenticAMD"));
    }

    /// Runs the real debugger on a crash file named by BOOTMEND_TEST_DUMP. Downloads symbols from Microsoft.
    #[test]
    #[ignore = "needs a crash file, the Windows debugger and the internet"]
    fn analyzes_a_real_crash_file() {
        let dump = std::env::var("BOOTMEND_TEST_DUMP").expect("set BOOTMEND_TEST_DUMP to a .dmp file");
        let cdb = cdb_path().expect("Debugging Tools for Windows not installed");
        let a = analyze(&cdb, Path::new(&dump)).unwrap();
        println!("{a:#?}");
        assert!(a.code.starts_with("0x"), "{a:?}");
    }
}
