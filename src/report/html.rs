//! A self-contained HTML report. No scripts, no external files, readable offline and printable.

use crate::rules::{evaluate, score, Area, Level};
use crate::snapshot::Snapshot;
use crate::util::J;
use serde_json::Value;
use std::fmt::Write;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn kv(out: &mut String, title: &str, rows: &[(&str, Option<String>)]) {
    let rows: Vec<_> = rows.iter().filter_map(|(k, v)| v.as_ref().map(|v| (k, v))).collect();
    if rows.is_empty() {
        return;
    }
    let _ = write!(out, "<h3>{}</h3><table class=kv>", esc(title));
    for (k, v) in rows {
        let _ = write!(out, "<tr><th>{}</th><td>{}</td></tr>", esc(k), esc(v));
    }
    out.push_str("</table>");
}

fn table(out: &mut String, title: &str, head: &[&str], rows: Vec<Vec<String>>) {
    if rows.is_empty() {
        return;
    }
    let _ = write!(out, "<h3>{}</h3><div class=scroll><table class=list><thead><tr>", esc(title));
    for h in head {
        let _ = write!(out, "<th>{}</th>", esc(h));
    }
    out.push_str("</tr></thead><tbody>");
    for r in rows {
        out.push_str("<tr>");
        for c in r {
            let _ = write!(out, "<td>{}</td>", esc(&c));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></div>");
}

fn s(v: &Value, k: &str) -> String {
    v.s(k).unwrap_or_default()
}

fn opt_num(v: Option<f64>, unit: &str) -> Option<String> {
    v.map(|x| if x.fract() == 0.0 { format!("{x:.0}{unit}") } else { format!("{x:.1}{unit}") })
}

const CSS: &str = r#"
:root{--bg:#f6f7f9;--card:#fff;--ink:#1b1f24;--muted:#4a5361;--line:#dadfe6;--accent:#1f56c4;
--red:#b42318;--red-bg:#fdecea;--amber:#8a4b00;--amber-bg:#fff4e0;--blue:#2b5797;--blue-bg:#e8f0fb;--green:#1e7b3c;--green-bg:#e6f4ea}
@media (prefers-color-scheme:dark){:root{--bg:#16181c;--card:#1f2227;--ink:#e8eaed;--muted:#a9b0bb;--line:#2e333a;--accent:#8ab0ff;
--red:#ff9c94;--red-bg:#3a1d1b;--amber:#ffc46b;--amber-bg:#3a2c14;--blue:#9ec2ff;--blue-bg:#1c2a40;--green:#7bd88f;--green-bg:#17301f}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.55 "Segoe UI",system-ui,sans-serif}
main{max-width:960px;margin:0 auto;padding:32px 16px 48px}
h1{font-size:28px;line-height:1.2;margin:0 0 4px}h2{font-size:20px;margin:40px 0 12px}h3{font-size:16px;margin:24px 0 8px}
.muted{color:var(--muted)}.badge{display:inline-block;padding:2px 10px;border-radius:999px;font-size:13px;background:var(--blue-bg);color:var(--blue);margin-left:8px}
.areas{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:12px}
.area{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:16px}
.area b{font-size:28px;display:block}.area span{color:var(--muted);font-size:14px}
details{background:var(--card);border:1px solid var(--line);border-radius:10px;margin:8px 0;padding:12px 16px}
summary{cursor:pointer;font-weight:600}summary .pill{margin-right:8px}
.pill{display:inline-block;padding:1px 9px;border-radius:999px;font-size:12px;font-weight:600}
.l0{background:var(--red-bg);color:var(--red)}.l1{background:var(--amber-bg);color:var(--amber)}.l2{background:var(--blue-bg);color:var(--blue)}.l3{background:var(--green-bg);color:var(--green)}
ol,ul{padding-left:22px;margin:6px 0}.conf{font-size:13px;color:var(--muted)}
table{border-collapse:collapse;width:100%;background:var(--card);border:1px solid var(--line);border-radius:10px;overflow:hidden;font-size:14px}
th,td{text-align:left;padding:7px 12px;border-bottom:1px solid var(--line);vertical-align:top;overflow-wrap:anywhere}
.kv th{width:34%;color:var(--muted);font-weight:500}.list th{color:var(--muted);font-weight:600;white-space:nowrap}
.scroll{overflow-x:auto}footer{margin-top:48px;color:var(--muted);font-size:13px}
@media print{body{background:#fff}details{break-inside:avoid}details>*{display:block}h2{break-after:avoid}}
"#;

pub fn render(snap: &Snapshot, hidden: bool) -> String {
    let findings = evaluate(snap);
    let sys = &snap.system;
    let dev = sys.g("device");
    let os = sys.g("os");
    let mut o = String::with_capacity(64 * 1024);
    let model = format!("{} {}", s(dev, "Manufacturer"), s(dev, "Model")).trim().to_string();
    let _ = write!(
        o,
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>PC check: {}</title><style>{CSS}</style></head><body><main>",
        esc(&snap.host)
    );
    let _ = write!(
        o,
        "<h1>{}</h1><p class=muted>{} · checked {}{}</p>",
        if hidden { "PC check".to_string() } else { format!("PC check for {}", esc(&snap.host)) },
        esc(if model.is_empty() { "Unknown model" } else { &model }),
        esc(&snap.taken),
        if hidden { "<span class=badge>Personal details hidden</span>" } else { "" }
    );

    o.push_str("<h2>At a glance</h2><div class=areas>");
    for a in Area::ALL {
        let sc = score(&findings, a);
        let _ = write!(
            o,
            "<div class=area><span>{}</span><b>{}</b><span>{} · {}</span></div>",
            a.name(),
            sc.verdict,
            sc.value,
            if sc.open == 0 { "nothing to do".to_string() } else { crate::rules::plural(sc.open, "suggestion", "suggestions") }
        );
    }
    o.push_str("</div>");

    o.push_str("<h2>Suggestions</h2>");
    for f in &findings {
        let li = f.level as usize;
        let _ = write!(
            o,
            "<details{}><summary><span class=\"pill l{li}\">{}</span>{}</summary>",
            if f.level == Level::DoFirst { " open" } else { "" },
            f.level.name(),
            esc(&f.title)
        );
        let _ = write!(o, "<p>{}</p>", esc(&f.summary));
        if !f.steps.is_empty() {
            o.push_str("<p><b>What to do</b></p><ol>");
            for st in &f.steps {
                let _ = write!(o, "<li>{}</li>", esc(st));
            }
            o.push_str("</ol>");
        }
        if !f.details.is_empty() {
            o.push_str("<p><b>Details</b></p><ul>");
            for d in &f.details {
                let _ = write!(o, "<li>{}</li>", esc(d));
            }
            o.push_str("</ul>");
        }
        let _ = write!(o, "<p class=conf>{} · {}</p></details>", f.area.name(), f.confidence.name());
    }

    o.push_str("<h2>This PC</h2>");
    kv(
        &mut o,
        "Device",
        &[
            ("Maker and model", Some(model.clone()).filter(|m| !m.is_empty())),
            ("Motherboard", Some(format!("{} {}", s(dev, "BoardMaker"), s(dev, "BoardProduct")).trim().to_string()).filter(|m| !m.is_empty())),
            ("BIOS", dev.s("BIOSVersion").map(|v| format!("{v} ({})", s(dev, "BIOSDate")))),
            ("Serial number", dev.s("Serial")),
            ("Firmware", dev.s("FirmwareMode")),
            ("Secure Boot", dev.s("SecureBoot")),
            ("TPM", dev.s("TpmSpec").map(|v| format!("Version {v}"))),
        ],
    );
    kv(
        &mut o,
        "Windows",
        &[
            ("Edition", os.s("Edition")),
            ("Version", os.s("Version").map(|v| format!("{v} (build {})", s(os, "Build")))),
            ("Installed", os.s("Installed")),
            ("Activation", os.s("Activation")),
            ("Last restart", os.s("LastBoot")),
        ],
    );
    let cpu = sys.g("cpu");
    kv(
        &mut o,
        "Processor",
        &[
            ("Model", cpu.s("Name")),
            ("Cores and threads", cpu.i("Cores").map(|c| format!("{c} cores, {} threads", cpu.i("Threads").unwrap_or(0)))),
            ("Rated speed", cpu.f("RatedMHz").map(|m| format!("{:.2} GHz", m / 1000.0))),
        ],
    );
    let mem = sys.g("memory");
    kv(
        &mut o,
        "Memory",
        &[
            ("Installed", opt_num(mem.f("TotalGB"), " GB")),
            ("Slots", mem.i("Slots").map(|n| format!("{} of {n} used", mem.a("Sticks").len()))),
            ("Most supported", opt_num(mem.f("MaxSupportedGB"), " GB")),
        ],
    );
    table(
        &mut o,
        "Memory sticks",
        &["Slot", "Size", "Type", "Rated", "Running at", "Maker", "Part"],
        mem.a("Sticks")
            .iter()
            .map(|m| {
                vec![
                    s(m, "Slot"),
                    opt_num(m.f("SizeGB"), " GB").unwrap_or_default(),
                    s(m, "Type"),
                    opt_num(m.f("SpeedMTs"), " MT/s").unwrap_or_default(),
                    opt_num(m.f("ConfiguredMTs"), " MT/s").unwrap_or_default(),
                    s(m, "Maker"),
                    s(m, "Part"),
                ]
            })
            .collect(),
    );
    table(
        &mut o,
        "Graphics",
        &["Name", "Memory", "Driver", "Driver date", "Screen"],
        sys.a("gpus")
            .iter()
            .map(|g| {
                vec![
                    s(g, "Name"),
                    opt_num(g.f("VramMB").map(|v| v / 1024.0), " GB").unwrap_or_default(),
                    s(g, "Driver"),
                    s(g, "DriverDate"),
                    g.i("Width").map(|w| format!("{w} x {} at {} Hz", g.i("Height").unwrap_or(0), g.i("RefreshHz").unwrap_or(0))).unwrap_or_default(),
                ]
            })
            .collect(),
    );
    table(
        &mut o,
        "Displays",
        &["Name", "Size", "Connection", "Made"],
        sys.a("displays").iter().map(|d| vec![s(d, "Name"), opt_num(d.f("DiagonalIn"), " in").unwrap_or_default(), s(d, "Connection"), s(d, "Year")]).collect(),
    );
    let bat = sys.g("battery");
    if !bat.is_null() {
        kv(
            &mut o,
            "Battery",
            &[
                (
                    "Health",
                    match (bat.f("FullChargemWh"), bat.f("DesignmWh")) {
                        (Some(f), Some(d)) if d > 0.0 => Some(format!("{:.0}% of original capacity", f / d * 100.0)),
                        _ => None,
                    },
                ),
                ("Charge cycles", bat.s("Cycles")),
            ],
        );
    }

    o.push_str("<h2>Storage</h2>");
    table(
        &mut o,
        "Drives",
        &["Drive", "Type", "Life used", "Temperature", "Powered on", "Written", "Source"],
        snap.drives
            .iter()
            .map(|d| {
                vec![
                    d.model.clone(),
                    d.kind.clone(),
                    opt_num(d.life_used_pct, "%").unwrap_or_default(),
                    opt_num(d.temp_c, " °C").unwrap_or_default(),
                    d.power_on_hours.map(|h| format!("{:.0} hours", h)).unwrap_or_default(),
                    opt_num(d.tb_written.map(|t| (t * 10.0).round() / 10.0), " TB").unwrap_or_default(),
                    d.source.clone(),
                ]
            })
            .collect(),
    );
    table(
        &mut o,
        "Volumes",
        &["Drive", "Name", "Size", "Free", "Encryption"],
        snap.storage
            .a("volumes")
            .iter()
            .map(|v| {
                vec![
                    s(v, "Letter"),
                    s(v, "Label"),
                    opt_num(v.f("SizeGB"), " GB").unwrap_or_default(),
                    opt_num(v.f("FreeGB"), " GB").unwrap_or_default(),
                    s(v, "BitLocker"),
                ]
            })
            .collect(),
    );

    o.push_str("<h2>Security</h2>");
    let d = snap.security.g("defender");
    kv(
        &mut o,
        "Windows Security",
        &[
            ("Real-time protection", d.s("RealTime")),
            ("Tamper protection", d.s("Tamper")),
            ("Definitions updated", d.s("SignaturesUpdated")),
            ("Last quick scan", d.s("LastQuickScan")),
            ("Last full scan", d.s("LastFullScan")),
            ("Other antivirus", Some(snap.security.strs("otherAntivirus").join(", ")).filter(|x| !x.is_empty())),
        ],
    );
    table(
        &mut o,
        "Browser extensions",
        &["Browser", "Extension", "Version", "From the store", "Reads all sites", "On"],
        snap.extensions
            .iter()
            .map(|e| vec![e.browser.clone(), e.name.clone(), e.version.clone(), yes(e.from_store), yes(e.all_sites), yes(e.enabled)])
            .collect(),
    );
    table(
        &mut o,
        "Starts with Windows",
        &["Name", "Where", "On"],
        snap.software.a("startup").iter().map(|x| vec![s(x, "Name"), s(x, "Scope"), yes(x.b("Enabled") != Some(false))]).collect(),
    );

    o.push_str("<h2>Installed apps</h2>");
    table(
        &mut o,
        &format!("{} apps", snap.software.a("apps").len()),
        &["Name", "Version", "Publisher", "Installed"],
        snap.software.a("apps").iter().map(|a| vec![s(a, "Name"), s(a, "Version"), s(a, "Publisher"), s(a, "Installed")]).collect(),
    );

    let _ = write!(o, "<footer>Made by Bootmend on this PC on {}. This report was saved to your own computer; Bootmend did not upload it or anything else.</footer></main></body></html>", esc(&snap.taken));
    o
}

fn yes(b: bool) -> String {
    if b { "Yes" } else { "No" }.into()
}
