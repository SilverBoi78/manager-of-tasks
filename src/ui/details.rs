//! The Details tab: a flat list of every process with many optional columns.

use super::widgets::{HeaderAlign, app_icon, command_button, header_cell, page_header, right_text};
use super::{Nav, PageCtx};
use crate::actions::{PRIORITIES, priority_name};
use crate::sys::process::{Class, Proc};
use crate::util;
use egui::{Align, Layout, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as ph;
use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Col {
    Name,
    Pid,
    Ppid,
    Status,
    User,
    Cpu,
    CpuTime,
    Mem,
    Rss,
    Shared,
    Swap,
    Virtual,
    Threads,
    Priority,
    Nice,
    DiskRead,
    DiskWrite,
    Gpu,
    GpuMem,
    Started,
    Unit,
    Cmd,
}

const ALL: [Col; 22] = [
    Col::Name,
    Col::Pid,
    Col::Ppid,
    Col::Status,
    Col::User,
    Col::Cpu,
    Col::CpuTime,
    Col::Mem,
    Col::Rss,
    Col::Shared,
    Col::Swap,
    Col::Virtual,
    Col::Threads,
    Col::Priority,
    Col::Nice,
    Col::DiskRead,
    Col::DiskWrite,
    Col::Gpu,
    Col::GpuMem,
    Col::Started,
    Col::Unit,
    Col::Cmd,
];

const DEFAULT: [Col; 9] = [
    Col::Name,
    Col::Pid,
    Col::Status,
    Col::User,
    Col::Cpu,
    Col::Mem,
    Col::Threads,
    Col::Priority,
    Col::Cmd,
];

impl Col {
    fn title(self) -> &'static str {
        match self {
            Col::Name => "Name",
            Col::Pid => "PID",
            Col::Ppid => "Parent PID",
            Col::Status => "Status",
            Col::User => "User name",
            Col::Cpu => "CPU",
            Col::CpuTime => "CPU time",
            Col::Mem => "Memory (private)",
            Col::Rss => "Working set",
            Col::Shared => "Shared memory",
            Col::Swap => "Swap",
            Col::Virtual => "Virtual size",
            Col::Threads => "Threads",
            Col::Priority => "Base priority",
            Col::Nice => "Nice",
            Col::DiskRead => "Disk read (total)",
            Col::DiskWrite => "Disk write (total)",
            Col::Gpu => "GPU",
            Col::GpuMem => "GPU memory",
            Col::Started => "Started",
            Col::Unit => "systemd unit",
            Col::Cmd => "Command line",
        }
    }

    fn key(self) -> String {
        format!("{self:?}").to_lowercase()
    }

    fn numeric(self) -> bool {
        !matches!(
            self,
            Col::Name
                | Col::Status
                | Col::User
                | Col::Priority
                | Col::Unit
                | Col::Cmd
                | Col::Started
        )
    }

    fn cmp(self, a: &Proc, b: &Proc) -> Ordering {
        let f = |x: f64, y: f64| x.partial_cmp(&y).unwrap_or(Ordering::Equal);
        match self {
            Col::Name => a.exe_name.to_lowercase().cmp(&b.exe_name.to_lowercase()),
            Col::Pid => a.pid.cmp(&b.pid),
            Col::Ppid => a.ppid.cmp(&b.ppid),
            Col::Status => a.state.cmp(&b.state),
            Col::User => a.user.cmp(&b.user),
            Col::Cpu => f(a.cpu as f64, b.cpu as f64),
            Col::CpuTime => f(a.cpu_time, b.cpu_time),
            Col::Mem => a.mem.cmp(&b.mem),
            Col::Rss => a.rss.cmp(&b.rss),
            Col::Shared => a.shared.cmp(&b.shared),
            Col::Swap => a.swap.cmp(&b.swap),
            Col::Virtual => a.vsize.cmp(&b.vsize),
            Col::Threads => a.threads.cmp(&b.threads),
            Col::Priority | Col::Nice => a.nice.cmp(&b.nice),
            Col::DiskRead => a.read_total.cmp(&b.read_total),
            Col::DiskWrite => a.write_total.cmp(&b.write_total),
            Col::Gpu => f(a.gpu as f64, b.gpu as f64),
            Col::GpuMem => a.gpu_mem.cmp(&b.gpu_mem),
            Col::Started => f(a.start_time, b.start_time),
            Col::Unit => a.unit.cmp(&b.unit),
            Col::Cmd => a.cmdline.cmp(&b.cmdline),
        }
    }
}

pub fn state_name(c: char) -> &'static str {
    match c {
        'R' => "Running",
        'S' => "Sleeping",
        'D' => "Disk sleep",
        'T' | 't' => "Suspended",
        'Z' => "Zombie",
        'I' => "Idle",
        'X' | 'x' => "Dead",
        _ => "Unknown",
    }
}

#[derive(Default)]
pub struct DetailsPage {
    sort: Option<(Col, bool)>,
    selected: Option<i32>,
    scroll_to_selected: bool,
}

enum Intent {
    Select(i32),
    End(Vec<i32>, i32),
    Signal(Vec<i32>, i32),
    Efficiency(Vec<i32>, bool),
    Nice(i32, i32),
    Affinity(i32),
    OpenLocation(String),
    Search(String),
    Properties(i32),
    Service(String),
    Processes(i32),
    Copy(String),
}

fn visible(settings: &crate::settings::Settings) -> Vec<Col> {
    if settings.details_columns.is_empty() {
        return DEFAULT.to_vec();
    }
    ALL.into_iter()
        .filter(|c| *c == Col::Name || settings.details_columns.contains(&c.key()))
        .collect()
}

/// A process and all of its descendants.
pub fn tree(procs: &[Proc], root: i32) -> Vec<i32> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let parent = out[i];
        out.extend(
            procs
                .iter()
                .filter(|p| p.ppid == parent && p.pid != parent)
                .map(|p| p.pid),
        );
        i += 1;
    }
    out
}

impl DetailsPage {
    pub fn select(&mut self, pid: i32) {
        self.selected = Some(pid);
        self.scroll_to_selected = true;
    }

    pub fn ui(&mut self, ui: &mut Ui, ctx: &mut PageCtx) {
        let pal = ctx.pal;
        let snap = ctx.snap;
        let mut procs: Vec<&Proc> = snap
            .procs
            .iter()
            .filter(|p| ctx.settings.show_kernel_threads || p.class != Class::Kernel)
            .filter(|p| {
                ctx.search.is_empty()
                    || ctx.matches(&p.exe_name)
                    || ctx.matches(&p.name)
                    || ctx.matches(&p.pid.to_string())
                    || ctx.matches(&p.user)
            })
            .collect();
        let (col, asc) = self.sort.unwrap_or((Col::Name, true));
        procs.sort_by(|a, b| {
            let o = col.cmp(a, b);
            let o = if asc { o } else { o.reverse() };
            o.then(a.pid.cmp(&b.pid))
        });

        let selected = self
            .selected
            .and_then(|pid| snap.procs.iter().find(|p| p.pid == pid));
        let mut intents = Vec::new();
        page_header(ui, &pal, "Details", |ui| {
            if command_button(ui, &pal, ph::INFO, "Properties", selected.is_some()).clicked()
                && let Some(p) = selected
            {
                intents.push(Intent::Properties(p.pid));
            }
            if command_button(ui, &pal, ph::PROHIBIT, "End task", selected.is_some()).clicked()
                && let Some(p) = selected
            {
                intents.push(Intent::End(vec![p.pid], libc::SIGTERM));
            }
            if command_button(ui, &pal, ph::APP_WINDOW, "Run new task", true).clicked() {
                ctx.dialogs.open_run();
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Delete))
            && !ui.ctx().egui_wants_keyboard_input()
            && let Some(p) = selected
        {
            intents.push(Intent::End(vec![p.pid], libc::SIGTERM));
        }

        let cols = visible(ctx.settings);
        let scroll_row = if self.scroll_to_selected {
            self.scroll_to_selected = false;
            self.selected
                .and_then(|pid| procs.iter().position(|p| p.pid == pid))
        } else {
            None
        };
        let sort = self.sort;
        let mut new_sort = None;
        let mut toggle_col: Option<Col> = None;
        let mut reset_cols = false;
        let icons = ctx.icons;
        let sel_pid = self.selected;
        let ncpu = snap.cpu.info.logical;

        ui.scope(|ui| {
            ui.visuals_mut().selection.bg_fill = pal.selected;
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
            let mut table = TableBuilder::new(ui)
                .id_salt(("details-table", cols.len()))
                .striped(false)
                .resizable(true)
                .sense(Sense::click())
                .auto_shrink(false)
                .cell_layout(Layout::left_to_right(Align::Center));
            for (i, c) in cols.iter().enumerate() {
                let last = i == cols.len() - 1;
                table = table.column(if last {
                    Column::remainder().at_least(80.0).clip(true)
                } else {
                    match c {
                        Col::Name => Column::initial(200.0).at_least(100.0).clip(true),
                        Col::User | Col::Status | Col::Priority => {
                            Column::initial(95.0).at_least(50.0).clip(true)
                        }
                        Col::Started => Column::initial(140.0).at_least(60.0).clip(true),
                        Col::Unit => Column::initial(180.0).at_least(60.0).clip(true),
                        _ => Column::initial(84.0).at_least(44.0).clip(true),
                    }
                });
            }
            if let Some(r) = scroll_row {
                table = table.scroll_to_row(r, Some(Align::Center));
            }
            table
                .header(32.0, |mut header| {
                    for c in &cols {
                        header.col(|ui| {
                            let sorted = match sort {
                                Some((sc, a)) if sc == *c => Some(a),
                                None if *c == Col::Name => Some(true),
                                _ => None,
                            };
                            let align = if c.numeric() {
                                HeaderAlign::Right
                            } else {
                                HeaderAlign::Left
                            };
                            let resp = header_cell(ui, &pal, c.title(), None, sorted, align);
                            if resp.clicked() {
                                new_sort = Some(match sort {
                                    Some((sc, a)) if sc == *c => (*c, !a),
                                    _ => (*c, !c.numeric()),
                                });
                            }
                            resp.context_menu(|ui| {
                                ui.label(RichText::new("Select columns").color(pal.text_weak));
                                ui.separator();
                                for c in ALL.iter().skip(1) {
                                    let mut on = cols.contains(c);
                                    if ui.checkbox(&mut on, c.title()).clicked() {
                                        toggle_col = Some(*c);
                                    }
                                }
                                ui.separator();
                                if ui.button("Reset columns").clicked() {
                                    reset_cols = true;
                                    ui.close();
                                }
                            });
                        });
                    }
                })
                .body(|body| {
                    body.rows(28.0, procs.len(), |mut row| {
                        let p = procs[row.index()];
                        row.set_selected(sel_pid == Some(p.pid));
                        for c in &cols {
                            row.col(|ui| cell(ui, &pal, icons, snap, p, *c));
                        }
                        let resp = row.response();
                        if resp.clicked() || resp.secondary_clicked() {
                            intents.push(Intent::Select(p.pid));
                        }
                        if resp.double_clicked() {
                            intents.push(Intent::Properties(p.pid));
                        }
                        resp.context_menu(|ui| {
                            context_menu(ui, &mut intents, &snap.procs, p, ncpu)
                        });
                    });
                });
        });

        if let Some(s) = new_sort {
            self.sort = Some(s);
        }
        if reset_cols {
            ctx.settings.details_columns.clear();
        } else if let Some(c) = toggle_col {
            let mut keys: Vec<String> = cols.iter().skip(1).map(|c| c.key()).collect();
            if let Some(i) = keys.iter().position(|k| *k == c.key()) {
                keys.remove(i);
            } else {
                keys.push(c.key());
            }
            if keys.is_empty() {
                keys.push("-".into());
            }
            ctx.settings.details_columns = keys;
        }
        for intent in intents {
            match intent {
                Intent::Select(pid) => self.selected = Some(pid),
                Intent::End(pids, sig) => {
                    let list: Vec<&Proc> = snap
                        .procs
                        .iter()
                        .filter(|p| pids.contains(&p.pid))
                        .collect();
                    ctx.dialogs.end_task(&list, snap.my_uid, sig, ctx.actions);
                }
                Intent::Signal(pids, sig) => ctx.actions.signal(pids, sig),
                Intent::Efficiency(pids, on) => {
                    ctx.dialogs.efficiency(pids, on, ctx.settings, ctx.actions)
                }
                Intent::Nice(pid, nice) => ctx.actions.set_nice(vec![pid], nice),
                Intent::Affinity(pid) => {
                    let name = snap
                        .procs
                        .iter()
                        .find(|p| p.pid == pid)
                        .map(|p| p.exe_name.to_string())
                        .unwrap_or_default();
                    ctx.dialogs.open_affinity(pid, &name, ncpu);
                }
                Intent::OpenLocation(path) => ctx.actions.open_location(path),
                Intent::Search(q) => ctx.actions.search_online(&q),
                Intent::Properties(pid) => ctx.dialogs.open_properties(pid),
                Intent::Service(unit) => *ctx.nav = Some(Nav::Services(unit)),
                Intent::Processes(pid) => *ctx.nav = Some(Nav::Processes(pid)),
                Intent::Copy(t) => ui.ctx().copy_text(t),
            }
        }
    }
}

fn cell(
    ui: &mut Ui,
    pal: &crate::theme::Palette,
    icons: Option<&crate::sys::icons::IconIndex>,
    snap: &crate::sys::Snapshot,
    p: &Proc,
    c: Col,
) {
    let weak = |s: String| RichText::new(s).color(pal.text_weak);
    match c {
        Col::Name => {
            ui.add_space(4.0);
            let fallback = if p.class == Class::Kernel {
                ph::CPU
            } else {
                ph::GEAR_SIX
            };
            app_icon(ui, pal, icons, p.icon.as_deref(), fallback, 16.0);
            ui.add(egui::Label::new(p.exe_name.as_ref()).truncate());
        }
        Col::Pid => right_text(ui, p.pid.to_string()),
        Col::Ppid => right_text(ui, p.ppid.to_string()),
        Col::Status => {
            ui.label(state_name(p.state));
        }
        Col::User => {
            ui.add(egui::Label::new(p.user.as_ref()).truncate());
        }
        Col::Cpu => right_text(ui, format!("{:02.0}", p.cpu)),
        Col::CpuTime => right_text(ui, util::cpu_time(p.cpu_time)),
        Col::Mem => right_text(ui, util::mem_k(p.mem)),
        Col::Rss => right_text(ui, util::mem_k(p.rss)),
        Col::Shared => right_text(ui, util::mem_k(p.shared)),
        Col::Swap => right_text(ui, util::mem_k(p.swap)),
        Col::Virtual => right_text(ui, util::mem_k(p.vsize)),
        Col::Threads => right_text(ui, p.threads.to_string()),
        Col::Priority => {
            ui.label(priority_name(p.nice));
        }
        Col::Nice => right_text(ui, p.nice.to_string()),
        Col::DiskRead => right_text(ui, util::bytes(p.read_total)),
        Col::DiskWrite => right_text(ui, util::bytes(p.write_total)),
        Col::Gpu => right_text(ui, format!("{:02.0}", p.gpu)),
        Col::GpuMem => right_text(
            ui,
            if p.gpu_mem > 0 {
                util::mem_k(p.gpu_mem)
            } else {
                String::new()
            },
        ),
        Col::Started => {
            ui.label(util::local_datetime(snap.boot_time + p.start_time as u64));
        }
        Col::Unit => {
            ui.add(egui::Label::new(weak(p.unit.to_string())).truncate());
        }
        Col::Cmd => {
            ui.add(egui::Label::new(weak(p.cmdline.to_string())).truncate());
        }
    }
}

fn context_menu(ui: &mut Ui, intents: &mut Vec<Intent>, all: &[Proc], p: &Proc, _ncpu: usize) {
    ui.set_min_width(220.0);
    if ui.button(format!("{}  End task", ph::PROHIBIT)).clicked() {
        intents.push(Intent::End(vec![p.pid], libc::SIGTERM));
        ui.close();
    }
    if ui
        .button(format!("{}  End process tree", ph::TREE_STRUCTURE))
        .clicked()
    {
        intents.push(Intent::End(tree(all, p.pid), libc::SIGTERM));
        ui.close();
    }
    if ui
        .button(format!("{}  Force end task", ph::SKULL))
        .clicked()
    {
        intents.push(Intent::End(vec![p.pid], libc::SIGKILL));
        ui.close();
    }
    if p.state == 'T' || p.state == 't' {
        if ui.button(format!("{}  Resume", ph::PLAY)).clicked() {
            intents.push(Intent::Signal(vec![p.pid], libc::SIGCONT));
            ui.close();
        }
    } else if ui.button(format!("{}  Suspend", ph::PAUSE)).clicked() {
        intents.push(Intent::Signal(vec![p.pid], libc::SIGSTOP));
        ui.close();
    }
    let mut eff = p.nice >= 19;
    if ui.checkbox(&mut eff, "Efficiency mode").clicked() {
        intents.push(Intent::Efficiency(vec![p.pid], eff));
        ui.close();
    }
    ui.separator();
    ui.menu_button(format!("{}  Set priority", ph::GAUGE), |ui| {
        let current = priority_name(p.nice);
        for (name, nice) in PRIORITIES {
            if ui.radio(current == *name, *name).clicked() {
                intents.push(Intent::Nice(p.pid, *nice));
                ui.close();
            }
        }
    });
    if ui.button(format!("{}  Set affinity", ph::CPU)).clicked() {
        intents.push(Intent::Affinity(p.pid));
        ui.close();
    }
    ui.separator();
    if let Some(path) = &p.exe_path
        && ui
            .button(format!("{}  Open file location", ph::FOLDER_OPEN))
            .clicked()
    {
        intents.push(Intent::OpenLocation(path.to_string()));
        ui.close();
    }
    if ui.button(format!("{}  Search online", ph::GLOBE)).clicked() {
        intents.push(Intent::Search(p.exe_name.to_string()));
        ui.close();
    }
    if ui.button(format!("{}  Copy", ph::COPY)).clicked() {
        intents.push(Intent::Copy(format!(
            "{}\t{}\t{}\t{}\t{:.1}%\t{}\t{}",
            p.exe_name,
            p.pid,
            state_name(p.state),
            p.user,
            p.cpu,
            util::mem_k(p.mem),
            p.cmdline
        )));
        ui.close();
    }
    if ui.button(format!("{}  Properties", ph::INFO)).clicked() {
        intents.push(Intent::Properties(p.pid));
        ui.close();
    }
    if p.class != Class::Kernel
        && ui
            .button(format!("{}  Go to process", ph::SQUARES_FOUR))
            .clicked()
    {
        intents.push(Intent::Processes(p.pid));
        ui.close();
    }
    if p.unit.ends_with(".service")
        && ui
            .button(format!("{}  Go to service", ph::PUZZLE_PIECE))
            .clicked()
    {
        intents.push(Intent::Service(p.unit.to_string()));
        ui.close();
    }
}
