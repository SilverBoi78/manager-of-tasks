//! The Startup apps tab: XDG autostart entries that can be enabled or disabled.

use super::widgets::{HeaderAlign, app_icon, command_button, header_cell, page_header};
use super::{Loader, PageCtx};
use crate::actions::Notice;
use crate::sys::startup::{self, StartupItem};
use egui::{Align, Layout, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as ph;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Col {
    Name,
    Command,
    Source,
    Status,
}

#[derive(Default)]
pub struct StartupPage {
    items: Loader<Vec<StartupItem>>,
    bios: Loader<Option<f64>>,
    selected: Option<String>,
    sort: Option<(Col, bool)>,
}

impl StartupPage {
    pub fn invalidate(&mut self) {
        self.items.invalidate();
    }

    pub fn ui(&mut self, ui: &mut Ui, ctx: &mut PageCtx) {
        let pal = ctx.pal;
        let bios = self
            .bios
            .poll(ui.ctx(), None, startup::firmware_time)
            .flatten();
        let mut items: Vec<StartupItem> = self
            .items
            .poll(ui.ctx(), None, startup::list)
            .clone()
            .unwrap_or_default();
        items.retain(|i| ctx.matches(&i.name) || ctx.matches(&i.command));
        let (col, asc) = self.sort.unwrap_or((Col::Name, true));
        items.sort_by(|a, b| {
            let o = match col {
                Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Col::Command => a.command.cmp(&b.command),
                Col::Source => a.system.cmp(&b.system),
                Col::Status => b.enabled.cmp(&a.enabled),
            };
            if asc { o } else { o.reverse() }
        });

        let sel = self
            .selected
            .as_ref()
            .and_then(|f| items.iter().find(|i| &i.file == f))
            .cloned();
        let mut toggle: Option<(StartupItem, bool)> = None;
        let mut props: Option<StartupItem> = None;
        page_header(ui, &pal, "Startup apps", |ui| {
            if command_button(ui, &pal, ph::INFO, "Properties", sel.is_some()).clicked() {
                props = sel.clone();
            }
            let enabled = sel.as_ref().map(|s| s.enabled);
            if command_button(ui, &pal, ph::PROHIBIT, "Disable", enabled == Some(true)).clicked() {
                toggle = sel.clone().map(|s| (s, false));
            }
            if command_button(ui, &pal, ph::CHECK_CIRCLE, "Enable", enabled == Some(false))
                .clicked()
            {
                toggle = sel.clone().map(|s| (s, true));
            }
            if command_button(ui, &pal, ph::APP_WINDOW, "Run new task", true).clicked() {
                ctx.dialogs.open_run();
            }
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Apps that start automatically when you sign in.")
                    .color(pal.text_weak),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(t) = bios {
                    ui.label(format!("Last BIOS time:  {t:.1} seconds"));
                }
            });
        });
        ui.add_space(6.0);

        let sort = self.sort;
        let mut new_sort = None;
        let mut new_sel = None;
        let mut open_loc = None;
        let mut search = None;
        ui.scope(|ui| {
            ui.visuals_mut().selection.bg_fill = pal.selected;
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
            TableBuilder::new(ui)
                .id_salt("startup-table")
                .resizable(true)
                .sense(Sense::click())
                .auto_shrink(false)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::initial(240.0).at_least(140.0).clip(true))
                .column(Column::remainder().at_least(160.0).clip(true))
                .column(Column::initial(90.0).clip(true))
                .column(Column::initial(90.0).clip(true))
                .column(Column::initial(120.0).clip(true))
                .header(34.0, |mut h| {
                    for (c, t) in [
                        (Col::Name, "Name"),
                        (Col::Command, "Command"),
                        (Col::Source, "Source"),
                        (Col::Status, "Status"),
                    ] {
                        h.col(|ui| {
                            let sorted = match sort.unwrap_or((Col::Name, true)) {
                                (sc, a) if sc == c => Some(a),
                                _ => None,
                            };
                            if header_cell(ui, &pal, t, None, sorted, HeaderAlign::Left).clicked() {
                                new_sort = Some(match sort {
                                    Some((sc, a)) if sc == c => (c, !a),
                                    _ => (c, true),
                                });
                            }
                        });
                    }
                    h.col(|ui| {
                        header_cell(ui, &pal, "Startup impact", None, None, HeaderAlign::Left);
                    });
                })
                .body(|body| {
                    body.rows(32.0, items.len(), |mut row| {
                        let it = &items[row.index()];
                        row.set_selected(sel.as_ref().is_some_and(|s| s.file == it.file));
                        row.col(|ui| {
                            ui.add_space(4.0);
                            app_icon(
                                ui,
                                &pal,
                                ctx.icons,
                                it.icon.as_deref(),
                                ph::ROCKET_LAUNCH,
                                18.0,
                            );
                            ui.add(egui::Label::new(&it.name).truncate());
                        });
                        row.col(|ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&it.command).color(pal.text_weak))
                                    .truncate(),
                            );
                        });
                        row.col(|ui| {
                            ui.label(if it.system { "System" } else { "User" });
                        });
                        row.col(|ui| {
                            let text = if it.enabled { "Enabled" } else { "Disabled" };
                            ui.label(if it.other_desktop {
                                RichText::new(format!("{text}*")).color(pal.text_weak)
                            } else {
                                RichText::new(text)
                            })
                            .on_hover_text(if it.other_desktop {
                                "Not started on this desktop (OnlyShowIn/NotShowIn)"
                            } else {
                                ""
                            });
                        });
                        row.col(|ui| {
                            ui.label(RichText::new("Not measured").color(pal.text_weak));
                        });
                        let r = row.response();
                        if r.clicked() || r.secondary_clicked() {
                            new_sel = Some(it.file.clone());
                        }
                        r.context_menu(|ui| {
                            ui.set_min_width(200.0);
                            if it.enabled {
                                if ui.button(format!("{}  Disable", ph::PROHIBIT)).clicked() {
                                    toggle = Some((it.clone(), false));
                                    ui.close();
                                }
                            } else if ui.button(format!("{}  Enable", ph::CHECK_CIRCLE)).clicked() {
                                toggle = Some((it.clone(), true));
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .button(format!("{}  Open file location", ph::FOLDER_OPEN))
                                .clicked()
                            {
                                open_loc = Some(it.path.display().to_string());
                                ui.close();
                            }
                            if ui.button(format!("{}  Search online", ph::GLOBE)).clicked() {
                                search = Some(it.program());
                                ui.close();
                            }
                            if ui.button(format!("{}  Properties", ph::INFO)).clicked() {
                                props = Some(it.clone());
                                ui.close();
                            }
                        });
                    });
                });
        });

        if let Some(s) = new_sort {
            self.sort = Some(s);
        }
        if let Some(s) = new_sel {
            self.selected = Some(s);
        }
        if let Some(p) = open_loc {
            ctx.actions.open_location(p);
        }
        if let Some(q) = search {
            ctx.actions.search_online(&q);
        }
        if let Some(it) = props {
            ctx.dialogs.info = Some(super::dialogs::Info {
                title: format!("{} Properties", it.name),
                rows: vec![
                    ("Name".into(), it.name.clone()),
                    ("Description".into(), it.comment.clone().unwrap_or_default()),
                    ("Command".into(), it.command.clone()),
                    (
                        "Status".into(),
                        if it.enabled {
                            "Enabled".into()
                        } else {
                            "Disabled".into()
                        },
                    ),
                    (
                        "Source".into(),
                        if it.system {
                            "System (/etc/xdg/autostart)".into()
                        } else {
                            "User (~/.config/autostart)".into()
                        },
                    ),
                    ("File".into(), it.path.display().to_string()),
                ],
            });
        }
        if let Some((it, on)) = toggle {
            let actions = ctx.actions.clone();
            std::thread::spawn(move || {
                match startup::set_enabled(&it, on) {
                    Ok(()) => actions.notify(Notice::Info(format!(
                        "{} {}",
                        it.name,
                        if on {
                            "will start automatically."
                        } else {
                            "will no longer start automatically."
                        }
                    ))),
                    Err(e) => {
                        actions.notify(Notice::Error(format!("Could not change {}: {e}", it.name)))
                    }
                }
                actions.notify(Notice::Refresh);
            });
        }
    }
}
