//! The Processes tab: apps, background processes and system processes,
//! grouped with heat-map columns.

use super::PageCtx;
use super::widgets::{
    HeaderAlign, app_icon, command_button, command_toggle, header_cell, heat_bg, page_header,
    right_text,
};
use crate::sys::process::{Class, Proc};
use crate::theme::semibold;
use crate::util;
use egui::{Align, Layout, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as ph;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Col {
    Name,
    Status,
    Pid,
    User,
    Cpu,
    Mem,
    Disk,
    Gpu,
    GpuEngine,
    GpuMem,
    Cmd,
}

impl Col {
    fn title(self) -> &'static str {
        match self {
            Col::Name => "Name",
            Col::Status => "Status",
            Col::Pid => "PID",
            Col::User => "User name",
            Col::Cpu => "CPU",
            Col::Mem => "Memory",
            Col::Disk => "Disk",
            Col::Gpu => "GPU",
            Col::GpuEngine => "GPU engine",
            Col::GpuMem => "GPU memory",
            Col::Cmd => "Command line",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Col::Pid => "pid",
            Col::User => "user",
            Col::GpuEngine => "gpu_engine",
            Col::GpuMem => "gpu_mem",
            Col::Cmd => "cmd",
            Col::Status => "status",
            _ => "",
        }
    }
    fn numeric(self) -> bool {
        matches!(
            self,
            Col::Pid | Col::Cpu | Col::Mem | Col::Disk | Col::Gpu | Col::GpuMem
        )
    }
}

const OPTIONAL: [Col; 6] = [
    Col::Status,
    Col::Pid,
    Col::User,
    Col::GpuEngine,
    Col::GpuMem,
    Col::Cmd,
];
const DEFAULT_OPTIONAL: [&str; 2] = ["status", "gpu_engine"];

#[derive(Clone, PartialEq, Eq, Debug)]
enum Sel {
    Group(Arc<str>),
    Pid(i32),
}

#[derive(Default)]
pub struct ProcessesPage {
    sort: Option<(Col, bool)>,
    expanded: HashSet<Arc<str>>,
    selected: Option<Sel>,
    pending_pid: Option<i32>,
    scroll_to_selected: bool,
}

struct Group<'a> {
    key: Arc<str>,
    class: Class,
    name: String,
    icon: Option<&'a str>,
    procs: Vec<&'a Proc>,
    main: &'a Proc,
    cpu: f32,
    mem: u64,
    disk: f64,
    gpu: f32,
    gpu_mem: u64,
    engine: Option<&'a str>,
    suspended: bool,
    efficiency: bool,
}

enum Row {
    Section(Class, usize),
    Group(usize),
    Child(usize, usize),
}

enum Intent {
    Select(Sel),
    Toggle(Arc<str>),
    End(Vec<i32>, i32),
    Efficiency(Vec<i32>, bool),
    Signal(Vec<i32>, i32),
    Details(i32),
    OpenLocation(String),
    Search(String),
    Properties(i32),
    Copy(String),
}

fn class_title(c: Class) -> &'static str {
    match c {
        Class::App => "Apps",
        Class::Background => "Background processes",
        Class::System => "System processes",
        Class::Kernel => "Kernel threads",
    }
}

fn status_of(p: &Proc) -> (&'static str, bool) {
    match p.state {
        'T' | 't' => ("Suspended", false),
        'Z' => ("Zombie", false),
        _ if p.nice >= 19 => ("", true),
        _ => ("", false),
    }
}

pub fn visible_optional(settings: &crate::settings::Settings) -> Vec<Col> {
    let keys: Vec<&str> = if settings.process_columns.is_empty() {
        DEFAULT_OPTIONAL.to_vec()
    } else {
        settings
            .process_columns
            .iter()
            .map(String::as_str)
            .collect()
    };
    OPTIONAL
        .into_iter()
        .filter(|c| keys.contains(&c.key()))
        .collect()
}

fn toggle_column(settings: &mut crate::settings::Settings, col: Col) {
    let mut keys: Vec<String> = visible_optional(settings)
        .iter()
        .map(|c| c.key().to_string())
        .collect();
    if let Some(i) = keys.iter().position(|k| k == col.key()) {
        keys.remove(i);
    } else {
        keys.push(col.key().to_string());
    }
    if keys.is_empty() {
        keys.push("-".into());
    }
    settings.process_columns = keys;
}

impl ProcessesPage {
    pub fn select_pid(&mut self, pid: i32) {
        self.pending_pid = Some(pid);
    }

    fn build_groups<'a>(&self, ctx: &PageCtx<'a>) -> Vec<Group<'a>> {
        let snap = ctx.snap;
        let mut index: HashMap<&str, usize> = HashMap::new();
        let mut groups: Vec<Group<'a>> = Vec::new();
        for p in &snap.procs {
            if p.class == Class::Kernel {
                continue;
            }
            let i = *index.entry(&p.group).or_insert_with(|| {
                groups.push(Group {
                    key: p.group.clone(),
                    class: p.class,
                    name: String::new(),
                    icon: None,
                    procs: Vec::new(),
                    main: p,
                    cpu: 0.0,
                    mem: 0,
                    disk: 0.0,
                    gpu: 0.0,
                    gpu_mem: 0,
                    engine: None,
                    suspended: true,
                    efficiency: true,
                });
                groups.len() - 1
            });
            groups[i].procs.push(p);
        }
        for g in &mut groups {
            let pids: HashSet<i32> = g.procs.iter().map(|p| p.pid).collect();
            g.main = g
                .procs
                .iter()
                .filter(|p| !pids.contains(&p.ppid))
                .min_by_key(|p| p.pid)
                .copied()
                .unwrap_or(g.procs[0]);
            let base = g
                .main
                .app_name
                .as_deref()
                .unwrap_or(&g.main.name)
                .to_string();
            g.name = base;
            g.icon = g
                .main
                .icon
                .as_deref()
                .or_else(|| g.procs.iter().find_map(|p| p.icon.as_deref()));
            let mut best_gpu = -1.0;
            for p in &g.procs {
                g.cpu += p.cpu;
                g.mem += p.mem;
                g.disk += p.disk_read + p.disk_write;
                g.gpu += p.gpu;
                g.gpu_mem += p.gpu_mem;
                if p.gpu > best_gpu && p.gpu_engine.is_some() {
                    best_gpu = p.gpu;
                    g.engine = p.gpu_engine.as_deref();
                }
                let (st, eff) = status_of(p);
                g.suspended &= st == "Suspended";
                g.efficiency &= eff;
            }
            g.gpu = g.gpu.min(100.0);
            // A process in any group may be the one the class is decided by; an
            // app group is an app as soon as its main process is.
            g.class = g.procs.iter().map(|p| p.class).min().unwrap_or(g.class);
        }

        // Search: keep groups whose name matches, otherwise only matching processes.
        if !ctx.search.is_empty() {
            groups.retain_mut(|g| {
                if ctx.matches(&g.name) {
                    return true;
                }
                g.procs.retain(|p| {
                    ctx.matches(&p.name)
                        || ctx.matches(&p.exe_name)
                        || ctx.matches(&p.pid.to_string())
                });
                !g.procs.is_empty()
            });
        }
        groups
    }

    fn sort_groups(&self, groups: &mut [Group]) {
        let (col, asc) = self.sort.unwrap_or((Col::Name, true));
        let cmp_f = |a: f64, b: f64| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
        groups.sort_by(|a, b| {
            let o = match col {
                Col::Cpu => cmp_f(a.cpu as f64, b.cpu as f64),
                Col::Mem => a.mem.cmp(&b.mem),
                Col::Disk => cmp_f(a.disk, b.disk),
                Col::Gpu => cmp_f(a.gpu as f64, b.gpu as f64),
                Col::GpuMem => a.gpu_mem.cmp(&b.gpu_mem),
                Col::Pid => a.main.pid.cmp(&b.main.pid),
                Col::User => a.main.user.cmp(&b.main.user),
                Col::GpuEngine => a.engine.cmp(&b.engine),
                Col::Cmd => a.main.cmdline.cmp(&b.main.cmdline),
                Col::Status => a
                    .suspended
                    .cmp(&b.suspended)
                    .then(a.efficiency.cmp(&b.efficiency)),
                Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            };
            let o = if asc { o } else { o.reverse() };
            o.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        for g in groups.iter_mut() {
            g.procs.sort_by(|a, b| {
                let o = match col {
                    Col::Cpu => cmp_f(a.cpu as f64, b.cpu as f64),
                    Col::Mem => a.mem.cmp(&b.mem),
                    Col::Disk => cmp_f(a.disk_read + a.disk_write, b.disk_read + b.disk_write),
                    Col::Gpu => cmp_f(a.gpu as f64, b.gpu as f64),
                    Col::GpuMem => a.gpu_mem.cmp(&b.gpu_mem),
                    Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    _ => a.pid.cmp(&b.pid),
                };
                if asc { o } else { o.reverse() }
            });
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, ctx: &mut PageCtx) {
        let pal = ctx.pal;
        let mut groups = self.build_groups(ctx);
        self.sort_groups(&mut groups);

        if let Some(pid) = self.pending_pid.take()
            && let Some(g) = groups.iter().find(|g| g.procs.iter().any(|p| p.pid == pid))
        {
            if g.procs.len() > 1 {
                self.expanded.insert(g.key.clone());
                self.selected = Some(Sel::Pid(pid));
            } else {
                self.selected = Some(Sel::Group(g.key.clone()));
            }
            self.scroll_to_selected = true;
        }

        // Rows.
        let mut rows = Vec::new();
        let push_group = |rows: &mut Vec<Row>, i: usize| {
            rows.push(Row::Group(i));
            let g = &groups[i];
            if g.procs.len() > 1 && (self.expanded.contains(&g.key) || !ctx.search.is_empty()) {
                rows.extend((0..g.procs.len()).map(|j| Row::Child(i, j)));
            }
        };
        if ctx.settings.group_by_type {
            for class in [Class::App, Class::Background, Class::System] {
                let idx: Vec<usize> = (0..groups.len())
                    .filter(|&i| groups[i].class == class)
                    .collect();
                if idx.is_empty() {
                    continue;
                }
                rows.push(Row::Section(class, idx.len()));
                for i in idx {
                    push_group(&mut rows, i);
                }
            }
        } else {
            for i in 0..groups.len() {
                push_group(&mut rows, i);
            }
        }

        // Selected processes for the command bar.
        let selected_pids: Vec<i32> = match &self.selected {
            Some(Sel::Group(k)) => groups
                .iter()
                .find(|g| &g.key == k)
                .map(|g| g.procs.iter().map(|p| p.pid).collect())
                .unwrap_or_default(),
            Some(Sel::Pid(pid)) => {
                if ctx.snap.procs.iter().any(|p| p.pid == *pid) {
                    vec![*pid]
                } else {
                    Default::default()
                }
            }
            None => vec![],
        };
        let sel_efficiency = !selected_pids.is_empty()
            && selected_pids
                .iter()
                .all(|pid| ctx.snap.procs.iter().any(|p| p.pid == *pid && p.nice >= 19));

        let mut intents: Vec<Intent> = Vec::new();
        page_header(ui, &pal, "Processes", |ui| {
            let view = command_button(ui, &pal, ph::EYE, "View", true);
            egui::Popup::menu(&view).show(|ui| {
                ui.set_min_width(200.0);
                ui.checkbox(&mut ctx.settings.group_by_type, "Group by type");
                ui.checkbox(&mut ctx.settings.heatmap, "Heat map colors");
                ui.separator();
                ui.menu_button("Columns", |ui| {
                    for col in OPTIONAL {
                        let mut on = visible_optional(ctx.settings).contains(&col);
                        if ui.checkbox(&mut on, col.title()).changed() {
                            toggle_column(ctx.settings, col);
                        }
                    }
                });
                ui.separator();
                if ui.button("Expand all").clicked() {
                    self.expanded = groups.iter().map(|g| g.key.clone()).collect();
                }
                if ui.button("Collapse all").clicked() {
                    self.expanded.clear();
                }
            });
            ui.separator();
            let has_sel = !selected_pids.is_empty();
            if command_toggle(
                ui,
                &pal,
                ph::LEAF,
                "Efficiency mode",
                sel_efficiency,
                has_sel,
            )
            .clicked()
            {
                intents.push(Intent::Efficiency(selected_pids.clone(), !sel_efficiency));
            }
            if command_button(ui, &pal, ph::PROHIBIT, "End task", has_sel).clicked() {
                intents.push(Intent::End(selected_pids.clone(), libc::SIGTERM));
            }
            if command_button(ui, &pal, ph::APP_WINDOW, "Run new task", true).clicked() {
                ctx.dialogs.open_run();
            }
        });

        if ui.input(|i| i.key_pressed(egui::Key::Delete))
            && !ui.ctx().egui_wants_keyboard_input()
            && !selected_pids.is_empty()
        {
            intents.push(Intent::End(selected_pids.clone(), libc::SIGTERM));
        }

        let mut cols = vec![Col::Name];
        let optional = visible_optional(ctx.settings);
        if optional.contains(&Col::Status) {
            cols.push(Col::Status);
        }
        if optional.contains(&Col::Pid) {
            cols.push(Col::Pid);
        }
        if optional.contains(&Col::User) {
            cols.push(Col::User);
        }
        cols.extend([Col::Cpu, Col::Mem, Col::Disk, Col::Gpu]);
        for c in [Col::GpuEngine, Col::GpuMem, Col::Cmd] {
            if optional.contains(&c) {
                cols.push(c);
            }
        }

        let snap = ctx.snap;
        let totals: HashMap<&str, String> = [
            ("CPU", format!("{:.0}%", snap.cpu.usage)),
            ("Memory", format!("{:.0}%", snap.mem.usage)),
            ("Disk", format!("{:.0}%", snap.disk_total)),
            ("GPU", format!("{:.0}%", snap.gpu_total)),
        ]
        .into();
        let mem_total = snap.mem.total.max(1) as f32;
        let heat = ctx.settings.heatmap;
        let selected = self.selected.clone();
        let expanded = &self.expanded;
        let icons = ctx.icons;
        let sort = self.sort;
        let mut new_sort = None;

        let scroll_row = if self.scroll_to_selected {
            self.scroll_to_selected = false;
            rows.iter().position(|r| match (r, &selected) {
                (Row::Group(i), Some(Sel::Group(k))) => &groups[*i].key == k,
                (Row::Child(i, j), Some(Sel::Pid(pid))) => groups[*i].procs[*j].pid == *pid,
                _ => false,
            })
        } else {
            None
        };

        ui.scope(|ui| {
            ui.visuals_mut().selection.bg_fill = pal.selected;
            ui.visuals_mut().widgets.hovered.bg_fill = pal.hover;
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
            let mut table = TableBuilder::new(ui)
                .id_salt("processes-table")
                .striped(false)
                .resizable(true)
                .sense(Sense::click())
                .auto_shrink(false)
                .cell_layout(Layout::left_to_right(Align::Center));
            for (i, c) in cols.iter().enumerate() {
                let last = i == cols.len() - 1;
                table = table.column(match c {
                    Col::Name => Column::initial(300.0).at_least(160.0).clip(true),
                    Col::Status => Column::initial(110.0).at_least(40.0).clip(true),
                    Col::Cmd => Column::remainder().at_least(120.0).clip(true),
                    Col::GpuEngine => Column::initial(130.0).at_least(60.0).clip(true),
                    Col::User => Column::initial(100.0).at_least(50.0).clip(true),
                    _ if last => Column::remainder().at_least(70.0).clip(true),
                    _ => Column::initial(84.0).at_least(56.0).clip(true),
                });
            }
            if let Some(r) = scroll_row {
                table = table.scroll_to_row(r, Some(Align::Center));
            }
            table
                .header(46.0, |mut header| {
                    for c in &cols {
                        header.col(|ui| {
                            let total = totals.get(c.title()).map(String::as_str);
                            let sorted = match sort {
                                Some((sc, asc)) if sc == *c => Some(asc),
                                _ => None,
                            };
                            let align = if c.numeric() {
                                HeaderAlign::Right
                            } else {
                                HeaderAlign::Left
                            };
                            if header_cell(ui, &pal, c.title(), total, sorted, align).clicked() {
                                new_sort = Some(match sort {
                                    Some((sc, asc)) if sc == *c => (*c, !asc),
                                    _ => (*c, !c.numeric()),
                                });
                            }
                        });
                    }
                })
                .body(|body| {
                    body.rows(30.0, rows.len(), |mut row| {
                        let r = &rows[row.index()];
                        match r {
                            Row::Section(class, n) => {
                                for (ci, _) in cols.iter().enumerate() {
                                    row.col(|ui| {
                                        if ci == 0 {
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} ({n})",
                                                    class_title(*class)
                                                ))
                                                .font(semibold(13.5))
                                                .color(pal.accent),
                                            );
                                        }
                                    });
                                }
                            }
                            Row::Group(gi) => {
                                let g = &groups[*gi];
                                let is_sel = selected == Some(Sel::Group(g.key.clone()));
                                row.set_selected(is_sel);
                                let multi = g.procs.len() > 1;
                                let open =
                                    expanded.contains(&g.key) || (!ctx.search.is_empty() && multi);
                                for c in &cols {
                                    row.col(|ui| {
                                        let p = g.main;
                                        match c {
                                            Col::Name => {
                                                if multi {
                                                    let chev = if open {
                                                        ph::CARET_DOWN
                                                    } else {
                                                        ph::CARET_RIGHT
                                                    };
                                                    let r = ui.add(
                                                        egui::Label::new(
                                                            RichText::new(chev)
                                                                .color(pal.text_weak),
                                                        )
                                                        .sense(Sense::click()),
                                                    );
                                                    if r.clicked() {
                                                        intents.push(Intent::Toggle(g.key.clone()));
                                                    }
                                                } else {
                                                    ui.add_space(14.0);
                                                }
                                                let fallback = if g.class == Class::App {
                                                    ph::APP_WINDOW
                                                } else {
                                                    ph::GEAR_SIX
                                                };
                                                app_icon(ui, &pal, icons, g.icon, fallback, 16.0);
                                                let label = if multi {
                                                    format!("{} ({})", g.name, g.procs.len())
                                                } else {
                                                    g.name.clone()
                                                };
                                                ui.add(egui::Label::new(label).truncate());
                                            }
                                            Col::Status => {
                                                status_cell(ui, &pal, g.suspended, g.efficiency)
                                            }
                                            Col::Pid => {
                                                if !multi {
                                                    right_text(ui, p.pid.to_string());
                                                }
                                            }
                                            Col::User => {
                                                ui.add(
                                                    egui::Label::new(p.user.as_ref()).truncate(),
                                                );
                                            }
                                            Col::Cpu => num_cell(
                                                ui,
                                                &pal,
                                                heat && !is_sel,
                                                (g.cpu / 100.0).sqrt(),
                                                util::pct1(g.cpu),
                                            ),
                                            Col::Mem => num_cell(
                                                ui,
                                                &pal,
                                                heat && !is_sel,
                                                (g.mem as f32 / mem_total).sqrt(),
                                                util::mem_mb(g.mem),
                                            ),
                                            Col::Disk => num_cell(
                                                ui,
                                                &pal,
                                                heat && !is_sel,
                                                (g.disk as f32 / 100e6).sqrt(),
                                                util::disk_rate_mb(g.disk),
                                            ),
                                            Col::Gpu => num_cell(
                                                ui,
                                                &pal,
                                                heat && !is_sel,
                                                (g.gpu / 100.0).sqrt(),
                                                util::pct1(g.gpu),
                                            ),
                                            Col::GpuMem => num_cell(
                                                ui,
                                                &pal,
                                                false,
                                                0.0,
                                                if g.gpu_mem > 0 {
                                                    util::mem_mb(g.gpu_mem)
                                                } else {
                                                    String::new()
                                                },
                                            ),
                                            Col::GpuEngine => {
                                                if let Some(e) = g.engine {
                                                    ui.add(egui::Label::new(e).truncate());
                                                }
                                            }
                                            Col::Cmd => {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(p.cmdline.as_ref())
                                                            .color(pal.text_weak),
                                                    )
                                                    .truncate(),
                                                );
                                            }
                                        }
                                    });
                                }
                                let resp = row.response();
                                if resp.clicked() || resp.secondary_clicked() {
                                    intents.push(Intent::Select(Sel::Group(g.key.clone())));
                                }
                                if resp.double_clicked() && multi {
                                    intents.push(Intent::Toggle(g.key.clone()));
                                }
                                let pids: Vec<i32> = g.procs.iter().map(|p| p.pid).collect();
                                resp.context_menu(|ui| {
                                    context_menu(
                                        ui,
                                        &mut intents,
                                        &pids,
                                        g.main,
                                        multi.then_some((&g.key, open)),
                                        g.efficiency,
                                    );
                                });
                            }
                            Row::Child(gi, pi) => {
                                let g = &groups[*gi];
                                let p = g.procs[*pi];
                                let is_sel = selected == Some(Sel::Pid(p.pid));
                                row.set_selected(is_sel);
                                let (st, eff) = status_of(p);
                                for c in &cols {
                                    row.col(|ui| match c {
                                        Col::Name => {
                                            ui.add_space(34.0);
                                            app_icon(
                                                ui,
                                                &pal,
                                                icons,
                                                p.icon.as_deref().or(g.icon),
                                                ph::GEAR_SIX,
                                                16.0,
                                            );
                                            ui.add(egui::Label::new(p.name.as_ref()).truncate());
                                        }
                                        Col::Status => {
                                            status_cell(ui, &pal, st == "Suspended", eff)
                                        }
                                        Col::Pid => right_text(ui, p.pid.to_string()),
                                        Col::User => {
                                            ui.add(egui::Label::new(p.user.as_ref()).truncate());
                                        }
                                        Col::Cpu => num_cell(
                                            ui,
                                            &pal,
                                            heat && !is_sel,
                                            (p.cpu / 100.0).sqrt(),
                                            util::pct1(p.cpu),
                                        ),
                                        Col::Mem => num_cell(
                                            ui,
                                            &pal,
                                            heat && !is_sel,
                                            (p.mem as f32 / mem_total).sqrt(),
                                            util::mem_mb(p.mem),
                                        ),
                                        Col::Disk => {
                                            let d = p.disk_read + p.disk_write;
                                            num_cell(
                                                ui,
                                                &pal,
                                                heat && !is_sel,
                                                (d as f32 / 100e6).sqrt(),
                                                util::disk_rate_mb(d),
                                            )
                                        }
                                        Col::Gpu => num_cell(
                                            ui,
                                            &pal,
                                            heat && !is_sel,
                                            (p.gpu / 100.0).sqrt(),
                                            util::pct1(p.gpu),
                                        ),
                                        Col::GpuMem => num_cell(
                                            ui,
                                            &pal,
                                            false,
                                            0.0,
                                            if p.gpu_mem > 0 {
                                                util::mem_mb(p.gpu_mem)
                                            } else {
                                                String::new()
                                            },
                                        ),
                                        Col::GpuEngine => {
                                            if let Some(e) = &p.gpu_engine {
                                                ui.add(egui::Label::new(e.as_ref()).truncate());
                                            }
                                        }
                                        Col::Cmd => {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(p.cmdline.as_ref())
                                                        .color(pal.text_weak),
                                                )
                                                .truncate(),
                                            );
                                        }
                                    });
                                }
                                let resp = row.response();
                                if resp.clicked() || resp.secondary_clicked() {
                                    intents.push(Intent::Select(Sel::Pid(p.pid)));
                                }
                                resp.context_menu(|ui| {
                                    context_menu(ui, &mut intents, &[p.pid], p, None, eff);
                                });
                            }
                        }
                    });
                });
        });

        if let Some(s) = new_sort {
            self.sort = Some(s);
        }
        for intent in intents {
            match intent {
                Intent::Select(s) => self.selected = Some(s),
                Intent::Toggle(k) => {
                    if !self.expanded.remove(&k) {
                        self.expanded.insert(k);
                    }
                }
                Intent::End(pids, sig) => {
                    let procs: Vec<&Proc> = snap
                        .procs
                        .iter()
                        .filter(|p| pids.contains(&p.pid))
                        .collect();
                    ctx.dialogs.end_task(&procs, snap.my_uid, sig, ctx.actions);
                }
                Intent::Efficiency(pids, on) => {
                    ctx.dialogs.efficiency(pids, on, ctx.settings, ctx.actions)
                }
                Intent::Signal(pids, sig) => ctx.actions.signal(pids, sig),
                Intent::Details(pid) => *ctx.nav = Some(super::Nav::Details(pid)),
                Intent::OpenLocation(path) => ctx.actions.open_location(path),
                Intent::Search(q) => ctx.actions.search_online(&q),
                Intent::Properties(pid) => ctx.dialogs.open_properties(pid),
                Intent::Copy(text) => ui.ctx().copy_text(text),
            }
        }
    }
}

fn status_cell(ui: &mut Ui, pal: &crate::theme::Palette, suspended: bool, efficiency: bool) {
    if suspended {
        ui.label(RichText::new(format!("{}  Suspended", ph::PAUSE)).color(pal.text_weak));
    } else if efficiency {
        ui.label(RichText::new(ph::LEAF).color(pal.disk))
            .on_hover_text("This process is in Efficiency mode");
    }
}

fn num_cell(ui: &mut Ui, pal: &crate::theme::Palette, heat: bool, frac: f32, text: String) {
    if heat {
        heat_bg(ui, pal, frac);
    }
    right_text(ui, text);
}

fn context_menu(
    ui: &mut Ui,
    intents: &mut Vec<Intent>,
    pids: &[i32],
    main: &Proc,
    group: Option<(&Arc<str>, bool)>,
    efficiency: bool,
) {
    ui.set_min_width(210.0);
    if let Some((key, open)) = group {
        if ui
            .button(if open { "Collapse" } else { "Expand" })
            .clicked()
        {
            intents.push(Intent::Toggle(key.clone()));
            ui.close();
        }
        ui.separator();
    }
    if ui.button(format!("{}  End task", ph::PROHIBIT)).clicked() {
        intents.push(Intent::End(pids.to_vec(), libc::SIGTERM));
        ui.close();
    }
    if ui
        .button(format!("{}  Force end task", ph::SKULL))
        .clicked()
    {
        intents.push(Intent::End(pids.to_vec(), libc::SIGKILL));
        ui.close();
    }
    let suspended = main.state == 'T' || main.state == 't';
    if suspended {
        if ui.button(format!("{}  Resume", ph::PLAY)).clicked() {
            intents.push(Intent::Signal(pids.to_vec(), libc::SIGCONT));
            ui.close();
        }
    } else if ui.button(format!("{}  Suspend", ph::PAUSE)).clicked() {
        intents.push(Intent::Signal(pids.to_vec(), libc::SIGSTOP));
        ui.close();
    }
    let mut eff = efficiency;
    if ui.checkbox(&mut eff, "Efficiency mode").clicked() {
        intents.push(Intent::Efficiency(pids.to_vec(), eff));
        ui.close();
    }
    ui.separator();
    if ui
        .button(format!("{}  Go to details", ph::LIST_BULLETS))
        .clicked()
    {
        intents.push(Intent::Details(main.pid));
        ui.close();
    }
    if let Some(path) = &main.exe_path
        && ui
            .button(format!("{}  Open file location", ph::FOLDER_OPEN))
            .clicked()
    {
        intents.push(Intent::OpenLocation(path.to_string()));
        ui.close();
    }
    if ui.button(format!("{}  Search online", ph::GLOBE)).clicked() {
        intents.push(Intent::Search(main.exe_name.to_string()));
        ui.close();
    }
    if ui.button(format!("{}  Copy", ph::COPY)).clicked() {
        intents.push(Intent::Copy(format!(
            "{}\t{}\tCPU {:.1}%\tMemory {}",
            main.app_name.as_deref().unwrap_or(&main.name),
            main.pid,
            main.cpu,
            util::mem_mb(main.mem)
        )));
        ui.close();
    }
    if ui.button(format!("{}  Properties", ph::INFO)).clicked() {
        intents.push(Intent::Properties(main.pid));
        ui.close();
    }
}
