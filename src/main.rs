#![cfg_attr(not(test), windows_subsystem = "windows")]

#[macro_use]
mod log;

mod cleanup;
mod collect;
mod diff;
mod elevate;
mod fixes;
mod gui;
mod live;
mod nettest;
mod report;
mod rules;
mod scan;
mod schedule;
mod snapshot;
mod trends;
mod util;

use anyhow::{anyhow, bail, Result};
use std::path::PathBuf;

const HELP: &str = "Bootmend: a plain-English check-up for your PC

Usage:
  bootmend                              Open the app
  bootmend --report <folder> [--pdf] [--show-personal]
                                        Run a check without a window and save a report
  bootmend --screenshots <folder> [--theme light|dark] [--size WxH]
                                        Save a picture of every page (for testing)
  bootmend --resume                     Carry on with fixes after restarting as administrator
";

struct Args {
    report: Option<PathBuf>,
    pdf: bool,
    show_personal: bool,
    scheduled: bool,
    screenshots: Option<PathBuf>,
    theme: Option<String>,
    size: Option<(f32, f32)>,
    resume: bool,
}

fn parse_args() -> Result<Args> {
    let mut a = Args { report: None, pdf: false, show_personal: false, scheduled: false, screenshots: None, theme: None, size: None, resume: false };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| anyhow!("{name} needs a value"));
        match x.as_str() {
            "--report" => a.report = Some(val("--report")?.into()),
            "--pdf" => a.pdf = true,
            "--show-personal" => a.show_personal = true,
            "--scheduled" => a.scheduled = true,
            "--resume" => a.resume = true,
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
            other => bail!("Unknown option {other}. Run bootmend --help for the list."),
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

/// Runs the full check without a window.
fn collect_now() -> Result<snapshot::Snapshot> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || collect::collect(tx));
    loop {
        match rx.recv()? {
            collect::Progress::StepDone { key, ok } => eprintln!("{} {key}", if ok { "done" } else { "incomplete" }),
            collect::Progress::Finished(s) => return Ok(*s),
            collect::Progress::Failed(e) => bail!("The check didn't finish: {e}"),
        }
    }
}

/// Work that runs without a window has nowhere to show errors, so they go to the log.
fn log_error(e: &anyhow::Error) {
    log!("error", "{e:#}");
}

fn scheduled() {
    if let Err(e) = collect_now().and_then(schedule::run_scheduled) {
        log_error(&e);
    }
}

/// The app used to be called rigscan. The first time it runs under the new name, its saved checks
/// move to the new folder, and check-ups set up under the old name move over in the background.
fn move_from_old_name() -> Option<Result<bool>> {
    std::env::var_os("LOCALAPPDATA").map(|base| util::move_old_data(std::path::Path::new(&base)))
}

fn after_move(moved: Option<Result<bool>>) {
    match moved {
        Some(Ok(true)) => {
            log!("app", "moved saved checks, settings and undo history over from the old rigscan folder");
            std::thread::spawn(|| {
                let (every, hour) = gui::saved_schedule();
                if let Err(e) = schedule::move_old_task(every, hour) {
                    log_error(&e);
                }
            });
        }
        Some(Ok(false)) | None => {}
        Some(Err(e)) => log_error(&e),
    }
}

fn headless(out: PathBuf, pdf: bool, hide: bool) -> Result<()> {
    std::fs::create_dir_all(&out)?;
    let snap = collect_now()?;
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
    log!("report", "saved {}", path.display());
    println!("Saved {}", path.display());
    Ok(())
}

fn main() {
    // The move only runs while the new folder doesn't exist yet, and starting the log can create it.
    let moved = move_from_old_name();
    log::init();
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            log!("app", "couldn't read the command line: {e}");
            console();
            eprintln!("{e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    let mode = match () {
        _ if args.scheduled => "a scheduled check-up",
        _ if args.report.is_some() => "a check without a window",
        _ if args.screenshots.is_some() => "screenshots",
        _ if args.resume => "the window, carrying on with fixes",
        _ => "the window",
    };
    let admin = if elevate::is_elevated() { " as administrator" } else { "" };
    log!("app", "Bootmend {} started{admin} for {mode}", env!("CARGO_PKG_VERSION"));
    after_move(moved);
    if args.scheduled {
        scheduled();
        return;
    }
    if let Some(out) = args.report {
        console();
        if let Err(e) = headless(out, args.pdf, !args.show_personal) {
            log_error(&e);
            eprintln!("{e:#}");
            std::process::exit(1);
        }
        return;
    }
    let (w, h) = args.size.unwrap_or((1180.0, 780.0));
    let mut viewport =
        eframe::egui::ViewportBuilder::default().with_title(util::APP).with_app_id("bootmend").with_inner_size([w, h]).with_min_inner_size([360.0, 320.0]);
    // Without this, eframe puts its own default icon in the title bar.
    match eframe::icon_data::from_png_bytes(include_bytes!("../assets/bootmend-256.png")) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(e) => log!("app", "couldn't load the window icon: {e}"),
    }
    let shots = args.screenshots;
    if shots.is_some() {
        // Screenshots are taken in a window parked off-screen with no taskbar button, so
        // nothing appears while they're taken.
        viewport = viewport.with_position([-20000.0, -20000.0]).with_taskbar(false).with_active(false);
    }
    // Screenshots stay on solid colours so they look the same on every PC.
    let mica = shots.is_none() && gui::mica_available();
    if mica {
        viewport = viewport.with_transparent(true);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let theme = args.theme;
    let resume = args.resume;
    if let Some(dir) = &shots {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("Couldn't create {}: {e}", dir.display());
            std::process::exit(1);
        }
    }
    let r = eframe::run_native(
        util::APP,
        options,
        Box::new(move |cc| {
            let app = gui::App::new(cc, shots, mica, resume);
            match theme.as_deref() {
                Some("dark") => cc.egui_ctx.set_theme(eframe::egui::Theme::Dark),
                Some("light") => cc.egui_ctx.set_theme(eframe::egui::Theme::Light),
                _ => {}
            }
            Ok(Box::new(app))
        }),
    );
    if let Err(e) = r {
        log!("app", "couldn't open the window: {e}");
        console();
        eprintln!("Bootmend couldn't open its window: {e}");
        std::process::exit(1);
    }
    log!("app", "closed");
}
