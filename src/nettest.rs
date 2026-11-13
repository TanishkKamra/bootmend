//! Connection check, run only when the person asks: how quickly the router and the DNS server answer,
//! and the internet's response time and speed measured against Cloudflare's public speed test.

use crate::util::CREATE_NO_WINDOW;
use anyhow::{anyhow, Context, Result};
use std::io::Write;
use std::net::Ipv4Addr;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SPEED: &str = "https://speed.cloudflare.com";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetCheck {
    /// Round trip to the router, in milliseconds, and how many of the pings got no answer.
    pub router_ms: Option<f64>,
    pub router_lost: u32,
    pub router_sent: u32,
    pub dns_ms: Option<f64>,
    pub internet_ms: Option<f64>,
    pub down_mbps: Option<f64>,
    pub up_mbps: Option<f64>,
    /// Parts that didn't work, in plain words.
    pub problems: Vec<String>,
}

/// Median of the values; `None` when there are none.
pub fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

/// Reads curl's `-w "%{http_code} %{time_connect} %{time_total} %{speed_download} %{speed_upload}"` line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transfer {
    pub code: u16,
    pub connect_s: f64,
    pub total_s: f64,
    pub down_bps: f64,
    pub up_bps: f64,
}

pub fn parse_transfer(out: &str) -> Option<Transfer> {
    let f: Vec<&str> = out.split_whitespace().collect();
    if f.len() < 5 {
        return None;
    }
    // Some curl builds print decimals with a comma under non-English settings.
    let n = |s: &str| s.replace(',', ".").parse::<f64>().ok();
    Some(Transfer { code: f[0].parse().ok()?, connect_s: n(f[1])?, total_s: n(f[2])?, down_bps: n(f[3])?, up_bps: n(f[4])? })
}

fn curl() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    format!(r"{root}\System32\curl.exe")
}

/// One request with Windows' own curl. `upload` bytes are sent as the request body when given.
fn transfer(url: &str, upload: Option<usize>, timeout: Duration) -> Result<Transfer> {
    let fmt = "%{http_code} %{time_connect} %{time_total} %{speed_download} %{speed_upload}";
    let secs = timeout.as_secs().to_string();
    let mut args = vec!["-s", "-o", "NUL", "--max-time", &secs, "-w", fmt];
    if upload.is_some() {
        args.extend(["--data-binary", "@-", "-H", "Content-Type: application/octet-stream"]);
    }
    args.push(url);
    let mut child = Command::new(curl())
        .args(&args)
        .stdin(if upload.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("Windows' curl tool couldn't start")?;
    if let (Some(n), Some(mut stdin)) = (upload, child.stdin.take()) {
        stdin.write_all(&vec![0u8; n]).context("sending test data")?;
    }
    let out = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let t = parse_transfer(&text).ok_or_else(|| anyhow!("no answer from Cloudflare"))?;
    if t.code == 0 {
        return Err(anyhow!("couldn't reach Cloudflare; is the PC online?"));
    }
    if t.code >= 400 {
        return Err(anyhow!("Cloudflare answered with error {}", t.code));
    }
    Ok(t)
}

/// Pings the router `count` times. Returns each round trip in ms; lost pings are left out.
fn ping(addr: Ipv4Addr, count: u32) -> Result<Vec<f64>> {
    use windows::Win32::NetworkManagement::IpHelper::{IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY};
    let h = unsafe { IcmpCreateFile() }.context("Windows couldn't open a ping")?;
    let data = [0x61u8; 32];
    let mut times = vec![];
    for _ in 0..count {
        let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + data.len() + 8];
        let dest = u32::from_ne_bytes(addr.octets());
        let n = unsafe { IcmpSendEcho(h, dest, data.as_ptr() as _, data.len() as u16, None, reply.as_mut_ptr() as _, reply.len() as u32, 1000) };
        if n > 0 {
            let r = unsafe { &*(reply.as_ptr() as *const ICMP_ECHO_REPLY) };
            if r.Status == 0 {
                // Windows reports 0 ms for anything under a millisecond.
                times.push(r.RoundTripTime.max(1) as f64);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    unsafe {
        let _ = IcmpCloseHandle(h);
    }
    Ok(times)
}

/// Time for the DNS server to look up a name, skipping Windows' own cache.
fn dns_lookup(name: &str) -> Result<f64> {
    use windows::core::HSTRING;
    use windows::Win32::NetworkManagement::Dns::{DnsFree, DnsFreeRecordList, DnsQuery_W, DNS_QUERY_BYPASS_CACHE, DNS_RECORDA, DNS_TYPE_A};
    let mut rec: *mut DNS_RECORDA = std::ptr::null_mut();
    let t = Instant::now();
    let r = unsafe { DnsQuery_W(&HSTRING::from(name), DNS_TYPE_A, DNS_QUERY_BYPASS_CACHE, None, &mut rec, None) };
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    if !rec.is_null() {
        unsafe { DnsFree(Some(rec as _), DnsFreeRecordList) };
    }
    if r.0 != 0 {
        return Err(anyhow!("the DNS server didn't answer (code {})", r.0));
    }
    Ok(ms)
}

/// Runs the whole check. `router` is the default gateway, when Windows reported one.
pub fn run(router: Option<Ipv4Addr>) -> NetCheck {
    let mut c = NetCheck::default();
    if let Some(r) = router {
        c.router_sent = 10;
        match ping(r, c.router_sent) {
            Ok(t) => {
                c.router_lost = c.router_sent - t.len() as u32;
                c.router_ms = median(t);
                if c.router_ms.is_none() {
                    c.problems.push("The router didn't answer pings. Some routers are set to ignore them, so this alone isn't a problem.".into());
                }
            }
            Err(e) => c.problems.push(format!("{e:#}")),
        }
    }
    let lookups: Vec<f64> = ["www.microsoft.com", "www.wikipedia.org", "www.cloudflare.com"].iter().filter_map(|n| dns_lookup(n).ok()).collect();
    c.dns_ms = median(lookups);
    if c.dns_ms.is_none() {
        c.problems.push("The DNS server, which turns website names into addresses, didn't answer.".into());
    }

    let mut connects = vec![];
    for _ in 0..5 {
        match transfer(&format!("{SPEED}/__down?bytes=0"), None, Duration::from_secs(10)) {
            Ok(t) => connects.push(t.connect_s * 1000.0),
            Err(e) => {
                c.problems.push(format!("Response time: {e:#}."));
                return c;
            }
        }
    }
    c.internet_ms = median(connects);

    // Small first; a fast line finishes 25 MB too quickly to measure well, so it gets a bigger file.
    match transfer(&format!("{SPEED}/__down?bytes=25000000"), None, Duration::from_secs(60)) {
        Ok(t) if t.total_s < 2.0 => match transfer(&format!("{SPEED}/__down?bytes=100000000"), None, Duration::from_secs(60)) {
            Ok(big) => c.down_mbps = Some(big.down_bps * 8.0 / 1e6),
            Err(_) => c.down_mbps = Some(t.down_bps * 8.0 / 1e6),
        },
        Ok(t) => c.down_mbps = Some(t.down_bps * 8.0 / 1e6),
        Err(e) => c.problems.push(format!("Download speed: {e:#}.")),
    }
    match transfer(&format!("{SPEED}/__up"), Some(10_000_000), Duration::from_secs(60)) {
        Ok(t) => c.up_mbps = Some(t.up_bps * 8.0 / 1e6),
        Err(e) => c.problems.push(format!("Upload speed: {e:#}.")),
    }
    c
}

/// What a download speed is good for, in plain words.
pub fn speed_words(mbps: f64) -> &'static str {
    match mbps {
        x if x < 5.0 => "Slow. Video may pause to load, and big downloads take a long time.",
        x if x < 25.0 => "Fine for HD video and video calls, one at a time.",
        x if x < 100.0 => "Good for 4K video and a few people online at once.",
        _ => "Fast. Plenty for a busy household.",
    }
}

/// What a response time means for calls and games.
pub fn latency_words(ms: f64) -> &'static str {
    match ms {
        x if x < 30.0 => "Quick. Calls and online games feel immediate.",
        x if x < 80.0 => "Fine for calls and most games.",
        _ => "Slow to respond. Calls may talk over each other and games may lag.",
    }
}

/// Where the slowness is, in plain words, when something stands out.
pub fn diagnosis(c: &NetCheck) -> Option<&'static str> {
    if c.router_sent > 0 && c.router_lost * 10 > c.router_sent * 2 {
        return Some("Some pings to the router got lost. On Wi-Fi this usually means a weak signal or interference from nearby networks.");
    }
    if c.router_ms.is_some_and(|ms| ms > 20.0) {
        return Some("The delay starts between this PC and the router. Moving closer to it, or using a cable, would help most.");
    }
    if c.dns_ms.is_some_and(|ms| ms > 150.0) {
        return Some("Looking up website names is slow, so pages take a moment before they start loading.");
    }
    if c.internet_ms.is_some_and(|ms| ms > 80.0) {
        return Some("Your home network answers quickly; the delay is further out, with the internet provider.");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnosis_points_at_the_slow_part() {
        let ok = NetCheck { router_ms: Some(3.0), router_sent: 10, dns_ms: Some(20.0), internet_ms: Some(25.0), ..Default::default() };
        assert_eq!(diagnosis(&ok), None);
        assert!(diagnosis(&NetCheck { router_lost: 4, ..ok.clone() }).unwrap().contains("lost"));
        assert!(diagnosis(&NetCheck { router_ms: Some(45.0), ..ok.clone() }).unwrap().contains("router"));
        assert!(diagnosis(&NetCheck { internet_ms: Some(140.0), ..ok.clone() }).unwrap().contains("provider"));
    }

    #[test]
    fn median_handles_odd_even_and_empty() {
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(vec![4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(vec![]), None);
    }

    #[test]
    fn curl_output_is_read_in_any_locale() {
        let t = parse_transfer("200 0.012345 1.500000 16666666 0").unwrap();
        assert_eq!((t.code, t.down_bps), (200, 16666666.0));
        assert!((t.connect_s - 0.012345).abs() < 1e-9);
        let t = parse_transfer("200 0,020000 2,000000 1000,5 250,25").unwrap();
        assert_eq!((t.connect_s, t.up_bps), (0.02, 250.25));
        assert_eq!(parse_transfer("000"), None);
        assert_eq!(parse_transfer("").map(|t| t.code), None);
    }

    #[test]
    fn plain_words_for_speeds() {
        assert!(speed_words(3.0).starts_with("Slow"));
        assert!(speed_words(300.0).starts_with("Fast"));
        assert!(latency_words(120.0).contains("lag"));
    }

    /// Runs the real check against Cloudflare. Uses about 135 MB of data.
    #[test]
    #[ignore = "goes online"]
    fn checks_this_connection() {
        let gw = std::env::var("BOOTMEND_GATEWAY").ok().and_then(|g| g.parse().ok());
        let t = Instant::now();
        println!("{:#?} in {:?}", run(gw), t.elapsed());
    }
}
