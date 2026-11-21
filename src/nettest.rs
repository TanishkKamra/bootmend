//! Connection check, run only when the person asks: how quickly the router, the DNS server and a
//! website answer, so a delay can be placed in the home network or further out.

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

/// Runs the whole check. `router` is the default gateway, when Windows reported one.
pub fn run(router: Option<Ipv4Addr>) -> NetCheck {
    log!("net", "connection check started");
    let t0 = Instant::now();
    let c = check(router);
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
    for p in &c.problems {
        log!("net", "problem: {p}");
    }
    c
}

fn check(router: Option<Ipv4Addr>) -> NetCheck {
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

    const TRIES: usize = 5;
    let (connects, failed, err) = attempts(TRIES, || connect_ms(SITE, Duration::from_secs(10)));
    match err {
        Some(e) if connects.is_empty() => {
            c.problems.push(format!("Response time: {e:#}."));
            return c;
        }
        Some(_) => c.problems.push(format!("{failed} of {TRIES} tries got no answer, so the connection may be dropping out.")),
        None => {}
    }
    c.internet_ms = median(connects);
    c
}

/// Runs `once` the given number of times. A try that gets no answer doesn't stop the rest, since a
/// patchy connection is often why someone runs the test, but two misses before any answer do, so a
/// dead connection isn't waited on five times. Returns the answers, how many failed, and the last error.
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

    /// Runs the real check: pings the router, looks up three names and connects to Microsoft's website.
    #[test]
    #[ignore = "goes online"]
    fn checks_this_connection() {
        let gw = std::env::var("BOOTMEND_GATEWAY").ok().and_then(|g| g.parse().ok());
        let t = Instant::now();
        println!("{:#?} in {:?}", run(gw), t.elapsed());
    }
}
