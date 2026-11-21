//! Connection check, run only when the person asks: how quickly the router, the DNS server and a
//! website answer, so a delay can be placed in the home network or further out, then how fast
//! the connection is, measured by M-Lab (see `ndt7`).

use crate::ndt7::{self, Phase, Speed};
use anyhow::{anyhow, Context, Result};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

/// A site every Windows PC already talks to.
const SITE: &str = "www.microsoft.com";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetCheck {
    /// Round trip to the router, in milliseconds, and how many of the pings got no answer.
    pub router_ms: Option<f64>,
    pub router_lost: u32,
    pub router_sent: u32,
    pub dns_ms: Option<f64>,
    pub internet_ms: Option<f64>,
    /// Tries to reach the website, and how many got no answer.
    pub internet_sent: u32,
    pub internet_lost: u32,
    pub jitter_ms: Option<f64>,
    pub wifi: Option<Wifi>,
    pub speed: Option<Speed>,
    /// Parts that didn't work, in plain words.
    pub problems: Vec<String>,
}

impl NetCheck {
    /// Pings and tries that got no answer, of those sent. A router that never answers is left
    /// out: some are set to ignore pings.
    pub fn packets(&self) -> (u32, u32) {
        let router = if self.router_ms.is_some() { (self.router_lost, self.router_sent) } else { (0, 0) };
        (router.0 + self.internet_lost, router.1 + self.internet_sent)
    }
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

/// Time to open a connection to `host`, after looking up its address. Nothing is sent: asking a
/// big site for even a page's headers sometimes stalls for seconds, which would read as a dropout.
fn connect_ms(host: &str, timeout: Duration) -> Result<f64> {
    use std::net::{TcpStream, ToSocketAddrs};
    let addr = (host, 443).to_socket_addrs().with_context(|| format!("couldn't look up {host}"))?.next().ok_or_else(|| anyhow!("{host} has no address"))?;
    let t = Instant::now();
    TcpStream::connect_timeout(&addr, timeout).with_context(|| format!("couldn't reach {host}; is the PC online?"))?;
    Ok(t.elapsed().as_secs_f64() * 1000.0)
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

/// The connected Wi-Fi, read from Windows as the test starts.
fn wifi() -> Option<Wifi> {
    let out = crate::util::run_hidden("netsh.exe", &["wlan", "show", "interfaces"], Duration::from_secs(5))
        .inspect_err(|e| log!("net", "couldn't read the Wi-Fi: {e:#}"))
        .ok()?;
    parse_wlan(&out.stdout)
}

/// Runs the whole check. `router` is the default gateway, when Windows reported one. `show`
/// hears which half of the speed test is running (none before it starts) and its speed so far.
pub fn run(router: Option<Ipv4Addr>, show: &dyn Fn(Option<Phase>, Option<f64>)) -> NetCheck {
    log!("net", "connection check started");
    let t0 = Instant::now();
    let c = check(router, show);
    let ms = |v: Option<f64>| v.map_or("n/a".to_string(), |v| format!("{v:.0} ms"));
    log!(
        "net",
        "connection check finished in {:.0}s: router {} ({} of {} pings lost), dns {}, internet {}",
        t0.elapsed().as_secs_f32(),
        ms(c.router_ms),
        c.router_lost,
        c.router_sent,
        ms(c.dns_ms),
        ms(c.internet_ms)
    );
    if let Some(s) = &c.speed {
        let mbps = |v: Option<f64>| v.map_or("n/a".to_string(), speed_text);
        log!("net", "speed: {} down, {} up, round trip {} quiet and {} busy", mbps(s.down_mbps), mbps(s.up_mbps), ms(s.idle_ms), ms(s.loaded_ms));
    }
    for p in &c.problems {
        log!("net", "problem: {p}");
    }
    c
}

fn check(router: Option<Ipv4Addr>, show: &dyn Fn(Option<Phase>, Option<f64>)) -> NetCheck {
    let mut c = NetCheck::default();
    show(None, None);
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

    // Read before the website is tried, so a dead connection still shows its Wi-Fi.
    c.wifi = wifi();
    // Enough tries to tell how steady the round trips are.
    const TRIES: usize = 10;
    let (connects, failed, err) = attempts(TRIES, || connect_ms(SITE, Duration::from_secs(10)));
    c.internet_sent = (connects.len() + failed) as u32;
    c.internet_lost = failed as u32;
    match err {
        Some(e) if connects.is_empty() => {
            c.problems.push(format!("Response time: {e:#}."));
            return c;
        }
        Some(_) => c.problems.push(format!("{failed} of {TRIES} tries got no answer, so the connection may be dropping out.")),
        None => {}
    }
    c.jitter_ms = jitter(&connects);
    c.internet_ms = median(connects);
    match ndt7::run(&|phase, mbps| show(Some(phase), mbps)) {
        Ok(s) => {
            if s.up_mbps.is_none() {
                c.problems.push("The upload half of the speed test couldn't finish.".into());
            }
            c.speed = Some(s);
        }
        Err(e) => c.problems.push(format!("Speed test: {e:#}.")),
    }
    c
}

/// Runs `once` the given number of times. A try that gets no answer doesn't stop the rest, since a
/// patchy connection is often why someone runs the test, but two misses before any answer do, so a
/// dead connection isn't waited on every time. Returns the answers, how many failed, and the last error.
fn attempts(tries: usize, mut once: impl FnMut() -> Result<f64>) -> (Vec<f64>, usize, Option<anyhow::Error>) {
    let (mut ok, mut failed, mut last) = (vec![], 0, None);
    for _ in 0..tries {
        match once() {
            Ok(v) => ok.push(v),
            Err(e) => {
                failed += 1;
                last = Some(e);
                if ok.is_empty() && failed == 2 {
                    break;
                }
            }
        }
    }
    (ok, failed, last)
}

/// What a response time means for calls and games.
pub fn latency_words(ms: f64) -> &'static str {
    match ms {
        x if x < 30.0 => "Quick. Calls and online games feel immediate.",
        x if x < 80.0 => "Fine for calls and most games.",
        _ => "Slow to respond. Calls may talk over each other and games may lag.",
    }
}

/// How much the round trips wobble: the average change from one to the next, in ms.
pub fn jitter(samples: &[f64]) -> Option<f64> {
    (samples.len() >= 2).then(|| samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (samples.len() - 1) as f64)
}

/// A speed such as "4.2 Mbps" or "245 Mbps".
pub fn speed_text(mbps: f64) -> String {
    // 9.96 would show as "10.0" with one decimal.
    if mbps < 9.95 {
        format!("{mbps:.1} Mbps")
    } else {
        format!("{mbps:.0} Mbps")
    }
}

/// What a connection this fast is good for, in a line.
pub fn good_for(down: f64, up: Option<f64>) -> String {
    let line = match down {
        d if d < 3.0 => "Enough for email and web pages, but video will stall.",
        d if d < 10.0 => "Good for web pages and HD video on one screen at a time.",
        d if d < 50.0 => "Good for HD video and video calls, a couple at once.",
        d if d < 200.0 => "Good for 4K video, video calls and downloads at the same time.",
        _ => "Plenty for a busy home: 4K video, video calls and big downloads at the same time.",
    };
    // A video call sends about as much as it receives.
    if up.is_some_and(|u| u < 2.0) {
        format!("{line} Uploads are slow, so others may see your video freeze in calls.")
    } else {
        line.to_string()
    }
}

/// How the extra delay feels while something fills the line.
pub fn lag_words(extra_ms: f64) -> &'static str {
    match extra_ms {
        x if x < 20.0 => "Barely changes. Good for games and calls",
        x if x < 60.0 => "Fine for calls",
        x if x < 150.0 => "Calls may stutter while something downloads",
        _ => "Calls and games stutter while something downloads",
    }
}

/// Jitter and lost packets together, in plain words.
pub fn steadiness(jitter_ms: Option<f64>, lost: u32, sent: u32) -> Option<String> {
    let mut parts = vec![];
    if let Some(j) = jitter_ms {
        parts.push(
            match j {
                x if x < 10.0 => "steady",
                x if x < 30.0 => "a little uneven",
                _ => "uneven",
            }
            .to_string(),
        );
    }
    if sent > 0 {
        parts.push(if lost == 0 { "no packets lost".to_string() } else { format!("{lost} of {sent} packets lost") });
    }
    let s = parts.join(", ");
    let mut chars = s.chars();
    chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
}

/// The Wi-Fi connection as Windows reports it while the test runs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wifi {
    /// Signal quality, 0 to 100.
    pub signal: Option<u32>,
    pub band: Option<String>,
    pub channel: Option<String>,
    /// The speed the PC and the router agreed on, in Mbps.
    pub link_mbps: Option<String>,
}

/// The connected Wi-Fi from `netsh wlan show interfaces`; `None` when there's none.
pub fn parse_wlan(text: &str) -> Option<Wifi> {
    // One block per adapter, each starting at its name.
    let mut blocks: Vec<Vec<(&str, &str)>> = vec![];
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim(), v.trim());
        if k == "Name" {
            blocks.push(vec![]);
        }
        if let Some(b) = blocks.last_mut() {
            b.push((k, v));
        }
    }
    let b = blocks.iter().find(|b| b.contains(&("State", "connected")))?;
    let get = |key: &str| b.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string()).filter(|v| !v.is_empty());
    Some(Wifi {
        signal: get("Signal").and_then(|s| s.trim_end_matches('%').trim().parse().ok()),
        band: get("Band"),
        channel: get("Channel"),
        link_mbps: get("Receive rate (Mbps)"),
    })
}

pub fn signal_words(pct: u32) -> &'static str {
    match pct {
        80.. => "Strong",
        60.. => "Good",
        40.. => "Fair",
        _ => "Weak",
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
    fn a_dropped_try_doesnt_stop_the_connection_test() {
        let mut n = 0;
        let (ok, failed, err) = attempts(5, || {
            n += 1;
            if n == 2 {
                Err(anyhow!("timed out"))
            } else {
                Ok(f64::from(n))
            }
        });
        assert_eq!((ok, failed), (vec![1.0, 3.0, 4.0, 5.0], 1));
        assert!(err.is_some());
        let mut tries = 0;
        let (ok, failed, err) = attempts(5, || {
            tries += 1;
            Err(anyhow!("no route"))
        });
        assert_eq!((ok.len(), failed, err.map(|e| e.to_string())), (0, 2, Some("no route".into())));
        assert_eq!(tries, 2, "a dead connection isn't tried five times");
    }

    #[test]
    fn diagnosis_points_at_the_slow_part() {
        let ok = NetCheck { router_ms: Some(3.0), router_sent: 10, dns_ms: Some(20.0), internet_ms: Some(25.0), ..Default::default() };
        assert_eq!(diagnosis(&ok), None);
        assert!(diagnosis(&NetCheck { router_lost: 4, ..ok.clone() }).unwrap().contains("lost"));
        assert!(diagnosis(&NetCheck { router_ms: Some(45.0), ..ok.clone() }).unwrap().contains("router"));
        assert!(diagnosis(&NetCheck { internet_ms: Some(140.0), ..ok.clone() }).unwrap().contains("provider"));
    }

    #[test]
    fn jitter_is_the_average_step_between_round_trips() {
        let j = jitter(&[20.0, 25.0, 22.0, 30.0]).unwrap();
        assert!((j - 16.0 / 3.0).abs() < 1e-9);
        assert_eq!(jitter(&[5.0]), None);
    }

    #[test]
    fn speeds_read_plainly() {
        assert_eq!(speed_text(4.23), "4.2 Mbps");
        assert_eq!(speed_text(245.6), "246 Mbps");
        assert_eq!(speed_text(9.97), "10 Mbps");
    }

    #[test]
    fn says_what_a_connection_is_good_for() {
        assert!(good_for(500.0, Some(50.0)).starts_with("Plenty for a busy home"));
        assert!(good_for(120.0, Some(20.0)).contains("4K"));
        assert!(good_for(30.0, Some(10.0)).contains("HD video and video calls"));
        assert!(good_for(2.0, None).contains("stall"));
        let slow_up = good_for(6.0, Some(1.0));
        assert!(slow_up.contains("one screen") && slow_up.contains("freeze"), "{slow_up}");
    }

    #[test]
    fn plain_words_for_lag_when_busy() {
        assert!(lag_words(10.0).contains("games"));
        assert_eq!(lag_words(42.0), "Fine for calls");
        assert!(lag_words(120.0).contains("stutter"));
    }

    #[test]
    fn steadiness_joins_jitter_and_loss() {
        assert_eq!(steadiness(Some(3.0), 0, 20).as_deref(), Some("Steady, no packets lost"));
        assert_eq!(steadiness(Some(15.0), 2, 20).as_deref(), Some("A little uneven, 2 of 20 packets lost"));
        assert!(steadiness(Some(40.0), 0, 20).unwrap().starts_with("Uneven"));
        assert_eq!(steadiness(None, 1, 10).as_deref(), Some("1 of 10 packets lost"));
        assert_eq!(steadiness(None, 0, 0), None);
    }

    const WLAN: &str = "
There is 1 interface on the system:

    Name                   : Wi-Fi
    Description            : Intel(R) Wi-Fi 6 AX201 160MHz
    State                  : connected
    SSID                   : Home
    Radio type             : 802.11ax
    Band                   : 5 GHz
    Channel                : 36
    Receive rate (Mbps)    : 866.7
    Transmit rate (Mbps)   : 650
    Signal                 : 88%
";

    #[test]
    fn reads_the_connected_wi_fi() {
        let w = parse_wlan(WLAN).unwrap();
        assert_eq!(w, Wifi { signal: Some(88), band: Some("5 GHz".into()), channel: Some("36".into()), link_mbps: Some("866.7".into()) });
        assert_eq!(parse_wlan(&WLAN.replace(": connected", ": disconnected")), None);
        assert_eq!(parse_wlan("There is no wireless interface on the system."), None);
        assert_eq!(signal_words(88), "Strong");
        assert_eq!(signal_words(65), "Good");
        assert_eq!(signal_words(45), "Fair");
        assert_eq!(signal_words(20), "Weak");
    }

    #[test]
    fn median_handles_odd_even_and_empty() {
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(vec![4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(vec![]), None);
    }

    #[test]
    fn plain_words_for_response_times() {
        assert!(latency_words(20.0).starts_with("Quick"));
        assert!(latency_words(120.0).contains("lag"));
    }

    /// Runs the real check: pings the router, looks up three names, connects to Microsoft's
    /// website and runs M-Lab's speed test, which publishes the result.
    #[test]
    #[ignore = "goes online"]
    fn checks_this_connection() {
        let gw = std::env::var("BOOTMEND_GATEWAY").ok().and_then(|g| g.parse().ok());
        let t = Instant::now();
        let c = run(gw, &|phase, mbps| println!("{phase:?} {mbps:?}"));
        println!("{c:#?} in {:?}", t.elapsed());
    }

    #[test]
    fn lost_packets_leave_out_a_silent_router() {
        let c = NetCheck { router_ms: Some(3.0), router_sent: 10, router_lost: 1, internet_sent: 10, internet_lost: 2, ..Default::default() };
        assert_eq!(c.packets(), (3, 20));
        assert_eq!(NetCheck { router_ms: None, router_lost: 10, ..c }.packets(), (2, 10));
    }
}
