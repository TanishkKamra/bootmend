//! "Copy my files to a USB drive", offered when the drive holding them is failing: pick a drive,
//! copy, then see where the files went.

use super::theme::{self, pal, S1, S2, S4, S5};
use super::widgets::{self, icon, muted};
use super::{App, Job};
use crate::cleanup::size_text;
use crate::rules::{join_and, plural};
use crate::usb_copy::{self, Copied, Folder, Found, Target};
use crate::util::shell_open;
use eframe::egui::{self, RichText, Ui};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// What looking found: the folders, what's in them, and the drives plugged in.
pub struct Looked {
    folders: Vec<Folder>,
    found: Found,
    drives: Vec<Target>,
}

pub enum Flow {
    Looking,
    Pick {
        found: Arc<Found>,
        /// "Documents, Desktop and Pictures".
        names: String,
        /// Each drive, and whether the files fit on it.
        drives: Vec<(Target, bool)>,
        chosen: Option<usize>,
    },
    Copying {
        total: u64,
        done: Arc<AtomicU64>,
        stop: Arc<AtomicBool>,
        drive: String,
        cloud: usize,
    },
    Over {
        r: Result<Copied, String>,
        drive: String,
        cloud: usize,
    },
    /// Looking didn't work.
    Failed(String),
}

fn drive_name(t: &Target) -> String {
    format!("{} ({}:)", t.name, t.letter)
}

/// What was pressed.
enum Press {
    Close,
    Again,
    Copy(usize),
    Stop,
    Open(PathBuf),
}

impl Flow {
    fn picked(l: Looked) -> Flow {
        let drives: Vec<(Target, bool)> = l
            .drives
            .into_iter()
            .map(|t| {
                let fits = usb_copy::needed(&l.found, t.cluster) <= t.free;
                (t, fits)
            })
            .collect();
        let chosen = drives.iter().position(|(_, fits)| *fits);
        let names = join_and(&l.folders.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
        Flow::Pick { found: Arc::new(l.found), names, drives, chosen }
    }

    /// Made-up drives and progress for the off-screen screenshots.
    pub fn sample(stage: u8) -> Flow {
        let usb = |letter, name: &str, gb: u64| Target { letter, name: name.into(), free: gb << 30, cluster: 131_072 };
        let cloud = 3;
        match stage {
            0 => Flow::Pick {
                found: Arc::new(Found::sample(12 << 30)),
                names: "Documents, Desktop and Pictures".into(),
                drives: vec![(usb('E', "SANDISK", 58), true), (usb('F', "Holiday photos", 3), false)],
                chosen: Some(0),
            },
            1 => Flow::Copying {
                total: 12 << 30,
                done: Arc::new(AtomicU64::new(5 << 30)),
                stop: Arc::new(AtomicBool::new(false)),
                drive: "SANDISK (E:)".into(),
                cloud,
            },
            _ => Flow::Over {
                r: Ok(Copied {
                    dir: PathBuf::from(r"E:\Bootmend copy 2026-11-16 13.42"),
                    files: 14_230,
                    bytes: 12 << 30,
                    failed: vec![r"Pictures\2019\IMG_0412.jpg".into(), r"Documents\old.pst".into()],
                    stopped: false,
                }),
                drive: "SANDISK (E:)".into(),
                cloud,
            },
        }
    }
}

impl App {
    /// Looks for drives and measures the folders. Only reads; copying waits for a press.
    pub(super) fn open_copy(&mut self, ctx: &egui::Context) {
        log!("action", "looking for USB drives to copy the folders to");
        self.copy = Some(Flow::Looking);
        self.spawn(ctx, || {
            Job::CopyLooked(
                usb_copy::look()
                    .map(|(folders, drives)| {
                        let found = usb_copy::survey(&folders);
                        Looked { folders, found, drives }
                    })
                    .map_err(|e| format!("{e:#}")),
            )
        });
    }

    pub(super) fn on_looked(&mut self, r: Result<Looked, String>) {
        // Closed while looking: nothing to show.
        if !matches!(self.copy, Some(Flow::Looking)) {
            return;
        }
        match &r {
            Ok(l) => log!(
                "action",
                "{} files ({} bytes) in {} folders, {} unreadable folders; {} drives",
                l.found.files(),
                l.found.bytes,
                l.folders.len(),
                l.found.unreadable,
                l.drives.len()
            ),
            Err(e) => log!("action", "couldn't look for drives: {e}"),
        }
        self.copy = Some(match r {
            Ok(l) => Flow::picked(l),
            Err(e) => Flow::Failed(e),
        });
    }

    pub(super) fn on_copied(&mut self, r: Result<Copied, String>) {
        if let Some(Flow::Copying { drive, cloud, .. }) = &self.copy {
            self.copy = Some(Flow::Over { r, drive: drive.clone(), cloud: *cloud });
        }
    }

    fn start_copy(&mut self, ctx: &egui::Context, i: usize) {
        let Some(Flow::Pick { found, drives, .. }) = &self.copy else { return };
        let Some((t, _)) = drives.get(i) else { return };
        let (found, root, drive) = (found.clone(), PathBuf::from(format!("{}:\\", t.letter)), drive_name(t));
        let (done, stop) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
        let (d, s) = (done.clone(), stop.clone());
        let cloud = found.cloud;
        self.copy = Some(Flow::Copying { total: found.bytes, done, stop, drive, cloud });
        self.spawn(ctx, move || {
            let r = usb_copy::fresh_dir(&root, chrono::Local::now().naive_local()).and_then(|dir| usb_copy::copy_to(&found, &dir, &d, &s));
            Job::Copied(r.map_err(|e| format!("{e:#}")))
        });
    }

    pub(super) fn copy_dialog(&mut self, ctx: &egui::Context) {
        let Some(flow) = &mut self.copy else { return };
        let p = pal(ctx);
        let mut press = None;
        let busy = matches!(flow, Flow::Copying { .. });
        if busy {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(S5 as i8));
        let m = egui::Modal::new(egui::Id::new("copy-files")).frame(frame).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 2.0 * (S5 + S4)).min(460.0));
            let heading = |ui: &mut Ui, title: &str| {
                let t = ui.label(RichText::new(title).text_style(theme::h2()));
                widgets::heading_role(ui, &t, 2, title);
                ui.add_space(S2);
            };
            match flow {
                Flow::Looking => {
                    heading(ui, "Copy your files to a USB drive");
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        muted(ui, "Looking for USB drives and measuring your files");
                    });
                    buttons(ui, |ui| {
                        if widgets::secondary(ui, "Cancel").clicked() {
                            press = Some(Press::Close);
                        }
                    });
                }
                Flow::Pick { found, names, drives, chosen } => {
                    heading(ui, "Copy your files to a USB drive");
                    if found.files() == 0 {
                        ui.label(format!("Your {names} have no files to copy."));
                        buttons(ui, |ui| {
                            if widgets::primary(ui, "Close").clicked() {
                                press = Some(Press::Close);
                            }
                        });
                        return;
                    }
                    if drives.is_empty() {
                        ui.label(format!("Plug in a USB drive or an external drive with more than {} free, then look again.", size_text(found.bytes)));
                        buttons(ui, |ui| {
                            if widgets::primary(ui, "Look again").clicked() {
                                press = Some(Press::Again);
                            }
                            if widgets::secondary(ui, "Cancel").clicked() {
                                press = Some(Press::Close);
                            }
                        });
                        return;
                    }
                    ui.label(format!(
                        "Bootmend copies your {names} ({}) into a new folder on the drive. Nothing on this PC is moved or deleted.",
                        size_text(found.bytes)
                    ));
                    ui.add_space(S4);
                    for (i, (t, fits)) in drives.iter().enumerate() {
                        let words = format!("{} · {} free{}", drive_name(t), size_text(t.free), if *fits { "" } else { ", not enough room" });
                        ui.add_enabled_ui(*fits, |ui| ui.radio_value(chosen, Some(i), words));
                        ui.add_space(S1);
                    }
                    ui.add_space(S2);
                    if ui.link("Plugged in another drive? Look again").clicked() {
                        press = Some(Press::Again);
                    }
                    buttons(ui, |ui| {
                        if widgets::primary_enabled(ui, "Copy", chosen.is_some()).clicked() {
                            press = chosen.map(Press::Copy);
                        }
                        if widgets::secondary(ui, "Cancel").clicked() {
                            press = Some(Press::Close);
                        }
                    });
                }
                Flow::Copying { total, done, stop, .. } => {
                    heading(ui, "Copying your files");
                    let d = done.load(Ordering::Relaxed).min(*total);
                    let frac = if *total == 0 { 1.0 } else { d as f32 / *total as f32 };
                    widgets::bar(ui, "Copied", frac, p.accent);
                    ui.add_space(S1);
                    muted(ui, format!("{} of {}", size_text(d), size_text(*total)));
                    ui.add_space(S2);
                    ui.label("Keep the drive plugged in until this finishes.");
                    let stopping = stop.load(Ordering::Relaxed);
                    buttons(ui, |ui| {
                        if widgets::secondary_enabled(ui, if stopping { "Stopping" } else { "Stop" }, !stopping).clicked() {
                            press = Some(Press::Stop);
                        }
                    });
                }
                Flow::Over { r: Ok(c), drive, cloud } => {
                    let folder = c.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let title = match (c.stopped, c.failed.is_empty()) {
                        (true, _) => "Copying stopped",
                        (false, true) => "Your files are copied",
                        (false, false) => "Most of your files are copied",
                    };
                    heading(ui, title);
                    if c.stopped {
                        ui.label(format!("{} copied to \"{folder}\" on {drive} before you stopped.", plural(c.files, "file was", "files were")));
                    } else {
                        ui.label(format!("{} ({}) are in \"{folder}\" on {drive}.", plural(c.files, "file", "files"), size_text(c.bytes)));
                    }
                    if !c.failed.is_empty() {
                        ui.add_space(S2);
                        ui.label(format!(
                            "{} couldn't be read, so {} copied. The failing drive may have damaged {}.",
                            plural(c.failed.len(), "file", "files"),
                            if c.failed.len() == 1 { "it wasn't" } else { "they weren't" },
                            if c.failed.len() == 1 { "it" } else { "them" }
                        ));
                        for f in c.failed.iter().take(3) {
                            muted(ui, f);
                        }
                        if c.failed.len() > 3 {
                            muted(ui, format!("and {} more", c.failed.len() - 3));
                        }
                    }
                    if *cloud > 0 {
                        ui.add_space(S2);
                        ui.label(format!(
                            "{} kept only in OneDrive {} already safe online, so Bootmend left them there.",
                            plural(*cloud, "file", "files"),
                            if *cloud == 1 { "is" } else { "are" }
                        ));
                    }
                    buttons(ui, |ui| {
                        if widgets::primary(ui, "Close").clicked() {
                            press = Some(Press::Close);
                        }
                        if c.files > 0 && widgets::icon_button(ui, icon::OPEN, "Open the folder", true).clicked() {
                            press = Some(Press::Open(c.dir.clone()));
                        }
                    });
                }
                Flow::Over { r: Err(e), .. } => {
                    heading(ui, "Copying didn't finish");
                    ui.label(format!("{e}."));
                    buttons(ui, |ui| {
                        if widgets::primary(ui, "Close").clicked() {
                            press = Some(Press::Close);
                        }
                    });
                }
                Flow::Failed(e) => {
                    heading(ui, "Copy your files to a USB drive");
                    ui.label(format!("Bootmend couldn't look for drives: {e}"));
                    buttons(ui, |ui| {
                        if widgets::primary(ui, "Look again").clicked() {
                            press = Some(Press::Again);
                        }
                        if widgets::secondary(ui, "Cancel").clicked() {
                            press = Some(Press::Close);
                        }
                    });
                }
            }
        });
        // Escape and clicks outside close it, except while copying: Stop does that.
        if m.should_close() && !busy && press.is_none() {
            press = Some(Press::Close);
        }
        match press {
            None => {}
            Some(Press::Close) => {
                log!("dialog", "closed the copy dialog");
                self.copy = None;
            }
            Some(Press::Again) => self.open_copy(ctx),
            Some(Press::Copy(i)) => self.start_copy(ctx, i),
            Some(Press::Stop) => {
                if let Some(Flow::Copying { stop, .. }) = &self.copy {
                    log!("action", "stopping the copy");
                    stop.store(true, Ordering::Relaxed);
                }
            }
            Some(Press::Open(dir)) => {
                if let Err(e) = shell_open(&dir.to_string_lossy()) {
                    self.toast(format!("{e:#}"), false);
                }
            }
        }
    }
}

/// The dialog's buttons, on the right as in Windows.
fn buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(S4);
    widgets::right_row(ui, add);
}
