//! The Performance tab: device list with live thumbnails and detail graphs.

use super::PageCtx;
use super::widgets::{Line, command_button, labeled_graph, page_header, paint_graph, stat};
use crate::settings::CpuGraph;
use crate::sys::{Series, Snapshot};
use crate::theme::{Palette, blend, regular, semibold};
use crate::util;
use egui::{Align, Align2, Color32, Frame, Layout, Rect, RichText, Sense, Ui, pos2, vec2};
use egui_phosphor::regular as ph;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Item {
    #[default]
    Cpu,
    Memory,
    Disk(usize),
    Net(usize),
    Gpu(usize),
}

#[derive(Default)]
pub struct PerformancePage {
    item: Item,
}

struct Thumb<'a> {
    item: Item,
    title: String,
    lines: Vec<String>,
    data: &'a Series,
    max: f32,
    color: Color32,
}

impl Item {
    /// Parse "cpu", "memory", "disk1", "net0", "gpu" etc.
    pub fn from_arg(s: &str) -> Option<Item> {
        let s = s.to_lowercase();
        let num = |p: &str| s.strip_prefix(p).map(|n| n.parse().unwrap_or(0));
        Some(match s.as_str() {
            "cpu" => Item::Cpu,
            "memory" | "mem" => Item::Memory,
            _ => {
                if let Some(n) = num("disk") {
                    Item::Disk(n)
                } else if let Some(n) = num("net") {
                    Item::Net(n)
                } else {
                    let n = num("gpu")?;
                    Item::Gpu(n)
                }
            }
        })
    }
}

impl PerformancePage {
    pub fn show_item(&mut self, item: Item) {
        self.item = item;
    }

    pub fn ui(&mut self, ui: &mut Ui, ctx: &mut PageCtx) {
        let pal = ctx.pal;
        let snap = ctx.snap;
        let mut copy = false;
        let secs = ctx.settings.speed.graph_seconds();
        page_header(ui, &pal, "Performance", |ui| {
            let more = command_button(ui, &pal, ph::DOTS_THREE, "", true);
            egui::Popup::menu(&more).show(|ui| {
                if ui.button(format!("{}  Copy", ph::COPY)).clicked() {
                    copy = true;
                    ui.close();
                }
            });
            if command_button(ui, &pal, ph::APP_WINDOW, "Run new task", true).clicked() {
                ctx.dialogs.open_run();
            }
        });

        let thumbs = thumbs(snap, &pal);
        // Fall back to CPU if the device went away (but not before the first sample arrives).
        if snap.seq > 1 && !thumbs.iter().any(|t| t.item == self.item) {
            self.item = Item::Cpu;
        }

        egui::Panel::left("perf-list")
            .frame(Frame::NONE.inner_margin(egui::Margin {
                left: 0,
                right: 10,
                top: 0,
                bottom: 0,
            }))
            .resizable(true)
            .default_size(250.0)
            .size_range(180.0..=380.0)
            .show_separator_line(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for t in &thumbs {
                            if thumb(ui, &pal, t, self.item == t.item, snap.seq) {
                                self.item = t.item;
                            }
                        }
                    });
            });

        egui::CentralPanel::no_frame().show(ui, |ui| {
            ui.add_space(2.0);
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width() - 6.0);
                    ui.add_space(0.0);
                    let text = match self.item {
                        Item::Cpu => cpu(ui, ctx),
                        Item::Memory => memory(ui, &pal, snap, secs),
                        Item::Disk(i) => disk(ui, &pal, snap, i, secs),
                        Item::Net(i) => net(ui, &pal, snap, i, secs),
                        Item::Gpu(i) => gpu(ui, &pal, snap, i),
                    };
                    if copy {
                        ui.ctx().copy_text(text);
                    }
                });
        });
    }
}

fn thumbs<'a>(snap: &'a Snapshot, pal: &Palette) -> Vec<Thumb<'a>> {
    let mut v = vec![
        Thumb {
            item: Item::Cpu,
            title: "CPU".into(),
            lines: vec![format!(
                "{:.0}%  {}",
                snap.cpu.usage,
                util::ghz(snap.cpu.speed_mhz)
            )],
            data: &snap.cpu.history,
            max: 100.0,
            color: pal.cpu,
        },
        Thumb {
            item: Item::Memory,
            title: "Memory".into(),
            lines: vec![format!(
                "{:.1}/{:.1} GB ({:.0}%)",
                snap.mem.used as f64 / 1073741824.0,
                snap.mem.total as f64 / 1073741824.0,
                snap.mem.usage
            )],
            data: &snap.mem.history,
            max: 100.0,
            color: pal.mem,
        },
    ];
    for (i, d) in snap.disks.iter().enumerate() {
        v.push(Thumb {
            item: Item::Disk(i),
            title: format!("Disk {} ({})", d.info.index, d.info.name),
            lines: vec![d.info.kind.clone(), format!("{:.0}%", d.active)],
            data: &d.active_history,
            max: 100.0,
            color: pal.disk,
        });
    }
    for (i, n) in snap.nets.iter().enumerate() {
        let max = util::nice_ceiling(n.send_history.max().max(n.recv_history.max()) as f64) as f32;
        v.push(Thumb {
            item: Item::Net(i),
            title: n.info.kind.clone(),
            lines: vec![
                n.info.iface.clone(),
                format!(
                    "S: {}  R: {}",
                    short_bits(n.send_bps),
                    short_bits(n.recv_bps)
                ),
            ],
            data: &n.recv_history,
            max: max.max(1000.0),
            color: pal.net,
        });
    }
    for (i, g) in snap.gpus.iter().enumerate() {
        let temp = g
            .temp_c
            .map(|t| format!(" ({t:.0} °C)"))
            .unwrap_or_default();
        v.push(Thumb {
            item: Item::Gpu(i),
            title: format!("GPU {}", g.info.index),
            lines: vec![g.info.name.clone(), format!("{:.0}%{temp}", g.util)],
            data: &g.history,
            max: 100.0,
            color: pal.gpu,
        });
    }
    v
}

fn short_bits(b: f64) -> String {
    if b < 1.0 { "0".into() } else { util::bits(b) }
}

fn thumb(ui: &mut Ui, pal: &Palette, t: &Thumb, selected: bool, phase: u64) -> bool {
    let h = 18.0 + 16.0 * t.lines.len() as f32 + 14.0;
    let h = h.max(62.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 5, pal.selected);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 5, pal.hover);
    }
    let g = Rect::from_min_size(
        pos2(rect.left() + 8.0, rect.center().y - 24.0),
        vec2(72.0, 48.0),
    );
    let painter = ui.painter_at(rect);
    paint_graph(
        &painter,
        g,
        pal,
        &[Line::filled(t.data, t.color)],
        t.max,
        t.color,
        phase,
        false,
    );
    let x = g.right() + 12.0;
    let mut y = rect.top() + (h - (18.0 + 16.0 * t.lines.len() as f32)) / 2.0;
    painter.text(
        pos2(x, y),
        Align2::LEFT_TOP,
        &t.title,
        regular(14.0),
        pal.text,
    );
    y += 19.0;
    for l in &t.lines {
        let mut job =
            egui::text::LayoutJob::simple_singleline(l.clone(), regular(12.0), pal.text_weak);
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.right() - x - 6.0);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        painter.galley(pos2(x, y), galley, pal.text_weak);
        y += 16.0;
    }
    resp.clicked()
}

fn title_row(ui: &mut Ui, pal: &Palette, title: &str, right: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(semibold(26.0)).color(pal.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(
                egui::Label::new(RichText::new(right).font(regular(15.0)).color(pal.text))
                    .truncate(),
            );
        });
    });
    ui.add_space(6.0);
}

/// Two-column key/value list shown at the bottom right of each page.
fn info_grid(ui: &mut Ui, pal: &Palette, id: &str, rows: &[(&str, String)]) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([28.0, 3.0])
        .show(ui, |ui| {
            for (k, v) in rows {
                ui.label(RichText::new(*k).font(regular(12.0)).color(pal.text_weak));
                ui.label(RichText::new(v).font(regular(12.5)));
                ui.end_row();
            }
        });
}

/// Left statistics block (big numbers) + right info grid, side by side.
fn stats_area(ui: &mut Ui, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    ui.add_space(8.0);
    let w = ui.available_width();
    if w < 620.0 {
        left(ui);
        ui.add_space(10.0);
        right(ui);
        return;
    }
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(w * 0.5, 0.0), Layout::top_down(Align::Min), left);
        ui.add_space(16.0);
        ui.allocate_ui_with_layout(vec2(w * 0.48, 0.0), Layout::top_down(Align::Min), right);
    });
}

fn stat_row(ui: &mut Ui, pal: &Palette, items: &[(&str, String)]) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 18.0;
        for (label, value) in items {
            ui.vertical(|ui| {
                ui.set_min_width(96.0);
                stat(ui, pal, label, value);
            });
        }
    });
    ui.add_space(6.0);
}

fn legend_stat(ui: &mut Ui, pal: &Palette, color: Color32, dashed: bool, label: &str, value: &str) {
    ui.allocate_ui_with_layout(vec2(140.0, 46.0), Layout::top_down(Align::Min), |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(4.0, 30.0), Sense::hover());
            if dashed {
                for k in 0..5 {
                    let y = r.top() + k as f32 * 6.5;
                    ui.painter()
                        .vline(r.center().x, y..=y + 4.0, egui::Stroke::new(2.0, color));
                }
            } else {
                ui.painter()
                    .vline(r.center().x, r.y_range(), egui::Stroke::new(2.0, color));
            }
            stat(ui, pal, label, value);
        });
    });
}

fn graph_height(ui: &Ui, reserve: f32, min: f32) -> f32 {
    (ui.available_height() - reserve).max(min)
}

fn cpu(ui: &mut Ui, ctx: &mut PageCtx) -> String {
    let pal = ctx.pal;
    let snap = ctx.snap;
    let c = &snap.cpu;
    let secs = ctx.settings.speed.graph_seconds();
    title_row(ui, &pal, "CPU", &c.info.model);
    let h = graph_height(ui, 240.0, 160.0);
    let w = ui.available_width();
    let kernel_line;
    let resp = match ctx.settings.cpu_graph {
        CpuGraph::Overall => {
            let mut lines = vec![Line::filled(&c.history, pal.cpu)];
            if ctx.settings.show_kernel_times {
                kernel_line = Line::filled(&c.kernel_history, blend(pal.cpu, Color32::BLACK, 0.35));
                lines.push(kernel_line);
            }
            labeled_graph(
                ui,
                &pal,
                vec2(w, h),
                &format!("% Utilization over {secs} seconds"),
                "100%",
                &format!("{secs} seconds"),
                &lines,
                100.0,
                pal.cpu,
                snap.seq,
            )
        }
        CpuGraph::Logical => {
            let n = c.per_cpu_history.len().max(1);
            let cols = ((n as f32 * w / h.max(1.0)).sqrt().ceil() as usize).clamp(1, n);
            let rows = n.div_ceil(cols);
            let gap = 4.0;
            let cw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
            let ch = ((h + 32.0 - gap * (rows as f32 - 1.0)) / rows as f32).max(24.0);
            ui.label(
                RichText::new(format!("% Utilization over {secs} seconds"))
                    .font(regular(11.0))
                    .color(pal.text_weak),
            );
            let (rect, resp) =
                ui.allocate_exact_size(vec2(w, rows as f32 * (ch + gap)), Sense::click());
            let painter = ui.painter_at(rect);
            for (i, hist) in c.per_cpu_history.iter().enumerate() {
                let (r, col) = (i / cols, i % cols);
                let cell = Rect::from_min_size(
                    pos2(
                        rect.left() + col as f32 * (cw + gap),
                        rect.top() + r as f32 * (ch + gap),
                    ),
                    vec2(cw, ch),
                );
                paint_graph(
                    &painter,
                    cell,
                    &pal,
                    &[Line::filled(hist, pal.cpu)],
                    100.0,
                    pal.cpu,
                    snap.seq,
                    true,
                );
            }
            resp
        }
    };
    resp.context_menu(|ui| {
        ui.menu_button("Change graph to", |ui| {
            if ui
                .radio(
                    ctx.settings.cpu_graph == CpuGraph::Overall,
                    "Overall utilization",
                )
                .clicked()
            {
                ctx.settings.cpu_graph = CpuGraph::Overall;
                ui.close();
            }
            if ui
                .radio(
                    ctx.settings.cpu_graph == CpuGraph::Logical,
                    "Logical processors",
                )
                .clicked()
            {
                ctx.settings.cpu_graph = CpuGraph::Logical;
                ui.close();
            }
        });
        if ui
            .checkbox(&mut ctx.settings.show_kernel_times, "Show kernel times")
            .clicked()
        {
            ui.close();
        }
    });

    let info = &c.info;
    let rows = [
        (
            "Base speed:",
            info.base_mhz.map(util::ghz).unwrap_or_else(|| "—".into()),
        ),
        ("Sockets:", info.sockets.to_string()),
        ("Cores:", info.cores.to_string()),
        ("Logical processors:", info.logical.to_string()),
        ("Virtualization:", info.virtualization.clone()),
        ("L1 cache:", util::bytes(info.l1)),
        ("L2 cache:", util::bytes(info.l2)),
        ("L3 cache:", util::bytes(info.l3)),
    ];
    stats_area(
        ui,
        |ui| {
            stat_row(
                ui,
                &pal,
                &[
                    ("Utilization", format!("{:.0}%", c.usage)),
                    ("Speed", util::ghz(c.speed_mhz)),
                ],
            );
            stat_row(
                ui,
                &pal,
                &[
                    ("Processes", c.processes.to_string()),
                    ("Threads", c.threads.to_string()),
                    ("Handles", util::group_thousands(c.handles)),
                ],
            );
            stat_row(ui, &pal, &[("Up time", util::uptime(c.uptime))]);
        },
        |ui| info_grid(ui, &pal, "cpu-info", &rows),
    );
    let mut text = format!(
        "CPU\n\t{}\n\tUtilization\t{:.0}%\n\tSpeed\t{}\n\tProcesses\t{}\n\tThreads\t{}\n\tHandles\t{}\n\tUp time\t{}\n",
        info.model,
        c.usage,
        util::ghz(c.speed_mhz),
        c.processes,
        c.threads,
        c.handles,
        util::uptime(c.uptime)
    );
    for (k, v) in rows {
        text.push_str(&format!("\t{k}\t{v}\n"));
    }
    text
}

fn memory(ui: &mut Ui, pal: &Palette, snap: &Snapshot, secs: u64) -> String {
    let m = &snap.mem;
    let gb = |b: u64| format!("{:.1} GB", b as f64 / 1073741824.0);
    let header_total = m.installed.unwrap_or(m.total);
    title_row(ui, pal, "Memory", &gb(header_total));
    let w = ui.available_width();
    let h = graph_height(ui, 300.0, 140.0);
    labeled_graph(
        ui,
        pal,
        vec2(w, h),
        "Memory usage",
        &gb(m.total),
        &format!("{secs} seconds"),
        &[Line::filled(&m.history, pal.mem)],
        100.0,
        pal.mem,
        snap.seq,
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new("Memory composition")
            .font(regular(11.0))
            .color(pal.text_weak),
    );
    composition_bar(ui, pal, m);

    let reserved = m.installed.map(|i| i.saturating_sub(m.total));
    let rows = [
        (
            "Installed:",
            m.installed.map(gb).unwrap_or_else(|| "—".into()),
        ),
        (
            "Hardware reserved:",
            reserved.map(util::bytes).unwrap_or_else(|| "—".into()),
        ),
        (
            "Swap:",
            format!(
                "{} / {}",
                util::bytes(m.swap_used),
                util::bytes(m.swap_total)
            ),
        ),
        ("Shared:", util::bytes(m.shared)),
        ("Buffers:", util::bytes(m.buffers)),
        (
            "Kernel (slab):",
            util::bytes(m.slab_reclaimable + m.slab_unreclaimable),
        ),
        ("Page tables:", util::bytes(m.page_tables)),
    ];
    stats_area(
        ui,
        |ui| {
            let in_use = if m.compressed > 0 {
                format!("{} ({})", gb(m.used), util::bytes(m.compressed))
            } else {
                gb(m.used)
            };
            stat_row(
                ui,
                pal,
                &[
                    ("In use (Compressed)", in_use),
                    ("Available", gb(m.available)),
                ],
            );
            stat_row(
                ui,
                pal,
                &[
                    (
                        "Committed",
                        format!(
                            "{:.1}/{:.1} GB",
                            m.committed as f64 / 1073741824.0,
                            m.commit_limit as f64 / 1073741824.0
                        ),
                    ),
                    ("Cached", gb(m.cached)),
                ],
            );
            stat_row(
                ui,
                pal,
                &[
                    ("Kernel (reclaimable)", util::bytes(m.slab_reclaimable)),
                    ("Kernel (unreclaimable)", util::bytes(m.slab_unreclaimable)),
                ],
            );
        },
        |ui| info_grid(ui, pal, "mem-info", &rows),
    );
    format!(
        "Memory\n\t{}\n\tIn use\t{}\n\tAvailable\t{}\n\tCommitted\t{}/{}\n\tCached\t{}\n\tSwap\t{}/{}\n",
        gb(header_total),
        gb(m.used),
        gb(m.available),
        util::bytes(m.committed),
        util::bytes(m.commit_limit),
        gb(m.cached),
        util::bytes(m.swap_used),
        util::bytes(m.swap_total)
    )
}

fn composition_bar(ui: &mut Ui, pal: &Palette, m: &crate::sys::memory::MemStats) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 44.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, pal.graph_bg);
    let total = m.total.max(1) as f32;
    let parts = [
        (
            "In use",
            m.used,
            pal.fill_of(pal.mem),
            "Memory used by processes, drivers and the kernel",
        ),
        (
            "Modified",
            m.modified,
            blend(pal.fill_of(pal.mem), pal.graph_bg, 0.35),
            "Memory whose contents must be written to disk before it can be used for another purpose",
        ),
        (
            "Standby",
            m.standby,
            blend(pal.fill_of(pal.mem), pal.graph_bg, 0.65),
            "Memory that contains cached data and code that is not actively in use",
        ),
        (
            "Free",
            m.free,
            pal.graph_bg,
            "Memory that is not currently in use",
        ),
    ];
    let mut x = rect.left();
    for (name, bytes, color, desc) in parts {
        let pw = bytes as f32 / total * rect.width();
        if pw <= 0.5 {
            continue;
        }
        let r = Rect::from_min_size(pos2(x, rect.top()), vec2(pw, rect.height()));
        painter.rect_filled(r, 0, color);
        if name != "In use" {
            painter.vline(r.left(), r.y_range(), egui::Stroke::new(1.0, pal.mem));
        }
        let id = ui.id().with(name);
        ui.interact(r, id, Sense::hover())
            .on_hover_text(format!("{name} ({})\n{desc}", util::bytes(bytes)));
        x += pw;
    }
    painter.rect_stroke(
        rect,
        0,
        egui::Stroke::new(1.0, pal.mem),
        egui::StrokeKind::Inside,
    );
}

fn disk(ui: &mut Ui, pal: &Palette, snap: &Snapshot, i: usize, secs: u64) -> String {
    let Some(d) = snap.disks.get(i) else {
        return String::new();
    };
    let info = &d.info;
    title_row(
        ui,
        pal,
        &format!("Disk {} ({})", info.index, info.name),
        &info.model,
    );
    let w = ui.available_width();
    let h = graph_height(ui, 260.0, 220.0);
    labeled_graph(
        ui,
        pal,
        vec2(w, h * 0.55),
        "Active time",
        "100%",
        &format!("{secs} seconds"),
        &[Line::filled(&d.active_history, pal.disk)],
        100.0,
        pal.disk,
        snap.seq,
    );
    ui.add_space(4.0);
    let max = util::nice_ceiling(d.read_history.max().max(d.write_history.max()) as f64)
        .max(1024.0 * 1024.0) as f32;
    labeled_graph(
        ui,
        pal,
        vec2(w, h * 0.45 - 30.0),
        "Disk transfer rate",
        &util::rate(max as f64),
        &format!("{secs} seconds"),
        &[
            Line::filled(&d.read_history, pal.disk),
            Line::dashed(&d.write_history, pal.disk),
        ],
        max,
        pal.disk,
        snap.seq,
    );
    let yes_no = |b: bool| if b { "Yes" } else { "No" }.to_string();
    let rows = [
        ("Capacity:", util::bytes(info.capacity)),
        (
            "Formatted:",
            if info.formatted > 0 {
                util::bytes(info.formatted)
            } else {
                "—".into()
            },
        ),
        ("System disk:", yes_no(info.system_disk)),
        ("Swap (page file):", yes_no(info.page_file)),
        ("Type:", info.kind.clone()),
        (
            "Mounted at:",
            if info.mounts.is_empty() {
                "—".into()
            } else {
                info.mounts.join("  ")
            },
        ),
    ];
    stats_area(
        ui,
        |ui| {
            stat_row(
                ui,
                pal,
                &[
                    ("Active time", format!("{:.0}%", d.active)),
                    (
                        "Average response time",
                        format!("{:.1} ms", d.avg_response_ms),
                    ),
                ],
            );
            ui.horizontal_top(|ui| {
                legend_stat(
                    ui,
                    pal,
                    pal.disk,
                    false,
                    "Read speed",
                    &util::rate(d.read_bps),
                );
                legend_stat(
                    ui,
                    pal,
                    pal.disk,
                    true,
                    "Write speed",
                    &util::rate(d.write_bps),
                );
            });
        },
        |ui| info_grid(ui, pal, "disk-info", &rows),
    );
    format!(
        "Disk {} ({})\n\t{}\n\tCapacity\t{}\n\tActive time\t{:.0}%\n\tRead speed\t{}\n\tWrite speed\t{}\n\tType\t{}\n",
        info.index,
        info.name,
        info.model,
        util::bytes(info.capacity),
        d.active,
        util::rate(d.read_bps),
        util::rate(d.write_bps),
        info.kind
    )
}

fn net(ui: &mut Ui, pal: &Palette, snap: &Snapshot, i: usize, secs: u64) -> String {
    let Some(n) = snap.nets.get(i) else {
        return String::new();
    };
    let info = &n.info;
    title_row(ui, pal, &info.kind, &info.adapter);
    let w = ui.available_width();
    let h = graph_height(ui, 230.0, 160.0);
    let max = util::nice_ceiling(n.send_history.max().max(n.recv_history.max()) as f64)
        .max(100_000.0) as f32;
    labeled_graph(
        ui,
        pal,
        vec2(w, h),
        "Throughput",
        &util::bits(max as f64),
        &format!("{secs} seconds"),
        &[
            Line::filled(&n.recv_history, pal.net),
            Line::dashed(&n.send_history, pal.net),
        ],
        max,
        pal.net,
        snap.seq,
    );
    let mut rows = vec![
        ("Adapter name:", info.adapter.clone()),
        ("Interface:", info.iface.clone()),
    ];
    if let Some(ssid) = &info.ssid {
        rows.push(("SSID:", ssid.clone()));
    }
    rows.push(("Connection type:", info.kind.clone()));
    rows.push((
        "IPv4 address:",
        info.ipv4.first().cloned().unwrap_or_else(|| "—".into()),
    ));
    rows.push((
        "IPv6 address:",
        info.ipv6.first().cloned().unwrap_or_else(|| "—".into()),
    ));
    rows.push((
        "Link speed:",
        info.link_mbps
            .map(|m| util::bits(m as f64 * 1e6))
            .unwrap_or_else(|| "—".into()),
    ));
    rows.push(("MAC address:", info.mac.clone()));
    stats_area(
        ui,
        |ui| {
            ui.horizontal_top(|ui| {
                legend_stat(ui, pal, pal.net, true, "Send", &util::bits(n.send_bps));
                legend_stat(ui, pal, pal.net, false, "Receive", &util::bits(n.recv_bps));
            });
        },
        |ui| info_grid(ui, pal, "net-info", &rows),
    );
    format!(
        "{}\n\t{}\n\tSend\t{}\n\tReceive\t{}\n\tIPv4\t{}\n",
        info.kind,
        info.adapter,
        util::bits(n.send_bps),
        util::bits(n.recv_bps),
        info.ipv4.join(", ")
    )
}

fn gpu(ui: &mut Ui, pal: &Palette, snap: &Snapshot, i: usize) -> String {
    let Some(g) = snap.gpus.get(i) else {
        return String::new();
    };
    let info = &g.info;
    title_row(ui, pal, "GPU", &info.name);
    let w = ui.available_width();
    let h = graph_height(ui, 250.0, 260.0);
    let gap = 10.0;

    // Engine graphs: 2x2 grid.
    let ew = (w - gap) / 2.0;
    let eh = (h * 0.6 - gap) / 2.0 - 16.0;
    for pair in g.engines.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for e in pair {
                ui.vertical(|ui| {
                    ui.set_width(ew);
                    labeled_graph(
                        ui,
                        pal,
                        vec2(ew, eh.max(50.0)),
                        &e.name,
                        &format!("{:.0}%", e.util),
                        "",
                        &[Line::filled(&e.history, pal.gpu)],
                        100.0,
                        pal.gpu,
                        snap.seq,
                    );
                });
            }
        });
    }
    ui.add_space(4.0);
    let gb = |b: u64| format!("{:.1} GB", b as f64 / 1073741824.0);
    let mh = (h * 0.4 - 40.0).max(40.0) / 2.0;
    if g.vram_total > 0 {
        labeled_graph(
            ui,
            pal,
            vec2(w, mh),
            "Dedicated GPU memory usage",
            &gb(g.vram_total),
            "",
            &[Line::filled(&g.vram_history, pal.gpu)],
            g.vram_total as f32,
            pal.gpu,
            snap.seq,
        );
    }
    if g.shared_total > 0 {
        labeled_graph(
            ui,
            pal,
            vec2(w, mh),
            "Shared GPU memory usage",
            &gb(g.shared_total),
            "",
            &[Line::filled(&g.shared_history, pal.gpu)],
            g.shared_total as f32,
            pal.gpu,
            snap.seq,
        );
    }

    let mut rows = vec![
        ("Driver:", info.driver.clone()),
        ("Driver version:", info.driver_version.clone()),
        ("Physical location:", info.location.clone()),
    ];
    if let Some(p) = &info.pcie {
        rows.push(("PCIe link:", p.clone()));
    }
    if let Some(v) = &info.vbios {
        rows.push(("VBIOS:", v.clone()));
    }
    if let Some(c) = g.clock_mhz {
        rows.push(("Core clock:", format!("{c:.0} MHz")));
    }
    if let Some(c) = g.mem_clock_mhz {
        rows.push(("Memory clock:", format!("{c:.0} MHz")));
    }
    if let Some(p) = g.power_w {
        rows.push(("Power draw:", format!("{p:.0} W")));
    }
    if let Some(f) = g.fan_rpm {
        rows.push(("Fan speed:", format!("{f} RPM")));
    }
    stats_area(
        ui,
        |ui| {
            stat_row(
                ui,
                pal,
                &[
                    ("Utilization", format!("{:.0}%", g.util)),
                    (
                        "Dedicated GPU memory",
                        format!(
                            "{:.1}/{:.1} GB",
                            g.vram_used as f64 / 1073741824.0,
                            g.vram_total as f64 / 1073741824.0
                        ),
                    ),
                ],
            );
            stat_row(
                ui,
                pal,
                &[
                    (
                        "GPU Memory",
                        format!(
                            "{:.1}/{:.1} GB",
                            (g.vram_used + g.shared_used) as f64 / 1073741824.0,
                            (g.vram_total + g.shared_total) as f64 / 1073741824.0
                        ),
                    ),
                    (
                        "Shared GPU memory",
                        format!(
                            "{:.1}/{:.1} GB",
                            g.shared_used as f64 / 1073741824.0,
                            g.shared_total as f64 / 1073741824.0
                        ),
                    ),
                ],
            );
            if let Some(t) = g.temp_c {
                stat_row(ui, pal, &[("GPU Temperature", format!("{t:.0} °C"))]);
            }
        },
        |ui| info_grid(ui, pal, "gpu-info", &rows),
    );
    format!(
        "GPU {}\n\t{}\n\tUtilization\t{:.0}%\n\tDedicated GPU memory\t{}/{}\n\tShared GPU memory\t{}/{}\n\tDriver\t{} {}\n",
        info.index,
        info.name,
        g.util,
        gb(g.vram_used),
        gb(g.vram_total),
        gb(g.shared_used),
        gb(g.shared_total),
        info.driver,
        info.driver_version
    )
}
