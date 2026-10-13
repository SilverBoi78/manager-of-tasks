//! The Users tab: resource usage per signed-in user.

use super::widgets::{HeaderAlign, app_icon, header_cell, heat_bg, page_header, right_text};
use super::{Nav, PageCtx};
use crate::sys::process::{Class, Proc};
use crate::util;
use egui::{Align, Layout, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as ph;
use std::collections::{BTreeMap, HashSet};

#[derive(Default)]
pub struct UsersPage {
    expanded: HashSet<u32>,
    selected: Option<(u32, Option<i32>)>,
}

struct UserRow<'a> {
    uid: u32,
    name: String,
    procs: Vec<&'a Proc>,
    cpu: f32,
    mem: u64,
    disk: f64,
    gpu: f32,
}

enum Row {
    User(usize),
    Proc(usize, usize),
}

enum Intent {
    Select(u32, Option<i32>),
    Toggle(u32),
    End(i32),
    Details(i32),
}

fn avatar(user: &str) -> Option<String> {
    let p = format!("/var/lib/AccountsService/icons/{user}");
    std::path::Path::new(&p).exists().then_some(p)
}

impl UsersPage {
    pub fn ui(&mut self, ui: &mut Ui, ctx: &mut PageCtx) {
        let pal = ctx.pal;
        let snap = ctx.snap;
        let mut map: BTreeMap<u32, UserRow> = BTreeMap::new();
        for p in &snap.procs {
            if p.class == Class::Kernel {
                continue;
            }
            let human = (1000..60000).contains(&p.uid) || p.uid == snap.my_uid;
            if !human {
                continue;
            }
            let u = map.entry(p.uid).or_insert_with(|| {
                let (name, full) = snap
                    .users
                    .get(&p.uid)
                    .cloned()
                    .unwrap_or_else(|| (p.user.clone(), p.user.clone()));
                UserRow {
                    uid: p.uid,
                    name: if ctx.settings.show_full_account_name {
                        format!("{full} ({name})")
                    } else {
                        name.to_string()
                    },
                    procs: Vec::new(),
                    cpu: 0.0,
                    mem: 0,
                    disk: 0.0,
                    gpu: 0.0,
                }
            });
            if ctx.matches(&p.name)
                || ctx.matches(&p.exe_name)
                || ctx.matches(&u.name)
                || ctx.matches(&p.pid.to_string())
            {
                u.procs.push(p);
            }
            u.cpu += p.cpu;
            u.mem += p.mem;
            u.disk += p.disk_read + p.disk_write;
            u.gpu += p.gpu;
        }
        let users: Vec<UserRow> = map.into_values().filter(|u| !u.procs.is_empty()).collect();
        let mut rows = Vec::new();
        for (i, u) in users.iter().enumerate() {
            rows.push(Row::User(i));
            if self.expanded.contains(&u.uid) || !ctx.search.is_empty() {
                let mut idx: Vec<usize> = (0..u.procs.len()).collect();
                idx.sort_by(|&a, &b| u.procs[b].mem.cmp(&u.procs[a].mem));
                rows.extend(idx.into_iter().map(|j| Row::Proc(i, j)));
            }
        }

        page_header(ui, &pal, "Users", |_| {});

        let mem_total = snap.mem.total.max(1) as f32;
        let heat = ctx.settings.heatmap;
        let selected = self.selected;
        let expanded = &self.expanded;
        let icons = ctx.icons;
        let mut intents = Vec::new();
        let totals = [
            ("CPU", format!("{:.0}%", snap.cpu.usage)),
            ("Memory", format!("{:.0}%", snap.mem.usage)),
            ("Disk", format!("{:.0}%", snap.disk_total)),
            ("GPU", format!("{:.0}%", snap.gpu_total)),
        ];

        ui.scope(|ui| {
            ui.visuals_mut().selection.bg_fill = pal.selected;
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
            TableBuilder::new(ui)
                .id_salt("users-table")
                .resizable(true)
                .sense(Sense::click())
                .auto_shrink(false)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::remainder().at_least(220.0).clip(true))
                .column(Column::initial(100.0).clip(true))
                .columns(Column::initial(90.0).at_least(60.0).clip(true), 4)
                .header(46.0, |mut h| {
                    h.col(|ui| {
                        header_cell(ui, &pal, "User", None, None, HeaderAlign::Left);
                    });
                    h.col(|ui| {
                        header_cell(ui, &pal, "Status", None, None, HeaderAlign::Left);
                    });
                    for (t, v) in &totals {
                        h.col(|ui| {
                            header_cell(ui, &pal, t, Some(v), None, HeaderAlign::Right);
                        });
                    }
                })
                .body(|body| {
                    body.rows(32.0, rows.len(), |mut row| match rows[row.index()] {
                        Row::User(i) => {
                            let u = &users[i];
                            let sel = selected == Some((u.uid, None));
                            row.set_selected(sel);
                            let open = expanded.contains(&u.uid) || !ctx.search.is_empty();
                            row.col(|ui| {
                                let chev = if open {
                                    ph::CARET_DOWN
                                } else {
                                    ph::CARET_RIGHT
                                };
                                if ui
                                    .add(
                                        egui::Label::new(RichText::new(chev).color(pal.text_weak))
                                            .sense(Sense::click()),
                                    )
                                    .clicked()
                                {
                                    intents.push(Intent::Toggle(u.uid));
                                }
                                let user = snap
                                    .users
                                    .get(&u.uid)
                                    .map(|x| x.0.to_string())
                                    .unwrap_or_default();
                                match avatar(&user) {
                                    Some(path) => {
                                        ui.add(
                                            egui::Image::new(format!("file://{path}"))
                                                .fit_to_exact_size(egui::vec2(20.0, 20.0))
                                                .corner_radius(10),
                                        );
                                    }
                                    None => {
                                        ui.label(
                                            RichText::new(ph::USER_CIRCLE)
                                                .size(20.0)
                                                .color(pal.text_weak),
                                        );
                                    }
                                }
                                ui.add(
                                    egui::Label::new(format!("{} ({})", u.name, u.procs.len()))
                                        .truncate(),
                                );
                            });
                            row.col(|ui| {
                                if u.uid == snap.my_uid {
                                    ui.label(RichText::new("Signed in").color(pal.text_weak));
                                }
                            });
                            row.col(|ui| {
                                num(
                                    ui,
                                    &pal,
                                    heat && !sel,
                                    (u.cpu / 100.0).sqrt(),
                                    util::pct1(u.cpu),
                                )
                            });
                            row.col(|ui| {
                                num(
                                    ui,
                                    &pal,
                                    heat && !sel,
                                    (u.mem as f32 / mem_total).sqrt(),
                                    util::mem_mb(u.mem),
                                )
                            });
                            row.col(|ui| {
                                num(
                                    ui,
                                    &pal,
                                    heat && !sel,
                                    (u.disk as f32 / 100e6).sqrt(),
                                    util::disk_rate_mb(u.disk),
                                )
                            });
                            row.col(|ui| {
                                num(
                                    ui,
                                    &pal,
                                    heat && !sel,
                                    (u.gpu.min(100.0) / 100.0).sqrt(),
                                    util::pct1(u.gpu.min(100.0)),
                                )
                            });
                            let r = row.response();
                            if r.clicked() || r.secondary_clicked() {
                                intents.push(Intent::Select(u.uid, None));
                            }
                            if r.double_clicked() {
                                intents.push(Intent::Toggle(u.uid));
                            }
                            r.context_menu(|ui| {
                                if ui
                                    .button(if open { "Collapse" } else { "Expand" })
                                    .clicked()
                                {
                                    intents.push(Intent::Toggle(u.uid));
                                    ui.close();
                                }
                            });
                        }
                        Row::Proc(i, j) => {
                            let p = users[i].procs[j];
                            let sel = selected == Some((p.uid, Some(p.pid)));
                            row.set_selected(sel);
                            row.col(|ui| {
                                ui.add_space(30.0);
                                app_icon(ui, &pal, icons, p.icon.as_deref(), ph::GEAR_SIX, 16.0);
                                ui.add(
                                    egui::Label::new(p.app_name.as_deref().unwrap_or(&p.name))
                                        .truncate(),
                                );
                            });
                            row.col(|_| {});
                            row.col(|ui| {
                                num(
                                    ui,
                                    &pal,
                                    heat && !sel,
                                    (p.cpu / 100.0).sqrt(),
                                    util::pct1(p.cpu),
                                )
                            });
                            row.col(|ui| {
                                num(
                                    ui,
                                    &pal,
                                    heat && !sel,
                                    (p.mem as f32 / mem_total).sqrt(),
                                    util::mem_mb(p.mem),
                                )
                            });
                            let d = p.disk_read + p.disk_write;
                            row.col(|ui| {
                                num(
                                    ui,
                                    &pal,
                                    heat && !sel,
                                    (d as f32 / 100e6).sqrt(),
                                    util::disk_rate_mb(d),
                                )
                            });
                            row.col(|ui| {
                                num(
                                    ui,
                                    &pal,
                                    heat && !sel,
                                    (p.gpu / 100.0).sqrt(),
                                    util::pct1(p.gpu),
                                )
                            });
                            let r = row.response();
                            if r.clicked() || r.secondary_clicked() {
                                intents.push(Intent::Select(p.uid, Some(p.pid)));
                            }
                            r.context_menu(|ui| {
                                if ui.button(format!("{}  End task", ph::PROHIBIT)).clicked() {
                                    intents.push(Intent::End(p.pid));
                                    ui.close();
                                }
                                if ui
                                    .button(format!("{}  Go to details", ph::LIST_BULLETS))
                                    .clicked()
                                {
                                    intents.push(Intent::Details(p.pid));
                                    ui.close();
                                }
                            });
                        }
                    });
                });
        });

        for i in intents {
            match i {
                Intent::Select(uid, pid) => self.selected = Some((uid, pid)),
                Intent::Toggle(uid) => {
                    if !self.expanded.remove(&uid) {
                        self.expanded.insert(uid);
                    }
                }
                Intent::End(pid) => {
                    let list: Vec<&Proc> = snap.procs.iter().filter(|p| p.pid == pid).collect();
                    ctx.dialogs
                        .end_task(&list, snap.my_uid, libc::SIGTERM, ctx.actions);
                }
                Intent::Details(pid) => *ctx.nav = Some(Nav::Details(pid)),
            }
        }
    }
}

fn num(ui: &mut Ui, pal: &crate::theme::Palette, heat: bool, frac: f32, text: String) {
    if heat {
        heat_bg(ui, pal, frac);
    }
    right_text(ui, text);
}
