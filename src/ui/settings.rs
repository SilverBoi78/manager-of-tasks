//! The Settings page.

use super::PageCtx;
use super::widgets::{page_header, setting_card, toggle};
use crate::settings::{Page, Speed, ThemePref};
use crate::theme::{regular, semibold};
use egui::{RichText, Ui};
use egui_phosphor::regular as ph;

pub const REPO_URL: &str = "https://github.com/SilverBoi78/manager-of-tasks";

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).font(semibold(14.0)));
    ui.add_space(4.0);
}

pub fn ui(ui: &mut Ui, ctx: &mut PageCtx) {
    let pal = ctx.pal;
    page_header(ui, &pal, "Settings", |_| {});
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.set_max_width(860.0);
            let s = &mut *ctx.settings;

            section(ui, "General");
            setting_card(
                ui,
                &pal,
                ph::HOUSE,
                "Default start page",
                "Choose the page shown when Manager of Tasks opens",
                |ui| {
                    egui::ComboBox::from_id_salt("start-page")
                        .selected_text(s.start_page.title())
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for p in Page::MAIN {
                                ui.selectable_value(&mut s.start_page, p, p.title());
                            }
                        });
                },
            );
            setting_card(
                ui,
                &pal,
                ph::TIMER,
                "Real time update speed",
                "How often the data is refreshed",
                |ui| {
                    egui::ComboBox::from_id_salt("speed")
                        .selected_text(s.speed.label())
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for sp in Speed::ALL {
                                let desc = match sp {
                                    Speed::High => "High (every 0.5 s)",
                                    Speed::Normal => "Normal (every second)",
                                    Speed::Low => "Low (every 4 s)",
                                    Speed::Paused => "Paused",
                                };
                                ui.selectable_value(&mut s.speed, sp, desc);
                            }
                        });
                },
            );

            section(ui, "Appearance");
            setting_card(
                ui,
                &pal,
                ph::PAINT_BRUSH,
                "App theme",
                "Select which app theme to display",
                |ui| {
                    egui::ComboBox::from_id_salt("theme")
                        .selected_text(match s.theme {
                            ThemePref::System => "Use system setting",
                            ThemePref::Light => "Light",
                            ThemePref::Dark => "Dark",
                        })
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut s.theme, ThemePref::Light, "Light");
                            ui.selectable_value(&mut s.theme, ThemePref::Dark, "Dark");
                            ui.selectable_value(
                                &mut s.theme,
                                ThemePref::System,
                                "Use system setting",
                            );
                        });
                },
            );
            setting_card(
                ui,
                &pal,
                ph::SQUARES_FOUR,
                "Heat map colors",
                "Shade busy cells on the Processes and Users pages",
                |ui| {
                    toggle(ui, &pal, &mut s.heatmap);
                },
            );

            section(ui, "Window management");
            setting_card(
                ui,
                &pal,
                ph::PUSH_PIN,
                "Always on top",
                "Keep the window above other windows (if your compositor allows it)",
                |ui| {
                    toggle(ui, &pal, &mut s.always_on_top);
                },
            );

            section(ui, "Other options");
            setting_card(
                ui,
                &pal,
                ph::SQUARES_FOUR,
                "Group processes by type",
                "Show Apps, Background processes and System processes separately",
                |ui| {
                    toggle(ui, &pal, &mut s.group_by_type);
                },
            );
            setting_card(
                ui,
                &pal,
                ph::USER,
                "Show full account name",
                "Show the full name of users on the Users page",
                |ui| {
                    toggle(ui, &pal, &mut s.show_full_account_name);
                },
            );
            setting_card(
                ui,
                &pal,
                ph::CLOCK_COUNTER_CLOCKWISE,
                "Show history for all processes",
                "Record App history for background processes too",
                |ui| {
                    toggle(ui, &pal, &mut s.history_all_processes);
                },
            );
            setting_card(
                ui,
                &pal,
                ph::CPU,
                "Show kernel threads",
                "List kernel threads on the Details page",
                |ui| {
                    toggle(ui, &pal, &mut s.show_kernel_threads);
                },
            );
            setting_card(
                ui,
                &pal,
                ph::LEAF,
                "Ask before turning on Efficiency mode",
                "Show a confirmation the first time",
                |ui| {
                    toggle(ui, &pal, &mut s.confirm_efficiency);
                },
            );

            section(ui, "About");
            let mut open_repo = false;
            setting_card(
                ui,
                &pal,
                ph::INFO,
                &format!("Manager of Tasks {}", env!("CARGO_PKG_VERSION")),
                "A Windows 11 style task manager for Linux. Open source under the MIT license.",
                |ui| {
                    if ui
                        .button(
                            RichText::new(format!("{}  GitHub", ph::GITHUB_LOGO))
                                .font(regular(13.0)),
                        )
                        .clicked()
                    {
                        open_repo = true;
                    }
                },
            );
            if open_repo {
                ctx.actions.open_url(REPO_URL);
            }
            ui.add_space(12.0);
        });
}
