#![cfg_attr(not(test), windows_subsystem = "windows")]

mod cleanup;
mod collect;
mod diff;
mod elevate;
mod fixes;
mod gui;
mod report;
mod rules;
mod scan;
mod snapshot;
mod util;

use anyhow::{anyhow, bail, Result};
use std::path::PathBuf;

const HELP: &str = "rigscan: a plain-English check-up for your PC

Usage:
  rigscan                               Open the app
  rigscan --report <folder> [--pdf] [--show-personal]
                                        Run a check without a window and save a report
  rigscan --screenshots <folder> [--theme light|dark] [--size WxH]
                                        Save a picture of every page (for testing)
";

struct Args {
    report: Option<PathBuf>,
    pdf: bool,
    show_personal: bool,
    screenshots: Option<PathBuf>,
    theme: Option<String>,
    size: Option<(f32, f32)>,
}

fn parse_args() -> Result<Args> {
    let mut a = Args { report: None, pdf: false, show_personal: false, screenshots: None, theme: None, size: None };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| anyhow!("{name} needs a value"));
        match x.as_str() {
            "--report" => a.report = Some(val("--report")?.into()),
            "--pdf" => a.pdf = true,
            "--show-personal" => a.show_personal = true,
            "--screenshots" => a.screenshots = Some(val("--screenshots")?.into()),
            "--theme" => {
                let t = val("--theme")?;
                if t != "light" && t != "dark" {
                    bail!("--theme must be light or dark");
                }
                a.theme = Some(t);
            }
            "--size" => {
                let v = val("--size")?;
                let (w, h) = v.split_once('x').ok_or_else(|| anyhow!("--size looks like 1180x780"))?;
                a.size = Some((w.parse()?, h.parse()?));
            }
            "-h" | "--help" | "/?" => {
                console();
                print!("{HELP}");
                std::process::exit(0);
            }
            other => bail!("Unknown option {other}. Run rigscan --help for the list."),
        }
    }
    Ok(a)
}

/// The app is a windowed program, so it has no console of its own. When started from a
/// terminal, borrow that terminal so messages show up there.
fn console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // Failing just means there's no terminal to write to; nothing else depends on it.
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

fn headless(out: PathBuf, pdf: bool, hide: bool) -> Result<()> {
    std::fs::create_dir_all(&out)?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || collect::collect(tx));
    let snap = loop {
        match rx.recv()? {
            collect::Progress::StepDone { key, ok } => eprintln!("{} {key}", if ok { "done" } else { "incomplete" }),
            collect::Progress::Finished(s) => break *s,
            collect::Progress::Failed(e) => bail!("The check didn't finish: {e}"),
        }
    };
    // Like a check in the app, this one is kept in History.
    snapshot::save(&snap)?;
    let host = if hide { "pc".to_string() } else { snap.host.clone() };
    let named = snapshot::Snapshot { host, ..snap.clone() };
    let path = out.join(report::default_name(&named, if pdf { "pdf" } else { "html" }));
    if pdf {
        report::write_pdf(&snap, hide, &path)?;
    } else {
        report::write_html(&snap, hide, &path)?;
    }
    println!("Saved {}", path.display());
    Ok(())
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            console();
            eprintln!("{e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    if let Some(out) = args.report {
        console();
        if let Err(e) = headless(out, args.pdf, !args.show_personal) {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
        return;
    }
    let (w, h) = args.size.unwrap_or((1180.0, 780.0));
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("rigscan")
            .with_app_id("rigscan")
            .with_inner_size([w, h])
            .with_min_inner_size([880.0, 600.0]),
        ..Default::default()
    };
    let theme = args.theme;
    let shots = args.screenshots;
    if let Some(dir) = &shots {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("Couldn't create {}: {e}", dir.display());
            std::process::exit(1);
        }
    }
    let r = eframe::run_native(
        "rigscan",
        options,
        Box::new(move |cc| {
            let app = gui::App::new(cc, shots);
            match theme.as_deref() {
                Some("dark") => cc.egui_ctx.set_theme(eframe::egui::Theme::Dark),
                Some("light") => cc.egui_ctx.set_theme(eframe::egui::Theme::Light),
                _ => {}
            }
            Ok(Box::new(app))
        }),
    );
    if let Err(e) = r {
        console();
        eprintln!("rigscan couldn't open its window: {e}");
        std::process::exit(1);
    }
}
