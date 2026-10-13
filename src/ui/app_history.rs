//! The App history tab: accumulated resource usage per app.

use super::PageCtx;
use super::widgets::{
    HeaderAlign, app_icon, command_button, header_cell, heat_bg, page_header, right_text,
};
use crate::sys::Shared;
use crate::sys::app_history::HistoryEntry;
use crate::util;
use egui::{Align, Layout, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as ph;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Col {
    Name,
    Cpu,
    Disk,
    Gpu,
    Last,
}

#[derive(Default)]
pub struct HistoryPage {
    sort: Option<(Col, bool)>,
    confirm_delete: bool,
}

impl HistoryPage {
    pub fn ui(&mut self, ui: &mut Ui, ctx: &mut PageCtx, shared: &Shared) {
        let pal = ctx.pal;
        let (since, mut entries): (u64, Vec<HistoryEntry>) = match shared.app_history.lock() {
            Ok(h) => (h.since, h.apps.values().cloned().collect()),
            Err(_) => (0, Vec::new()),
        };
        entries.retain(|e| ctx.matches(&e.name));
        let (col, asc) = self.sort.unwrap_or((Col::Cpu, false));
        entries.sort_by(|a, b| {
            let o = match col {
                Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Col::Cpu => a.cpu_secs.total_cmp(&b.cpu_secs),
                Col::Disk => a.disk_bytes.cmp(&b.disk_bytes),
                Col::Gpu => a.gpu_secs.total_cmp(&b.gpu_secs),
                Col::Last => a.last_seen.cmp(&b.last_seen),
            };
            if asc { o } else { o.reverse() }
        });

        page_header(ui, &pal, "App history", |ui| {
            if command_button(ui, &pal, ph::TRASH, "Delete usage history", true).clicked() {
                self.confirm_delete = true;
            }
        });
        ui.label(
            RichText::new(format!(
                "Resource usage since {} for current user account.",
                util::local_date(since)
            ))
            .color(pal.text_weak),
        );
        ui.add_space(6.0);

        if self.confirm_delete {
            egui::Modal::new(egui::Id::new("delete-history")).show(ui.ctx(), |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("Delete usage history?").size(16.0));
                ui.add_space(6.0);
                ui.label("This clears the recorded CPU, disk and GPU usage for all apps.");
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        self.confirm_delete = false;
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Delete").color(egui::Color32::WHITE))
                                .fill(pal.accent),
                        )
                        .clicked()
                    {
                        if let Ok(mut h) = shared.app_history.lock() {
                            h.clear();
                        }
                        self.confirm_delete = false;
                    }
                });
            });
        }

        let max_cpu = entries.iter().map(|e| e.cpu_secs).fold(1.0, f64::max);
        let max_disk = entries
            .iter()
            .map(|e| e.disk_bytes)
            .max()
            .unwrap_or(1)
            .max(1) as f64;
        let max_gpu = entries.iter().map(|e| e.gpu_secs).fold(1.0, f64::max);
        let sort = self.sort;
        let mut new_sort = None;
        let heat = ctx.settings.heatmap;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
            TableBuilder::new(ui)
                .id_salt("history-table")
                .resizable(true)
                .sense(Sense::hover())
                .auto_shrink(false)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::remainder().at_least(220.0).clip(true))
                .columns(Column::initial(110.0).at_least(70.0).clip(true), 3)
                .column(Column::initial(150.0).at_least(80.0).clip(true))
                .header(34.0, |mut h| {
                    for (c, title, align) in [
                        (Col::Name, "Name", HeaderAlign::Left),
                        (Col::Cpu, "CPU time", HeaderAlign::Right),
                        (Col::Disk, "Disk", HeaderAlign::Right),
                        (Col::Gpu, "GPU time", HeaderAlign::Right),
                        (Col::Last, "Last used", HeaderAlign::Left),
                    ] {
                        h.col(|ui| {
                            let sorted = match sort.unwrap_or((Col::Cpu, false)) {
                                (sc, a) if sc == c => Some(a),
                                _ => None,
                            };
                            if header_cell(ui, &pal, title, None, sorted, align).clicked() {
                                new_sort = Some(match sort {
                                    Some((sc, a)) if sc == c => (c, !a),
                                    _ => (c, c == Col::Name),
                                });
                            }
                        });
                    }
                })
                .body(|body| {
                    body.rows(30.0, entries.len(), |mut row| {
                        let e = &entries[row.index()];
                        row.col(|ui| {
                            ui.add_space(4.0);
                            app_icon(ui, &pal, ctx.icons, e.icon.as_deref(), ph::APP_WINDOW, 16.0);
                            ui.add(egui::Label::new(&e.name).truncate());
                        });
                        row.col(|ui| {
                            if heat {
                                heat_bg(ui, &pal, (e.cpu_secs / max_cpu).sqrt() as f32);
                            }
                            right_text(ui, util::cpu_time(e.cpu_secs));
                        });
                        row.col(|ui| {
                            if heat {
                                heat_bg(ui, &pal, (e.disk_bytes as f64 / max_disk).sqrt() as f32);
                            }
                            right_text(ui, util::bytes(e.disk_bytes));
                        });
                        row.col(|ui| {
                            if heat {
                                heat_bg(ui, &pal, (e.gpu_secs / max_gpu).sqrt() as f32);
                            }
                            right_text(ui, util::cpu_time(e.gpu_secs));
                        });
                        row.col(|ui| {
                            ui.label(
                                RichText::new(util::local_datetime(e.last_seen))
                                    .color(pal.text_weak),
                            );
                        });
                    });
                });
        });
        if let Some(s) = new_sort {
            self.sort = Some(s);
        }
    }
}
