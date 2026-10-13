//! The application window: title bar, navigation pane and page routing.

use crate::actions::{Actions, Notice};
use crate::settings::{Page, Settings, ThemePref};
use crate::sys::icons::IconIndex;
use crate::sys::{self, Cmd, Shared, Snapshot};
use crate::theme::{self, Palette, regular};
use crate::ui::{self, Nav, PageCtx, dialogs::Dialogs};
use egui::{
    Align, Align2, Color32, CornerRadius, Frame, Key, Layout, Margin, Rect, RichText, Sense,
    Stroke, UiBuilder, pos2, vec2,
};
use egui_phosphor::regular as ph;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

pub struct App {
    shared: Arc<Shared>,
    snap: Arc<Snapshot>,
    settings: Settings,
    saved_settings: Settings,
    page: Page,
    search: String,
    focus_search: bool,
    actions: Actions,
    notices: Receiver<Notice>,
    toasts: Vec<(bool, String, Instant)>,
    icons: Arc<OnceLock<IconIndex>>,
    dialogs: Dialogs,
    nav: Option<Nav>,
    processes: ui::processes::ProcessesPage,
    performance: ui::performance::PerformancePage,
    history: ui::app_history::HistoryPage,
    startup: ui::startup::StartupPage,
    users: ui::users::UsersPage,
    details: ui::details::DetailsPage,
    services: ui::services::ServicesPage,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        start_page: Option<Page>,
        perf_item: Option<ui::performance::Item>,
    ) -> Self {
        let ctx = cc.egui_ctx.clone();
        egui_extras::install_image_loaders(&ctx);
        theme::setup_fonts(&ctx);
        theme::apply(&ctx);
        apply_theme(&ctx, settings.theme);
        if settings.always_on_top {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
        }

        let shared = sys::start(
            ctx.clone(),
            settings.speed.interval(),
            settings.history_all_processes,
        );
        let (tx, rx) = channel();
        let actions = Actions::new(tx, ctx.clone());

        let icons = Arc::new(OnceLock::new());
        {
            let icons = icons.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = icons.set(IconIndex::build());
                ctx.request_repaint();
            });
        }

        let mut performance = ui::performance::PerformancePage::default();
        if let Some(item) = perf_item {
            performance.show_item(item);
        }

        Self {
            snap: shared.snapshot(),
            shared,
            page: start_page.unwrap_or(settings.start_page),
            saved_settings: settings.clone(),
            settings,
            search: String::new(),
            focus_search: false,
            actions,
            notices: rx,
            toasts: Vec::new(),
            icons,
            dialogs: Dialogs::default(),
            nav: None,
            processes: Default::default(),
            performance,
            history: Default::default(),
            startup: Default::default(),
            users: Default::default(),
            details: Default::default(),
            services: Default::default(),
        }
    }

    fn poll(&mut self) {
        self.snap = self.shared.snapshot();
        while let Ok(n) = self.notices.try_recv() {
            match n {
                Notice::Info(m) => self.toasts.push((false, m, Instant::now())),
                Notice::Error(m) => self.toasts.push((true, m, Instant::now())),
                Notice::Refresh => {
                    self.shared.send(Cmd::RefreshNow);
                    self.startup.invalidate();
                    self.services.invalidate();
                }
            }
        }
        self.toasts.retain(|t| t.2.elapsed().as_secs() < 6);
    }

    fn settings_changed(&mut self, ctx: &egui::Context) {
        if self.settings == self.saved_settings {
            return;
        }
        let old = &self.saved_settings;
        if old.speed != self.settings.speed {
            self.shared
                .send(Cmd::SetInterval(self.settings.speed.interval()));
        }
        if old.theme != self.settings.theme {
            apply_theme(ctx, self.settings.theme);
        }
        if old.always_on_top != self.settings.always_on_top {
            let level = if self.settings.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
        }
        if old.history_all_processes != self.settings.history_all_processes {
            self.shared
                .send(Cmd::SetHistoryAll(self.settings.history_all_processes));
        }
        self.settings.save();
        self.saved_settings = self.settings.clone();
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (find, esc, f5, tab, shift) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(Key::F),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::F5),
                i.modifiers.ctrl && i.key_pressed(Key::Tab),
                i.modifiers.shift,
            )
        });
        if find {
            self.focus_search = true;
        }
        if esc && !self.search.is_empty() && !ctx.egui_wants_keyboard_input() {
            self.search.clear();
        }
        if f5 {
            self.shared.send(Cmd::RefreshNow);
        }
        if tab {
            let pages = Page::MAIN;
            let i = pages.iter().position(|p| *p == self.page).unwrap_or(0);
            let n = pages.len();
            self.page = pages[if shift { (i + n - 1) % n } else { (i + 1) % n }];
        }
    }

    fn title_bar(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let full = ui.max_rect();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 34.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect, 4, pal.hover);
            }
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                ph::LIST,
                regular(18.0),
                pal.text,
            );
            if resp.on_hover_text("Open/close navigation").clicked() {
                self.settings.nav_expanded = !self.settings.nav_expanded;
            }
            ui.add_space(4.0);
            app_logo(ui, pal, 18.0);
            ui.label(
                RichText::new("Manager of Tasks")
                    .font(regular(12.5))
                    .color(pal.text),
            );
        });

        // Search box centered in the title bar, like Windows 11.
        let w = (full.width() * 0.42).clamp(220.0, 460.0);
        let rect = Rect::from_center_size(full.center(), vec2(w, 32.0));
        let left_edge = 240.0 + full.left();
        let rect = if rect.left() < left_edge {
            rect.translate(vec2(left_edge - rect.left(), 0.0))
        } else {
            rect
        };
        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            Frame::new()
                .fill(if pal.dark {
                    Color32::from_rgb(0x2c, 0x2c, 0x2c)
                } else {
                    Color32::WHITE
                })
                .stroke(Stroke::new(1.0, pal.border))
                .corner_radius(CornerRadius::same(5))
                .inner_margin(Margin::symmetric(10, 5))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(ph::MAGNIFYING_GLASS)
                                .font(regular(14.0))
                                .color(pal.text_weak),
                        );
                        let edit = egui::TextEdit::singleline(&mut self.search)
                            .frame(egui::Frame::NONE)
                            .hint_text("Type a name, publisher, or PID to search")
                            .desired_width(ui.available_width() - 22.0);
                        let r = ui.add(edit);
                        if self.focus_search {
                            r.request_focus();
                            self.focus_search = false;
                        }
                        if !self.search.is_empty() {
                            let x = ui.add(
                                egui::Label::new(RichText::new(ph::X).color(pal.text_weak))
                                    .sense(Sense::click()),
                            );
                            if x.clicked() {
                                self.search.clear();
                            }
                        }
                    });
                });
        });
    }

    fn nav_pane(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let expanded = self.settings.nav_expanded;
        let item = |ui: &mut egui::Ui, page: Page, icon: &str, current: Page| -> bool {
            let w = ui.available_width();
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 38.0), Sense::click());
            let selected = page == current;
            if selected || resp.hovered() {
                let fill = if selected {
                    pal.hover
                } else {
                    theme::blend(pal.bg, pal.hover, 0.7)
                };
                ui.painter().rect_filled(rect, 5, fill);
            }
            if selected {
                let pill = Rect::from_center_size(
                    pos2(rect.left() + 2.5, rect.center().y),
                    vec2(3.0, 16.0),
                );
                ui.painter().rect_filled(pill, 2, pal.accent);
            }
            ui.painter().text(
                pos2(rect.left() + 22.0, rect.center().y),
                Align2::CENTER_CENTER,
                icon,
                regular(18.0),
                pal.text,
            );
            if expanded {
                ui.painter().text(
                    pos2(rect.left() + 46.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    page.title(),
                    regular(13.0),
                    pal.text,
                );
            }
            let resp = if expanded {
                resp
            } else {
                resp.on_hover_text(page.title())
            };
            resp.clicked()
        };
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing.y = 3.0;
        let icons = [
            (Page::Processes, ph::SQUARES_FOUR),
            (Page::Performance, ph::PULSE),
            (Page::AppHistory, ph::CLOCK_COUNTER_CLOCKWISE),
            (Page::Startup, ph::GAUGE),
            (Page::Users, ph::USERS),
            (Page::Details, ph::LIST_BULLETS),
            (Page::Services, ph::PUZZLE_PIECE),
        ];
        for (page, icon) in icons {
            if item(ui, page, icon, self.page) {
                self.page = page;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(6.0);
            if item(ui, Page::Settings, ph::GEAR_SIX, self.page) {
                self.page = Page::Settings;
            }
        });
    }

    fn toasts(&self, ctx: &egui::Context, pal: &Palette) {
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -16.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for (err, msg, _) in &self.toasts {
                    Frame::new()
                        .fill(pal.card)
                        .stroke(Stroke::new(1.0, pal.border))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(14, 10))
                        .shadow(ui.style().visuals.popup_shadow)
                        .show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.horizontal(|ui| {
                                let (icon, color) = if *err {
                                    (ph::WARNING, pal.danger)
                                } else {
                                    (ph::INFO, pal.accent)
                                };
                                ui.label(RichText::new(icon).font(regular(16.0)).color(color));
                                ui.add(egui::Label::new(msg.as_str()).wrap());
                            });
                        });
                    ui.add_space(6.0);
                }
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

fn apply_theme(ctx: &egui::Context, pref: ThemePref) {
    ctx.set_theme(match pref {
        ThemePref::System => egui::ThemePreference::System,
        ThemePref::Light => egui::ThemePreference::Light,
        ThemePref::Dark => egui::ThemePreference::Dark,
    });
}

/// The app's logo: a rounded square with a pulse line.
pub fn app_logo(ui: &mut egui::Ui, pal: &Palette, size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint_logo(ui.painter(), rect, pal.accent);
}

pub fn paint_logo(painter: &egui::Painter, rect: Rect, accent: Color32) {
    painter.rect_filled(rect, rect.width() * 0.22, accent);
    let r = rect.shrink(rect.width() * 0.18);
    let pts = [
        (0.0, 0.6),
        (0.25, 0.6),
        (0.4, 0.2),
        (0.6, 0.85),
        (0.75, 0.45),
        (1.0, 0.45),
    ]
    .map(|(x, y)| pos2(r.left() + x * r.width(), r.top() + y * r.height()));
    painter.add(egui::Shape::line(
        pts.to_vec(),
        Stroke::new(rect.width() * 0.09, Color32::WHITE),
    ));
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll();
        self.shortcuts(&ctx);
        let pal = Palette::of(&ctx);

        egui::Panel::top("title-bar")
            .frame(Frame::new().fill(pal.bg).inner_margin(Margin {
                left: 6,
                right: 12,
                top: 6,
                bottom: 4,
            }))
            .exact_size(46.0)
            .show_separator_line(false)
            .show(ui, |ui| self.title_bar(ui, &pal));

        egui::Panel::left("nav")
            .frame(Frame::new().fill(pal.bg).inner_margin(Margin {
                left: 6,
                right: 6,
                top: 0,
                bottom: 4,
            }))
            .exact_size(if self.settings.nav_expanded {
                230.0
            } else {
                54.0
            })
            .resizable(false)
            .show_separator_line(false)
            .show(ui, |ui| self.nav_pane(ui, &pal));

        // Handle navigation requests from the previous frame.
        if let Some(nav) = self.nav.take() {
            match nav {
                Nav::Details(pid) => {
                    self.page = Page::Details;
                    self.details.select(pid);
                }
                Nav::Processes(pid) => {
                    self.page = Page::Processes;
                    self.processes.select_pid(pid);
                }
                Nav::Services(name) => {
                    self.page = Page::Services;
                    self.services.select(name);
                }
            }
        }

        let icons = self.icons.get();
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(pal.content)
                    .stroke(Stroke::new(1.0, pal.border))
                    .corner_radius(CornerRadius {
                        nw: 8,
                        ne: 0,
                        sw: 0,
                        se: 0,
                    })
                    .inner_margin(Margin {
                        left: 18,
                        right: 14,
                        top: 10,
                        bottom: 8,
                    }),
            )
            .show(ui, |ui| {
                let mut pctx = PageCtx {
                    snap: &self.snap,
                    settings: &mut self.settings,
                    actions: &self.actions,
                    icons,
                    search: &self.search,
                    dialogs: &mut self.dialogs,
                    pal,
                    nav: &mut self.nav,
                };
                match self.page {
                    Page::Processes => self.processes.ui(ui, &mut pctx),
                    Page::Performance => self.performance.ui(ui, &mut pctx),
                    Page::AppHistory => self.history.ui(ui, &mut pctx, &self.shared),
                    Page::Startup => self.startup.ui(ui, &mut pctx),
                    Page::Users => self.users.ui(ui, &mut pctx),
                    Page::Details => self.details.ui(ui, &mut pctx),
                    Page::Services => self.services.ui(ui, &mut pctx),
                    Page::Settings => ui::settings::ui(ui, &mut pctx),
                }
            });

        self.dialogs
            .show(&ctx, &pal, &self.snap, &self.actions, &mut self.settings);
        self.toasts(&ctx, &pal);
        self.settings_changed(&ctx);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        Palette::new(visuals.dark_mode).bg.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Ok(mut h) = self.shared.app_history.lock() {
            h.save();
        }
        self.settings.save();
    }
}
