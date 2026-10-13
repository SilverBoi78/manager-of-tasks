//! Modal dialogs and property windows.

use crate::actions::{Actions, get_affinity, priority_name};
use crate::settings::Settings;
use crate::sys::Snapshot;
use crate::sys::process::Proc;
use crate::theme::{Palette, regular, semibold};
use crate::util;
use egui::{Align, Id, Layout, RichText};
use egui_phosphor::regular as ph;

pub enum Pending {
    Signal { pids: Vec<i32>, sig: i32 },
    Efficiency { pids: Vec<i32>, on: bool },
}

pub struct Confirm {
    pub title: String,
    pub message: String,
    pub ok: String,
    pub action: Pending,
    pub dont_ask: Option<bool>,
}

pub struct RunTask {
    pub input: String,
    pub admin: bool,
    pub focus: bool,
}

pub struct Affinity {
    pub pid: i32,
    pub name: String,
    pub cpus: Vec<bool>,
}

pub struct Info {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

#[derive(Default)]
pub struct Dialogs {
    pub run: Option<RunTask>,
    pub confirm: Option<Confirm>,
    pub affinity: Option<Affinity>,
    pub properties: Vec<i32>,
    pub info: Option<Info>,
}

impl Dialogs {
    pub fn open_run(&mut self) {
        self.run = Some(RunTask {
            input: String::new(),
            admin: false,
            focus: true,
        });
    }

    pub fn open_affinity(&mut self, pid: i32, name: &str, ncpu: usize) {
        self.affinity = Some(Affinity {
            pid,
            name: name.to_string(),
            cpus: get_affinity(pid, ncpu),
        });
    }

    pub fn open_properties(&mut self, pid: i32) {
        if !self.properties.contains(&pid) {
            self.properties.push(pid);
        }
    }

    /// End (or kill) processes, asking first when they are critical or belong to another user.
    pub fn end_task(&mut self, procs: &[&Proc], my_uid: u32, sig: i32, actions: &Actions) {
        if procs.is_empty() {
            return;
        }
        let pids: Vec<i32> = procs.iter().map(|p| p.pid).collect();
        let critical = procs
            .iter()
            .find(|p| super::is_critical(&p.exe_name) || p.pid == 1);
        let foreign = procs.iter().any(|p| p.uid != my_uid);
        if critical.is_some() || foreign {
            let name = critical.unwrap_or(&procs[0]).name.to_string();
            let message = if critical.is_some() {
                format!(
                    "Ending {name} may make your desktop session unstable or log you out. Unsaved data in open apps will be lost.\n\nDo you want to end this process?"
                )
            } else {
                format!(
                    "{name} is a system process owned by another user. Ending it requires administrator rights and may make the system unstable.\n\nDo you want to end this process?"
                )
            };
            self.confirm = Some(Confirm {
                title: format!("Do you want to end {name}?"),
                message,
                ok: if sig == libc::SIGKILL {
                    "Force end".into()
                } else {
                    "End process".into()
                },
                action: Pending::Signal { pids, sig },
                dont_ask: None,
            });
        } else {
            actions.signal(pids, sig);
        }
    }

    pub fn efficiency(&mut self, pids: Vec<i32>, on: bool, settings: &Settings, actions: &Actions) {
        if on && settings.confirm_efficiency {
            self.confirm = Some(Confirm {
                title: "Turn on Efficiency mode?".into(),
                message: "Efficiency mode lowers the CPU and disk priority of the process so other apps stay responsive. The process may run slower. Turning it off again may require administrator rights.".into(),
                ok: "Turn on Efficiency mode".into(),
                action: Pending::Efficiency { pids, on },
                dont_ask: Some(false),
            });
        } else {
            actions.efficiency(pids, on);
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        pal: &Palette,
        snap: &Snapshot,
        actions: &Actions,
        settings: &mut Settings,
    ) {
        self.show_run(ctx, pal, actions);
        self.show_confirm(ctx, pal, actions, settings);
        self.show_affinity(ctx, pal, actions);
        self.show_info(ctx, pal);
        self.show_properties(ctx, pal, snap);
    }

    fn show_run(&mut self, ctx: &egui::Context, pal: &Palette, actions: &Actions) {
        let Some(run) = &mut self.run else { return };
        let mut close = false;
        let mut submit = false;
        let modal = egui::Modal::new(Id::new("run-task")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new("Create new task").font(semibold(18.0)));
            ui.add_space(6.0);
            ui.label(
                RichText::new("Type the name of a program, folder, document, or Internet resource, and Manager of Tasks will open it for you.")
                    .color(pal.text_weak),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Open:");
                let r = ui.add(
                    egui::TextEdit::singleline(&mut run.input)
                        .desired_width(f32::INFINITY)
                        .hint_text("e.g. firefox, konsole, ~/Documents"),
                );
                if run.focus {
                    r.request_focus();
                    run.focus = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }
            });
            ui.add_space(4.0);
            ui.checkbox(&mut run.admin, "Create this task with administrative privileges");
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                if ui.add(egui::Button::new(RichText::new("OK").color(egui::Color32::WHITE)).fill(pal.accent)).clicked() {
                    submit = true;
                }
            });
        });
        if submit {
            let cmd = expand_tilde(&run.input);
            actions.run_task(cmd, run.admin);
            close = true;
        }
        if close || modal.should_close() {
            self.run = None;
        }
    }

    fn show_confirm(
        &mut self,
        ctx: &egui::Context,
        pal: &Palette,
        actions: &Actions,
        settings: &mut Settings,
    ) {
        let Some(c) = &mut self.confirm else { return };
        let mut result = None;
        let modal = egui::Modal::new(Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(ph::WARNING)
                        .font(regular(24.0))
                        .color(pal.net),
                );
                ui.label(RichText::new(&c.title).font(semibold(16.0)));
            });
            ui.add_space(8.0);
            ui.label(&c.message);
            if let Some(d) = &mut c.dont_ask {
                ui.add_space(6.0);
                ui.checkbox(d, "Don't show again");
            }
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    result = Some(false);
                }
                if ui
                    .add(
                        egui::Button::new(RichText::new(&c.ok).color(egui::Color32::WHITE))
                            .fill(pal.accent),
                    )
                    .clicked()
                {
                    result = Some(true);
                }
            });
        });
        if modal.should_close() && result.is_none() {
            result = Some(false);
        }
        if let Some(ok) = result {
            let c = self.confirm.take().expect("checked above");
            if ok {
                if c.dont_ask == Some(true) {
                    settings.confirm_efficiency = false;
                }
                match c.action {
                    Pending::Signal { pids, sig } => actions.signal(pids, sig),
                    Pending::Efficiency { pids, on } => actions.efficiency(pids, on),
                }
            }
        }
    }

    fn show_affinity(&mut self, ctx: &egui::Context, pal: &Palette, actions: &Actions) {
        let Some(a) = &mut self.affinity else { return };
        let mut result = None;
        let modal = egui::Modal::new(Id::new("affinity")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new("Processor affinity").font(semibold(16.0)));
            ui.label(
                RichText::new(format!(
                    "Which processors are allowed to run {} ({})?",
                    a.name, a.pid
                ))
                .color(pal.text_weak),
            );
            ui.add_space(6.0);
            let mut all = a.cpus.iter().all(|c| *c);
            if ui.checkbox(&mut all, "<All Processors>").changed() {
                a.cpus.iter_mut().for_each(|c| *c = all);
            }
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    egui::Grid::new("cpus").num_columns(4).show(ui, |ui| {
                        for (i, c) in a.cpus.iter_mut().enumerate() {
                            ui.checkbox(c, format!("CPU {i}"));
                            if i % 4 == 3 {
                                ui.end_row();
                            }
                        }
                    });
                });
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    result = Some(false);
                }
                let any = a.cpus.iter().any(|c| *c);
                if ui
                    .add_enabled(
                        any,
                        egui::Button::new(RichText::new("OK").color(egui::Color32::WHITE))
                            .fill(pal.accent),
                    )
                    .clicked()
                {
                    result = Some(true);
                }
            });
        });
        if modal.should_close() && result.is_none() {
            result = Some(false);
        }
        if let Some(ok) = result {
            let a = self.affinity.take().expect("checked above");
            if ok {
                let cpus = a
                    .cpus
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| **c)
                    .map(|(i, _)| i)
                    .collect();
                actions.set_affinity(a.pid, cpus);
            }
        }
    }

    fn show_info(&mut self, ctx: &egui::Context, pal: &Palette) {
        let Some(info) = &self.info else { return };
        let mut open = true;
        egui::Window::new(&info.title)
            .id(Id::new("info-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(460.0)
            .show(ctx, |ui| kv_grid(ui, pal, "info-grid", &info.rows));
        if !open {
            self.info = None;
        }
    }

    fn show_properties(&mut self, ctx: &egui::Context, pal: &Palette, snap: &Snapshot) {
        let mut closed = Vec::new();
        for &pid in &self.properties {
            let mut open = true;
            let proc = snap.procs.iter().find(|p| p.pid == pid);
            let title = match proc {
                Some(p) => format!("{} Properties", p.name),
                None => format!("Process {pid} Properties"),
            };
            egui::Window::new(title)
                .id(Id::new(("props", pid)))
                .open(&mut open)
                .collapsible(false)
                .default_width(520.0)
                .show(ctx, |ui| match proc {
                    Some(p) => {
                        let rows = process_rows(p, snap);
                        kv_grid(ui, pal, ("props-grid", pid), &rows);
                    }
                    None => {
                        ui.label("This process has exited.");
                    }
                });
            if !open {
                closed.push(pid);
            }
        }
        self.properties.retain(|p| !closed.contains(p));
    }
}

fn kv_grid(
    ui: &mut egui::Ui,
    pal: &Palette,
    id: impl std::hash::Hash + std::fmt::Debug,
    rows: &[(String, String)],
) {
    egui::ScrollArea::vertical()
        .max_height(480.0)
        .show(ui, |ui| {
            egui::Grid::new(id)
                .num_columns(2)
                .spacing([16.0, 6.0])
                .striped(false)
                .show(ui, |ui| {
                    for (k, v) in rows {
                        ui.label(RichText::new(k).color(pal.text_weak));
                        ui.add(egui::Label::new(v).wrap().selectable(true));
                        ui.end_row();
                    }
                });
        });
}

fn process_rows(p: &Proc, snap: &Snapshot) -> Vec<(String, String)> {
    let parent = snap
        .procs
        .iter()
        .find(|x| x.pid == p.ppid)
        .map(|x| format!("{} ({})", x.name, x.pid))
        .unwrap_or_else(|| p.ppid.to_string());
    let cwd = std::fs::read_link(format!("/proc/{}/cwd", p.pid))
        .map(|c| c.display().to_string())
        .unwrap_or_else(|_| "Access denied".into());
    let started = snap.boot_time + p.start_time as u64;
    let state = match p.state {
        'R' => "Running",
        'S' => "Sleeping",
        'D' => "Waiting for I/O",
        'T' | 't' => "Suspended",
        'Z' => "Zombie",
        'I' => "Idle",
        _ => "Unknown",
    };
    let mut rows = vec![
        ("Name".into(), p.name.to_string()),
        ("Process name".into(), p.exe_name.to_string()),
        ("PID".into(), p.pid.to_string()),
        ("Parent".into(), parent),
        ("User".into(), p.user.to_string()),
        ("Status".into(), state.into()),
        (
            "Location".into(),
            p.exe_path.as_deref().unwrap_or("Access denied").to_string(),
        ),
        ("Command line".into(), p.cmdline.to_string()),
        ("Working directory".into(), cwd),
        ("Started".into(), util::local_datetime(started)),
        ("CPU time".into(), util::cpu_time(p.cpu_time)),
        ("Memory (private)".into(), util::mem_mb(p.mem)),
        ("Working set (RSS)".into(), util::mem_mb(p.rss)),
        ("Shared memory".into(), util::mem_mb(p.shared)),
        ("Swap".into(), util::mem_mb(p.swap)),
        ("Virtual size".into(), util::bytes(p.vsize)),
        ("Threads".into(), p.threads.to_string()),
        (
            "Priority".into(),
            format!("{} (nice {})", priority_name(p.nice), p.nice),
        ),
        ("Disk read (total)".into(), util::bytes(p.read_total)),
        ("Disk write (total)".into(), util::bytes(p.write_total)),
    ];
    if p.gpu_mem > 0 || p.gpu > 0.0 {
        rows.push(("GPU".into(), util::pct1(p.gpu)));
        rows.push(("GPU memory".into(), util::mem_mb(p.gpu_mem)));
    }
    if !p.unit.is_empty() {
        rows.push(("systemd unit".into(), p.unit.to_string()));
    }
    rows
}

fn expand_tilde(s: &str) -> String {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("~")
        && let Ok(home) = std::env::var("HOME")
    {
        return format!("{home}{rest}");
    }
    s.to_string()
}
