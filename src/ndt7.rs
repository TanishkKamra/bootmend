//! Download and upload speed, measured with Measurement Lab's ndt7 test only when the person
//! asks. It talks through Windows' own WinHTTP, so it uses Windows' TLS and proxy settings and
//! needs no extra libraries. M-Lab publishes every result, with the connection's internet
//! address, as open data; Bootmend says so before the first test.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::{Duration, Instant};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

const LOCATE: &str = "https://locate.measurementlab.net/v2/nearest/ndt/ndt7";
/// M-Lab's privacy policy, which says what it publishes.
pub const PRIVACY: &str = "https://www.measurementlab.net/privacy/";
const PROTOCOL: &str = "net.measurementlab.ndt.v7";
/// The server ends a test after 10 seconds and closes it by 13; past this, Bootmend stops too.
const LIMIT: Duration = Duration::from_secs(15);
/// How long Bootmend sends for in the upload test.
const UPLOAD_FOR: Duration = Duration::from_secs(10);
const FIRST_MESSAGE: usize = 1 << 13;
const LARGEST_MESSAGE: usize = 1 << 20;
/// How often the speed so far is passed on while a test runs.
const SHOW_EVERY: Duration = Duration::from_millis(250);

/// What a test server saw on its connection at one moment. Times are in microseconds.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TcpInfo {
    pub elapsed_us: Option<i64>,
    pub rtt_us: Option<i64>,
    pub min_rtt_us: Option<i64>,
    pub bytes_received: Option<i64>,
}

/// The test server M-Lab picked, and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Server {
    pub download: String,
    pub upload: String,
    pub place: Option<String>,
}

/// What the speed test found. Round trips are in ms.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Speed {
    pub down_mbps: Option<f64>,
    pub up_mbps: Option<f64>,
    /// The quietest round trip to the test server, and the typical one while the download filled the line.
    pub idle_ms: Option<f64>,
    pub loaded_ms: Option<f64>,
    /// Where the test server is, such as "London, GB".
    pub place: Option<String>,
}

impl Speed {
    /// How much longer a round trip takes while the line is full.
    pub fn lag_ms(&self) -> Option<f64> {
        Some((self.loaded_ms? - self.idle_ms?).max(0.0))
    }
}

/// Which half of the test is running.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Download,
    Upload,
}

/// The server's addresses from M-Lab's list of nearby servers.
pub fn parse_locate(body: &str) -> Result<Server> {
    let v: Value = serde_json::from_str(body).context("M-Lab's list of test servers wasn't readable")?;
    let r = v["results"].as_array().and_then(|a| a.first()).ok_or_else(|| anyhow!("M-Lab has no test server free right now. Try again in a few minutes"))?;
    let url = |k: &str| r["urls"][k].as_str().map(str::to_string).ok_or_else(|| anyhow!("M-Lab's test server gave no {k} address"));
    let loc = &r["location"];
    let place = match (loc["city"].as_str(), loc["country"].as_str()) {
        (Some(city), Some(country)) => Some(format!("{city}, {country}")),
        (Some(city), None) => Some(city.to_string()),
        _ => None,
    };
    Ok(Server { download: url("wss:///ndt/v7/download")?, upload: url("wss:///ndt/v7/upload")?, place })
}

/// The server's TCP figures from one of its measurement messages.
pub fn parse_measurement(text: &str) -> Option<TcpInfo> {
    let v: Value = serde_json::from_str(text).ok()?;
    let t = v.get("TCPInfo")?;
    let n = |k: &str| t.get(k).and_then(Value::as_i64);
    Some(TcpInfo { elapsed_us: n("ElapsedTime"), rtt_us: n("RTT"), min_rtt_us: n("MinRTT"), bytes_received: n("BytesReceived") })
}

pub fn mbps(bytes: u64, secs: f64) -> Option<f64> {
    (secs > 0.0 && bytes > 0).then(|| bytes as f64 * 8.0 / secs / 1e6)
}

/// Upload messages start at 8 KiB and double whenever they are under a sixteenth of what has
/// been sent, as the ndt7 spec suggests, up to 1 MiB.
pub fn next_size(size: usize, sent: u64) -> usize {
    if size < LARGEST_MESSAGE && (size as u64) < sent / 16 {
        size * 2
    } else {
        size
    }
}

/// Download speed from what arrived in `secs`, then the round trips the server saw: the
/// quietest one and the typical one while the download filled the line, in ms.
pub fn download_result(bytes: u64, secs: f64, seen: &[TcpInfo]) -> (Option<f64>, Option<f64>, Option<f64>) {
    let ms = |us: i64| us as f64 / 1000.0;
    let idle = seen.iter().filter_map(|t| t.min_rtt_us).filter(|&us| us > 0).min().map(ms);
    let loaded = crate::nettest::median(seen.iter().filter_map(|t| t.rtt_us).filter(|&us| us > 0).map(ms).collect());
    (mbps(bytes, secs), idle, loaded)
}

/// Upload speed as the server counted it, which leaves out what still sat in Windows' buffers;
/// `None` when the server said nothing.
pub fn upload_result(last: Option<&TcpInfo>) -> Option<f64> {
    let (b, us) = last.and_then(|t| Some((t.bytes_received?, t.elapsed_us?))).filter(|&(b, us)| b > 0 && us > 0)?;
    mbps(b as u64, us as f64 / 1e6)
}

/// The address with Bootmend's name and version added, as M-Lab asks of its clients.
pub fn with_client(url: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}client_name=bootmend&client_version={}", env!("CARGO_PKG_VERSION"))
}

/// Host, port and path of a secure address. Anything that isn't encrypted is refused.
pub fn split_url(url: &str) -> Result<(String, u16, String)> {
    // Messages leave out the query, which carries M-Lab's access token.
    let shown = url.split('?').next().unwrap_or_default();
    let rest = url.strip_prefix("wss://").or_else(|| url.strip_prefix("https://")).ok_or_else(|| anyhow!("{shown} isn't an encrypted address"))?;
    let (authority, path) = rest.find(['/', '?']).map_or((rest, "/"), |i| rest.split_at(i));
    let path = if path.starts_with('?') { format!("/{path}") } else { path.to_string() };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().with_context(|| format!("{shown} has an unreadable port"))?),
        None => (authority, 443),
    };
    if host.is_empty() {
        bail!("{shown} has no host name");
    }
    Ok((host.to_string(), port, path))
}

/// Finds the nearest free server, then measures download and then upload speed. `show` hears the
/// speed so far a few times a second. An upload that fails leaves the download result standing.
pub fn run(show: &dyn Fn(Phase, Option<f64>)) -> Result<Speed> {
    let server = parse_locate(&get(&with_client(LOCATE))?)?;
    log!("net", "speed test server: {}", server.place.as_deref().unwrap_or("unknown place"));
    let (down_mbps, idle_ms, loaded_ms) = download(&server.download, show)?;
    let up_mbps = upload(&server.upload, show).inspect_err(|e| log!("net", "upload test failed: {e:#}")).ok().flatten();
    Ok(Speed { down_mbps, up_mbps, idle_ms, loaded_ms, place: server.place })
}

fn download(url: &str, show: &dyn Fn(Phase, Option<f64>)) -> Result<(Option<f64>, Option<f64>, Option<f64>)> {
    let ws = Socket::open(&with_client(url))?;
    let start = Instant::now();
    let (mut bytes, mut seen, mut shown) = (0u64, vec![], start);
    let end = guarded(&ws, LIMIT + Duration::from_secs(2), || {
        ws.receive(
            start + LIMIT,
            |n| {
                bytes += n;
                if shown.elapsed() >= SHOW_EVERY {
                    shown = Instant::now();
                    show(Phase::Download, mbps(bytes, start.elapsed().as_secs_f64()));
                }
            },
            |text| seen.extend(parse_measurement(text)),
        )
    });
    let secs = start.elapsed().as_secs_f64();
    log!("net", "download: {bytes} bytes in {secs:.1}s, {} measurements, ended by {end}", seen.len());
    if bytes == 0 {
        bail!("the test server sent nothing ({end})");
    }
    Ok(download_result(bytes, secs, &seen))
}

fn upload(url: &str, show: &dyn Fn(Phase, Option<f64>)) -> Result<Option<f64>> {
    let ws = Socket::open(&with_client(url))?;
    let start = Instant::now();
    let mut last = None::<TcpInfo>;
    let (sent, secs, end) = guarded(&ws, LIMIT + Duration::from_secs(5), || {
        // The server's measurements are read only once sending stops. While a send is under way,
        // WinHTTP answers the server's pings with a broken frame ("FIN not set on control") and
        // M-Lab hangs up, so the speed so far counts what was handed to Windows.
        let data = filler(LARGEST_MESSAGE);
        let (mut sent, mut size, mut shown) = (0u64, FIRST_MESSAGE, start);
        while start.elapsed() < UPLOAD_FOR {
            let err = unsafe { WinHttpWebSocketSend(ws.ws.get(), WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE, Some(&data[..size])) };
            if err != 0 {
                log!("net", "upload stopped early: WinHTTP error {err}");
                break;
            }
            sent += size as u64;
            size = next_size(size, sent);
            if shown.elapsed() >= SHOW_EVERY {
                shown = Instant::now();
                show(Phase::Upload, mbps(sent, start.elapsed().as_secs_f64()));
            }
        }
        let secs = start.elapsed().as_secs_f64();
        // Asks the server to close; its queued measurements, then its own close, follow.
        let err = unsafe { WinHttpWebSocketShutdown(ws.ws.get(), WINHTTP_WEB_SOCKET_SUCCESS_CLOSE_STATUS.0 as u16, None, 0) };
        if err != 0 {
            log!("net", "couldn't ask the test server to close: WinHTTP error {err}");
        }
        let end = ws.receive(Instant::now() + Duration::from_secs(3), |_| {}, |text| last = parse_measurement(text).or(last.take()));
        (sent, secs, end)
    });
    log!("net", "upload: {sent} bytes in {secs:.1}s, ended by {end}");
    if sent == 0 {
        bail!("the test server took nothing");
    }
    Ok(upload_result(last.as_ref()))
}

/// Runs `f`, cutting the socket off if it is still going `after` from now, so a server that goes
/// quiet can't hold the test up.
fn guarded<T>(ws: &Socket, after: Duration, f: impl FnOnce() -> T) -> T {
    std::thread::scope(|s| {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        s.spawn(move || {
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(after) {
                log!("net", "the test server went quiet; cutting the connection");
                ws.ws.close();
            }
        });
        let r = f();
        drop(tx);
        r
    })
}

/// Bytes to send that no link along the way can shrink by compressing them.
fn filler(n: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

/// A WinHTTP handle, closed when dropped. Closing it from another thread cancels a call that
/// is waiting on it, which is how a server that goes quiet is cut off.
struct Handle(AtomicPtr<core::ffi::c_void>);

impl Handle {
    fn new(h: *mut core::ffi::c_void, what: &str) -> Result<Self> {
        if h.is_null() {
            return Err(windows::core::Error::from_win32()).context(what.to_string());
        }
        Ok(Handle(AtomicPtr::new(h)))
    }

    fn get(&self) -> *mut core::ffi::c_void {
        self.0.load(Ordering::Acquire)
    }

    fn close(&self) {
        let h = self.0.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !h.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(h);
            }
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.close();
    }
}

/// A session, connection and request for `url`, sent and answered. With `websocket`, it asks to
/// switch to a WebSocket that speaks ndt7.
fn request(url: &str, websocket: bool) -> Result<(Handle, Handle, Handle)> {
    let (host, port, path) = split_url(url)?;
    let agent = HSTRING::from(format!("Bootmend/{}", env!("CARGO_PKG_VERSION")));
    let session = Handle::new(
        unsafe { WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0) },
        "Windows couldn't open a web connection",
    )?;
    // Name lookup, connecting and sending get 10 seconds each; a reply may take 15.
    unsafe { WinHttpSetTimeouts(session.get(), 10_000, 10_000, 10_000, 15_000) }.context("Windows couldn't set the time limits")?;
    let connect = Handle::new(unsafe { WinHttpConnect(session.get(), &HSTRING::from(host.as_str()), port, 0) }, "Windows couldn't reach the test server")?;
    let req = Handle::new(
        unsafe {
            WinHttpOpenRequest(
                connect.get(),
                &HSTRING::from("GET"),
                &HSTRING::from(path.as_str()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        },
        "Windows couldn't start the request",
    )?;
    if websocket {
        unsafe { WinHttpSetOption(Some(req.get()), WINHTTP_OPTION_UPGRADE_TO_WEB_SOCKET, None) }.context("Windows couldn't ask for a WebSocket")?;
        let header: Vec<u16> = format!("Sec-WebSocket-Protocol: {PROTOCOL}\r\n").encode_utf16().collect();
        unsafe { WinHttpAddRequestHeaders(req.get(), &header, WINHTTP_ADDREQ_FLAG_ADD) }.context("Windows couldn't add the test's header")?;
    }
    unsafe { WinHttpSendRequest(req.get(), None, None, 0, 0, 0) }.with_context(|| format!("couldn't reach {host}; is the PC online?"))?;
    unsafe { WinHttpReceiveResponse(req.get(), std::ptr::null_mut()) }.with_context(|| format!("{host} didn't answer"))?;
    Ok((session, connect, req))
}

fn status(req: &Handle) -> Result<u32> {
    let (mut code, mut len) = (0u32, 4u32);
    unsafe {
        WinHttpQueryHeaders(
            req.get(),
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut code as *mut u32 as _),
            &mut len,
            std::ptr::null_mut(),
        )
    }
    .context("the answer had no status")?;
    Ok(code)
}

/// The body of an HTTPS address, up to 1 MiB.
fn get(url: &str) -> Result<String> {
    let (_session, _connect, req) = request(url, false)?;
    match status(&req)? {
        200 => {}
        204 => bail!("M-Lab has no test server free right now. Try again in a few minutes"),
        code => bail!("M-Lab's server list answered with error {code}"),
    }
    let (mut body, mut buf) = (Vec::new(), vec![0u8; 16 * 1024]);
    loop {
        let mut n = 0u32;
        unsafe { WinHttpReadData(req.get(), buf.as_mut_ptr() as _, buf.len() as u32, &mut n) }.context("M-Lab's server list stopped part way")?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n as usize]);
        if body.len() > 1 << 20 {
            bail!("M-Lab's server list was far larger than expected");
        }
    }
    String::from_utf8(body).context("M-Lab's server list wasn't text")
}

/// An open ndt7 WebSocket. The fields drop in order: the socket, then its connection and session.
struct Socket {
    ws: Handle,
    _connect: Handle,
    _session: Handle,
}

impl Socket {
    fn open(url: &str) -> Result<Self> {
        let (session, connect, req) = request(url, true)?;
        match status(&req)? {
            101 => {}
            code => bail!("the test server refused the test (error {code})"),
        }
        let ws = Handle::new(unsafe { WinHttpWebSocketCompleteUpgrade(req.get(), None) }, "Windows couldn't open the WebSocket")?;
        Ok(Socket { ws, _connect: connect, _session: session })
    }

    /// Reads until the server closes, an error, or `until`. Binary bytes go to `binary`; each
    /// whole text message goes to `text`. Says what ended it.
    fn receive(&self, until: Instant, mut binary: impl FnMut(u64), mut text: impl FnMut(&str)) -> &'static str {
        let mut buf = vec![0u8; 1 << 16];
        let mut message = Vec::new();
        while Instant::now() < until {
            let (mut n, mut kind) = (0u32, WINHTTP_WEB_SOCKET_BUFFER_TYPE(0));
            let err = unsafe { WinHttpWebSocketReceive(self.ws.get(), buf.as_mut_ptr() as _, buf.len() as u32, &mut n, &mut kind) };
            if err != 0 {
                log!("net", "the test connection ended: WinHTTP error {err}");
                return "an error";
            }
            let got = &buf[..n as usize];
            match kind {
                WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE | WINHTTP_WEB_SOCKET_BINARY_FRAGMENT_BUFFER_TYPE => binary(u64::from(n)),
                WINHTTP_WEB_SOCKET_UTF8_FRAGMENT_BUFFER_TYPE => message.extend_from_slice(got),
                WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE => {
                    message.extend_from_slice(got);
                    text(&String::from_utf8_lossy(&message));
                    message.clear();
                }
                WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE => {
                    let (mut code, mut reason, mut len) = (0u16, [0u8; 123], 0u32);
                    if unsafe { WinHttpWebSocketQueryCloseStatus(self.ws.get(), &mut code, Some(reason.as_mut_ptr() as _), reason.len() as u32, &mut len) } == 0
                    {
                        log!("net", "the test server closed the connection: {code} {}", String::from_utf8_lossy(&reason[..(len as usize).min(reason.len())]));
                    }
                    let err = unsafe { WinHttpWebSocketClose(self.ws.get(), WINHTTP_WEB_SOCKET_SUCCESS_CLOSE_STATUS.0 as u16, None, 0) };
                    if err != 0 {
                        log!("net", "couldn't finish closing the test connection: WinHTTP error {err}");
                    }
                    return "the server closing";
                }
                _ => {}
            }
        }
        "the time limit"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCATE: &str = r#"{"results":[
        {"machine":"mlab1-lhr03.mlab-oti.measurement-lab.org","location":{"city":"London","country":"GB"},
         "urls":{"ws:///ndt/v7/download":"ws://a/ndt/v7/download?access_token=x",
                 "wss:///ndt/v7/download":"wss://ndt-mlab1-lhr03.mlab-oti.measurement-lab.org/ndt/v7/download?access_token=d",
                 "wss:///ndt/v7/upload":"wss://ndt-mlab1-lhr03.mlab-oti.measurement-lab.org/ndt/v7/upload?access_token=u"}},
        {"machine":"mlab2-ams03","location":{"city":"Amsterdam","country":"NL"},"urls":{}}]}"#;

    #[test]
    fn takes_the_nearest_servers_encrypted_addresses() {
        let s = parse_locate(LOCATE).unwrap();
        assert_eq!(s.download, "wss://ndt-mlab1-lhr03.mlab-oti.measurement-lab.org/ndt/v7/download?access_token=d");
        assert_eq!(s.upload, "wss://ndt-mlab1-lhr03.mlab-oti.measurement-lab.org/ndt/v7/upload?access_token=u");
        assert_eq!(s.place.as_deref(), Some("London, GB"));
        assert!(parse_locate(r#"{"results":[]}"#).is_err(), "no server free");
        assert!(parse_locate("<html>").is_err());
        let no_place = parse_locate(&LOCATE.replace(r#""location":{"city":"London","country":"GB"},"#, "")).unwrap();
        assert_eq!(no_place.place, None);
    }

    #[test]
    fn reads_the_servers_tcp_figures() {
        let m = r#"{"ConnectionInfo":{"Client":"1.2.3.4:5"},"TCPInfo":{"ElapsedTime":2500000,"RTT":41000,"MinRTT":12000,"BytesReceived":900,"BytesAcked":5}}"#;
        assert_eq!(
            parse_measurement(m),
            Some(TcpInfo { elapsed_us: Some(2_500_000), rtt_us: Some(41_000), min_rtt_us: Some(12_000), bytes_received: Some(900) })
        );
        assert_eq!(parse_measurement(r#"{"TCPInfo":{"RTT":7}}"#), Some(TcpInfo { rtt_us: Some(7), ..Default::default() }));
        assert_eq!(parse_measurement(r#"{"AppInfo":{"NumBytes":1}}"#), None);
        assert_eq!(parse_measurement("not json"), None);
    }

    #[test]
    fn upload_messages_grow_with_the_connection() {
        assert_eq!(next_size(8192, 0), 8192);
        assert_eq!(next_size(8192, 8192 * 16), 8192, "not yet under a sixteenth");
        assert_eq!(next_size(8192, 8192 * 16 + 16), 16384);
        assert_eq!(next_size(1 << 20, u64::MAX), 1 << 20, "1 MiB at most");
    }

    #[test]
    fn download_speed_and_round_trips() {
        let seen = [
            TcpInfo { rtt_us: Some(20_000), min_rtt_us: Some(15_000), ..Default::default() },
            TcpInfo { rtt_us: Some(60_000), min_rtt_us: Some(12_000), ..Default::default() },
            TcpInfo { rtt_us: Some(40_000), min_rtt_us: Some(12_000), ..Default::default() },
        ];
        let (down, idle, loaded) = download_result(125_000_000, 10.0, &seen);
        assert_eq!(down, Some(100.0));
        assert_eq!(idle, Some(12.0));
        assert_eq!(loaded, Some(40.0));
        assert_eq!(download_result(0, 0.0, &[]), (None, None, None));
    }

    #[test]
    fn upload_speed_trusts_what_the_server_received() {
        let last = TcpInfo { elapsed_us: Some(10_000_000), bytes_received: Some(50_000_000), ..Default::default() };
        assert_eq!(upload_result(Some(&last)), Some(40.0));
        // What was handed to Windows includes what never left, so it isn't a result.
        assert_eq!(upload_result(None), None);
        assert_eq!(upload_result(Some(&TcpInfo::default())), None);
        assert_eq!(mbps(1, 0.0), None);
    }

    #[test]
    fn says_who_is_asking() {
        let v = env!("CARGO_PKG_VERSION");
        assert_eq!(with_client("wss://h/p?access_token=d"), format!("wss://h/p?access_token=d&client_name=bootmend&client_version={v}"));
        assert_eq!(with_client("https://h/p"), format!("https://h/p?client_name=bootmend&client_version={v}"));
    }

    #[test]
    fn splits_only_encrypted_addresses() {
        assert_eq!(split_url("wss://h.example/ndt/v7/upload?t=1").unwrap(), ("h.example".to_string(), 443, "/ndt/v7/upload?t=1".to_string()));
        assert_eq!(split_url("https://h.example:4443").unwrap(), ("h.example".to_string(), 4443, "/".to_string()));
        assert!(split_url("ws://h.example/ndt").is_err());
        assert!(split_url("http://h.example/").is_err());
        assert!(split_url("wss://:443/").is_err());
        assert!(split_url("wss://h:notaport/").is_err());
        // The query carries M-Lab's access token, which stays out of messages and the log.
        let e = split_url("ws://h.example/ndt?access_token=secret").unwrap_err().to_string();
        assert!(e.contains("h.example") && !e.contains("secret"), "{e}");
    }
}
