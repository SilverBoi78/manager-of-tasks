//! The Services tab: systemd system and user services.

use super::widgets::{
    HeaderAlign, command_button, header_cell, page_header, right_text, segmented,
};
use super::{Loader, Nav, PageCtx};
use crate::actions::Notice;
use crate::sys::services::{self, Service};
use egui::{Align, Layout, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as ph;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Col {
    Name,
    Pid,
    Description,
    Status,
    Startup,
}

#[derive(Default)]
pub struct ServicesPage {
    system: Loader<Result<Vec<Service>, String>>,
    user: Loader<Result<Vec<Service>, String>>,
    user_mode: bool,
    selected: Option<String>,
    sort: Option<(Col, bool)>,
    scroll_to_selected: bool,
}

impl ServicesPage {
    pub fn invalidate(&mut self) {
        self.system.invalidate();
        self.user.invalidate();
    }

    pub fn select(&mut self, unit: String) {
        self.user_mode = false;
        self.selected = Some(unit);
        self.scroll_to_selected = true;
    }

    pub fn ui(&mut self, ui: &mut Ui, ctx: &mut PageCtx) {
        let pal = ctx.pal;
        let user = self.user_mode;
        let loader = if user {
            &mut self.user
        } else {
            &mut self.system
        };
        let loading = loader.is_loading();
        let data = loader
            .poll(ui.ctx(), Some(Duration::from_secs(5)), move || {
                services::list(user)
            })
            .clone();
        let (mut list, error) = match data {
            Some(Ok(l)) => (l, None),
            Some(Err(e)) => (Vec::new(), Some(e)),
            None => (Vec::new(), None),
        };
        list.retain(|s| {
            ctx.matches(&s.name)
                || ctx.matches(&s.description)
                || s.pid.is_some_and(|p| ctx.matches(&p.to_string()))
        });
        let (col, asc) = self.sort.unwrap_or((Col::Name, true));
        list.sort_by(|a, b| {
            let o = match col {
                Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Col::Pid => a.pid.cmp(&b.pid),
                Col::Description => a
                    .description
                    .to_lowercase()
                    .cmp(&b.description.to_lowercase()),
                Col::Status => a.status().cmp(b.status()),
                Col::Startup => a.file_state.cmp(&b.file_state),
            };
            let o = if asc { o } else { o.reverse() };
            o.then(a.name.cmp(&b.name))
        });

        let sel = self
            .selected
            .as_ref()
            .and_then(|n| list.iter().find(|s| &s.name == n))
            .cloned();
        let mut verb: Option<(&'static str, String)> = None;
        let mut mode = self.user_mode;
        page_header(ui, &pal, "Services", |ui| {
            let running = sel.as_ref().map(|s| s.is_running());
            if command_button(
                ui,
                &pal,
                ph::ARROW_CLOCKWISE,
                "Restart",
                running == Some(true),
            )
            .clicked()
            {
                verb = sel.as_ref().map(|s| ("restart", s.name.clone()));
            }
            if command_button(ui, &pal, ph::STOP, "Stop", running == Some(true)).clicked() {
                verb = sel.as_ref().map(|s| ("stop", s.name.clone()));
            }
            if command_button(ui, &pal, ph::PLAY, "Start", running == Some(false)).clicked() {
                verb = sel.as_ref().map(|s| ("start", s.name.clone()));
            }
            ui.add_space(8.0);
            segmented(ui, &pal, &mut mode, &[(true, "User"), (false, "System")]);
        });
        if mode != self.user_mode {
            self.user_mode = mode;
            self.selected = None;
        }
        if let Some(e) = &error {
            ui.label(RichText::new(e).color(pal.danger));
        } else if list.is_empty() && loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading services…");
            });
        }

        let sort = self.sort;
        let mut new_sort = None;
        let mut new_sel = None;
        let mut nav = None;
        let mut search = None;
        let scroll_row = if self.scroll_to_selected && !list.is_empty() {
            self.scroll_to_selected = false;
            self.selected
                .as_ref()
                .and_then(|n| list.iter().position(|s| &s.name == n))
        } else {
            None
        };
        ui.scope(|ui| {
            ui.visuals_mut().selection.bg_fill = pal.selected;
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
            let mut table = TableBuilder::new(ui)
                .id_salt("services-table")
                .resizable(true)
                .sense(Sense::click())
                .auto_shrink(false)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::initial(250.0).at_least(120.0).clip(true))
                .column(Column::initial(70.0).at_least(40.0).clip(true))
                .column(Column::remainder().at_least(160.0).clip(true))
                .column(Column::initial(120.0).clip(true))
                .column(Column::initial(100.0).clip(true));
            if let Some(r) = scroll_row {
                table = table.scroll_to_row(r, Some(Align::Center));
            }
            table
                .header(34.0, |mut h| {
                    for (c, t, align) in [
                        (Col::Name, "Name", HeaderAlign::Left),
                        (Col::Pid, "PID", HeaderAlign::Right),
                        (Col::Description, "Description", HeaderAlign::Left),
                        (Col::Status, "Status", HeaderAlign::Left),
                        (Col::Startup, "Startup type", HeaderAlign::Left),
                    ] {
                        h.col(|ui| {
                            let sorted = match sort.unwrap_or((Col::Name, true)) {
                                (sc, a) if sc == c => Some(a),
                                _ => None,
                            };
                            if header_cell(ui, &pal, t, None, sorted, align).clicked() {
                                new_sort = Some(match sort {
                                    Some((sc, a)) if sc == c => (c, !a),
                                    _ => (c, true),
                                });
                            }
                        });
                    }
                })
                .body(|body| {
                    body.rows(28.0, list.len(), |mut row| {
                        let s = &list[row.index()];
                        row.set_selected(sel.as_ref().is_some_and(|x| x.name == s.name));
                        row.col(|ui| {
                            ui.add_space(4.0);
                            ui.label(RichText::new(ph::PUZZLE_PIECE).color(pal.text_weak));
                            ui.add(egui::Label::new(s.display_name()).truncate());
                        });
                        row.col(|ui| {
                            if let Some(p) = s.pid {
                                right_text(ui, p.to_string());
                            }
                        });
                        row.col(|ui| {
                            ui.add(egui::Label::new(&s.description).truncate());
                        });
                        row.col(|ui| {
                            let color = match s.status() {
                                "Failed" => pal.danger,
                                "Stopped" => pal.text_weak,
                                _ => pal.text,
                            };
                            ui.label(RichText::new(s.status()).color(color));
                        });
                        row.col(|ui| {
                            ui.label(
                                RichText::new(startup_type(&s.file_state)).color(pal.text_weak),
                            );
                        });
                        let r = row.response();
                        if r.clicked() || r.secondary_clicked() {
                            new_sel = Some(s.name.clone());
                        }
                        r.context_menu(|ui| {
                            ui.set_min_width(200.0);
                            let running = s.is_running();
                            if ui
                                .add_enabled(
                                    !running,
                                    egui::Button::new(format!("{}  Start", ph::PLAY)),
                                )
                                .clicked()
                            {
                                verb = Some(("start", s.name.clone()));
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    running,
                                    egui::Button::new(format!("{}  Stop", ph::STOP)),
                                )
                                .clicked()
                            {
                                verb = Some(("stop", s.name.clone()));
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    running,
                                    egui::Button::new(format!("{}  Restart", ph::ARROW_CLOCKWISE)),
                                )
                                .clicked()
                            {
                                verb = Some(("restart", s.name.clone()));
                                ui.close();
                            }
                            ui.separator();
                            let can_toggle =
                                matches!(s.file_state.as_str(), "enabled" | "disabled");
                            if ui
                                .add_enabled(
                                    can_toggle && s.file_state == "disabled",
                                    egui::Button::new(format!(
                                        "{}  Enable at startup",
                                        ph::CHECK_CIRCLE
                                    )),
                                )
                                .clicked()
                            {
                                verb = Some(("enable", s.name.clone()));
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    can_toggle && s.file_state == "enabled",
                                    egui::Button::new(format!(
                                        "{}  Disable at startup",
                                        ph::PROHIBIT
                                    )),
                                )
                                .clicked()
                            {
                                verb = Some(("disable", s.name.clone()));
                                ui.close();
                            }
                            ui.separator();
                            if let Some(pid) = s.pid
                                && ui
                                    .button(format!("{}  Go to details", ph::LIST_BULLETS))
                                    .clicked()
                            {
                                nav = Some(Nav::Details(pid as i32));
                                ui.close();
                            }
                            if ui.button(format!("{}  Search online", ph::GLOBE)).clicked() {
                                search = Some(s.name.clone());
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
        if let Some(n) = nav {
            *ctx.nav = Some(n);
        }
        if let Some(q) = search {
            ctx.actions.search_online(&q);
        }
        if let Some((v, unit)) = verb {
            let actions = ctx.actions.clone();
            std::thread::spawn(move || {
                if let Err(e) = services::action(user, v, &unit) {
                    actions.notify(Notice::Error(e));
                }
                actions.notify(Notice::Refresh);
            });
        }
    }
}

fn startup_type(state: &str) -> &str {
    match state {
        "enabled" | "enabled-runtime" => "Automatic",
        "disabled" => "Manual",
        "static" | "indirect" | "generated" | "transient" | "alias" => "Static",
        "masked" | "masked-runtime" => "Disabled",
        "" => "",
        other => other,
    }
}
