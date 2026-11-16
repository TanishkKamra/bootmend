//! Reference data the rules compare against. Every entry names its public source.

pub struct BadExtension {
    pub id: Option<&'static str>,
    pub name_contains: &'static str,
    pub why: &'static str,
    pub source: &'static str,
}

/// Extensions with published evidence of harvesting user data.
pub const BAD_EXTENSIONS: &[BadExtension] = &[
    BadExtension {
        id: Some("eppiocemhmnlbhjplcgkofciiegomcon"),
        name_contains: "urban vpn",
        why: "Security researchers found it collecting people's conversations with AI chat services and sending them to its makers.",
        source: "Koi Security research, December 2025 (reported by The Register and CSO Online)",
    },
    BadExtension {
        id: None,
        name_contains: "1clickvpn",
        why: "Researchers found it shares the same data-collecting code as Urban VPN Proxy.",
        source: "Koi Security research, December 2025",
    },
    BadExtension {
        id: None,
        name_contains: "urban browser guard",
        why: "Researchers found it shares the same data-collecting code as Urban VPN Proxy.",
        source: "Koi Security research, December 2025",
    },
    BadExtension {
        id: None,
        name_contains: "urban ad blocker",
        why: "Researchers found it shares the same data-collecting code as Urban VPN Proxy.",
        source: "Koi Security research, December 2025",
    },
];

pub struct VulnerableApp {
    pub name_starts: &'static str,
    pub fixed_in: &'static str,
    pub cve: &'static str,
    pub why: &'static str,
    pub download: &'static str,
}

/// App versions with flaws that attackers have used in real attacks (listed in CISA's KEV catalog).
pub const VULNERABLE_APPS: &[VulnerableApp] = &[
    VulnerableApp {
        name_starts: "WinRAR",
        fixed_in: "7.13",
        cve: "CVE-2025-8088",
        why: "Older versions can be tricked by a booby-trapped archive into placing files anywhere on the PC, including the Startup folder. Attackers have used this in real attacks.",
        download: "https://www.win-rar.com/download.html",
    },
    VulnerableApp {
        name_starts: "7-Zip",
        fixed_in: "24.09",
        cve: "CVE-2025-0411",
        why: "Older versions drop Windows' \"downloaded from the internet\" warning when you open files from inside an archive. Attackers have used this in real attacks.",
        download: "https://www.7-zip.org/download.html",
    },
];

/// Base and boost clocks in GHz, from the makers' spec pages.
pub const CPU_CLOCKS: &[(&str, f64, f64)] = &[
    ("Ryzen 5 3600X", 3.8, 4.4),
    ("Ryzen 5 3600", 3.6, 4.2),
    ("Ryzen 7 3700X", 3.6, 4.4),
    ("Ryzen 7 3800X", 3.9, 4.5),
    ("Ryzen 5 5600X", 3.7, 4.6),
    ("Ryzen 5 5600", 3.5, 4.4),
    ("Ryzen 7 5700X", 3.4, 4.6),
    ("Ryzen 7 5800X3D", 3.4, 4.5),
    ("Ryzen 7 5800X", 3.8, 4.7),
    ("Ryzen 9 5900X", 3.7, 4.8),
    ("Ryzen 5 7600X", 4.7, 5.3),
    ("Ryzen 5 7600", 3.8, 5.1),
    ("Ryzen 7 7800X3D", 4.2, 5.0),
    ("Ryzen 7 9800X3D", 4.7, 5.2),
    ("i5-10400", 2.9, 4.3),
    ("i5-12400", 2.5, 4.4),
    ("i5-13400", 2.5, 4.6),
    ("i5-12600K", 3.7, 4.9),
    ("i5-13600K", 3.5, 5.1),
    ("i5-14600K", 3.5, 5.3),
    ("i7-9700K", 3.6, 4.9),
    ("i7-12700K", 3.6, 5.0),
    ("i7-13700K", 3.4, 5.4),
    ("i7-14700K", 3.4, 5.6),
    ("i9-13900K", 3.0, 5.8),
    ("i9-14900K", 3.2, 6.0),
];

pub fn cpu_clocks(name: &str) -> Option<(f64, f64)> {
    CPU_CLOCKS.iter().find(|(k, _, _)| name.contains(k)).map(|(_, b, t)| (*b, *t))
}

/// Process names of common remote-control apps.
pub const REMOTE_TOOLS: &[(&str, &str)] = &[
    ("anydesk", "AnyDesk"),
    ("teamviewer", "TeamViewer"),
    ("rustdesk", "RustDesk"),
    ("screenconnect", "ScreenConnect"),
    ("connectwisecontrol", "ScreenConnect"),
    ("ultraviewer", "UltraViewer"),
    ("remoting_host", "Chrome Remote Desktop"),
    ("splashtop", "Splashtop"),
    ("logmein", "LogMeIn"),
    ("atera", "Atera"),
];

#[cfg(test)]
mod tests {
    #[test]
    fn x_variants_match_before_base_models() {
        assert_eq!(super::cpu_clocks("AMD Ryzen 5 3600X 6-Core Processor"), Some((3.8, 4.4)));
        assert_eq!(super::cpu_clocks("AMD Ryzen 5 3600 6-Core Processor"), Some((3.6, 4.2)));
        assert_eq!(super::cpu_clocks("Intel(R) Core(TM) i5-12400F"), Some((2.5, 4.4)));
    }
}
