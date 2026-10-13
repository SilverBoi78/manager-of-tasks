//! Windows 11 (Fluent) inspired look: colors, fonts and widget styling.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme,
};
use std::sync::Arc;

pub const SEMIBOLD: &str = "semibold";

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Title bar / navigation background.
    pub bg: Color32,
    /// Main content surface.
    pub content: Color32,
    /// Raised cards (settings, dialogs).
    pub card: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub border: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub accent: Color32,
    pub graph_bg: Color32,
    pub cpu: Color32,
    pub mem: Color32,
    pub disk: Color32,
    pub net: Color32,
    pub gpu: Color32,
    pub danger: Color32,
}

impl Palette {
    pub fn of(ctx: &egui::Context) -> Self {
        Self::new(ctx.global_style().visuals.dark_mode)
    }

    pub fn new(dark: bool) -> Self {
        let accent = accent(dark);
        if dark {
            Palette {
                dark,
                bg: Color32::from_rgb(0x20, 0x20, 0x20),
                content: Color32::from_rgb(0x27, 0x27, 0x27),
                card: Color32::from_rgb(0x2d, 0x2d, 0x2d),
                text: Color32::from_rgb(0xff, 0xff, 0xff),
                text_weak: Color32::from_rgb(0xa8, 0xa8, 0xa8),
                border: Color32::from_rgb(0x3a, 0x3a, 0x3a),
                hover: Color32::from_rgb(0x34, 0x34, 0x34),
                selected: blend(Color32::from_rgb(0x27, 0x27, 0x27), accent, 0.22),
                accent,
                graph_bg: Color32::from_rgb(0x1c, 0x1c, 0x1c),
                cpu: Color32::from_rgb(0x3c, 0xa0, 0xe6),
                mem: Color32::from_rgb(0xb8, 0x5c, 0xdc),
                disk: Color32::from_rgb(0x70, 0xbe, 0x32),
                net: Color32::from_rgb(0xd8, 0x86, 0x2c),
                gpu: Color32::from_rgb(0x2c, 0xc0, 0xb4),
                danger: Color32::from_rgb(0xff, 0x99, 0xa4),
            }
        } else {
            Palette {
                dark,
                bg: Color32::from_rgb(0xf3, 0xf3, 0xf3),
                content: Color32::from_rgb(0xfb, 0xfb, 0xfb),
                card: Color32::from_rgb(0xff, 0xff, 0xff),
                text: Color32::from_rgb(0x1a, 0x1a, 0x1a),
                text_weak: Color32::from_rgb(0x5f, 0x5f, 0x5f),
                border: Color32::from_rgb(0xe0, 0xe0, 0xe0),
                hover: Color32::from_rgb(0xec, 0xec, 0xec),
                selected: blend(Color32::from_rgb(0xfb, 0xfb, 0xfb), accent, 0.18),
                accent,
                graph_bg: Color32::WHITE,
                cpu: Color32::from_rgb(0x11, 0x7d, 0xbb),
                mem: Color32::from_rgb(0x8b, 0x12, 0xae),
                disk: Color32::from_rgb(0x4d, 0xa6, 0x0c),
                net: Color32::from_rgb(0xa7, 0x4f, 0x01),
                gpu: Color32::from_rgb(0x00, 0x87, 0x87),
                danger: Color32::from_rgb(0xc4, 0x2b, 0x1c),
            }
        }
    }

    /// Heat map cell color for a usage fraction (0..1), like the Processes tab.
    pub fn heat(&self, frac: f32) -> Color32 {
        let f = frac.clamp(0.0, 1.0);
        let stops: [(f32, Color32); 4] = if self.dark {
            [
                (0.0, Color32::from_rgb(0x2e, 0x2b, 0x22)),
                (0.05, Color32::from_rgb(0x45, 0x3a, 0x1c)),
                (0.5, Color32::from_rgb(0x7a, 0x55, 0x10)),
                (1.0, Color32::from_rgb(0x9c, 0x4a, 0x0c)),
            ]
        } else {
            [
                (0.0, Color32::from_rgb(0xff, 0xf4, 0xc4)),
                (0.05, Color32::from_rgb(0xff, 0xe8, 0x9c)),
                (0.5, Color32::from_rgb(0xff, 0xc8, 0x5a)),
                (1.0, Color32::from_rgb(0xf7, 0x8f, 0x2e)),
            ]
        };
        for w in stops.windows(2) {
            let (a, ca) = w[0];
            let (b, cb) = w[1];
            if f <= b {
                return blend(ca, cb, (f - a) / (b - a));
            }
        }
        stops[3].1
    }

    pub fn fill_of(&self, line: Color32) -> Color32 {
        let a = if self.dark { 0.28 } else { 0.16 };
        blend(self.graph_bg, line, a)
    }

    pub fn grid_of(&self, line: Color32) -> Color32 {
        let a = if self.dark { 0.22 } else { 0.18 };
        blend(self.graph_bg, line, a)
    }
}

pub fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// The desktop's accent color (KDE), or the Windows default blue.
fn accent(dark: bool) -> Color32 {
    static ACCENT: std::sync::OnceLock<Option<Color32>> = std::sync::OnceLock::new();
    let base = ACCENT.get_or_init(read_kde_accent);
    match base {
        Some(c) if dark => blend(*c, Color32::WHITE, 0.25),
        Some(c) => *c,
        None if dark => Color32::from_rgb(0x4c, 0xc2, 0xff),
        None => Color32::from_rgb(0x00, 0x67, 0xc0),
    }
}

fn read_kde_accent() -> Option<Color32> {
    let home = std::env::var("HOME").ok()?;
    let text = std::fs::read_to_string(format!("{home}/.config/kdeglobals")).ok()?;
    let mut group = "";
    let mut accent = None;
    let mut selection = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            group = line;
            continue;
        }
        let parse = |v: &str| -> Option<Color32> {
            let n: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (n.len() >= 3).then(|| Color32::from_rgb(n[0], n[1], n[2]))
        };
        if group == "[General]"
            && let Some(v) = line.strip_prefix("AccentColor=")
        {
            accent = parse(v);
        }
        if group == "[Colors:Selection]"
            && let Some(v) = line.strip_prefix("BackgroundNormal=")
        {
            selection = parse(v);
        }
    }
    accent.or(selection)
}

pub fn apply(ctx: &egui::Context) {
    for theme in [Theme::Dark, Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            let dark = theme == Theme::Dark;
            // Compute palette for this theme without depending on the current mode.
            let p = Palette::new(dark);
            let v = &mut style.visuals;
            v.panel_fill = p.content;
            v.window_fill = p.card;
            v.extreme_bg_color = if dark {
                Color32::from_rgb(0x1e, 0x1e, 0x1e)
            } else {
                Color32::WHITE
            };
            v.faint_bg_color = p.hover;
            v.window_stroke = Stroke::new(1.0, p.border);
            v.window_corner_radius = CornerRadius::same(8);
            v.menu_corner_radius = CornerRadius::same(8);
            v.override_text_color = Some(p.text);
            v.hyperlink_color = p.accent;
            v.selection.bg_fill = p.accent.gamma_multiply(0.6);
            v.selection.stroke = Stroke::new(1.0, p.text);
            v.striped = false;
            let r = CornerRadius::same(4);
            v.widgets.noninteractive.corner_radius = r;
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
            v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
            v.widgets.inactive.corner_radius = r;
            v.widgets.inactive.weak_bg_fill = if dark {
                Color32::from_rgb(0x2f, 0x2f, 0x2f)
            } else {
                Color32::WHITE
            };
            v.widgets.inactive.bg_fill = v.widgets.inactive.weak_bg_fill;
            v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
            v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
            v.widgets.hovered.corner_radius = r;
            v.widgets.hovered.weak_bg_fill = p.hover;
            v.widgets.hovered.bg_fill = p.hover;
            v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.border);
            v.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
            v.widgets.hovered.expansion = 0.0;
            v.widgets.active.corner_radius = r;
            v.widgets.active.weak_bg_fill = blend(p.hover, p.text, 0.08);
            v.widgets.active.bg_fill = v.widgets.active.weak_bg_fill;
            v.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
            v.widgets.active.expansion = 0.0;
            v.widgets.open.corner_radius = r;
            v.widgets.open.weak_bg_fill = p.hover;
            v.popup_shadow.color = Color32::from_black_alpha(if dark { 90 } else { 40 });
            v.window_shadow.color = Color32::from_black_alpha(if dark { 110 } else { 50 });

            style.spacing.item_spacing = egui::vec2(8.0, 6.0);
            style.spacing.button_padding = egui::vec2(10.0, 5.0);
            style.spacing.menu_margin = egui::Margin::same(6);
            style.spacing.interact_size.y = 26.0;
            style.spacing.scroll.bar_width = 6.0;
            style.spacing.scroll.floating = true;

            style.text_styles = [
                (
                    TextStyle::Small,
                    FontId::new(11.0, FontFamily::Proportional),
                ),
                (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
                (
                    TextStyle::Button,
                    FontId::new(13.0, FontFamily::Proportional),
                ),
                (
                    TextStyle::Monospace,
                    FontId::new(12.0, FontFamily::Monospace),
                ),
                (
                    TextStyle::Heading,
                    FontId::new(20.0, FontFamily::Name(SEMIBOLD.into())),
                ),
            ]
            .into();
        });
    }
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Find a font file with fontconfig.
fn fc_match(pattern: &str) -> Option<(String, String)> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{family}\n%{file}\n%{style}", pattern])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut lines = text.lines();
    let family = lines.next()?.to_string();
    let file = lines.next()?.to_string();
    let style = lines.next().unwrap_or("").to_string();
    let ok = file.ends_with(".ttf")
        || file.ends_with(".otf")
        || file.ends_with(".TTF")
        || file.ends_with(".OTF");
    ok.then_some((family + "|" + &style, file))
}

pub fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    // Prefer Segoe UI (if installed), then common modern UI fonts.
    let candidates = [
        "Segoe UI Variable Text",
        "Segoe UI",
        "Inter",
        "Noto Sans",
        "Cantarell",
        "DejaVu Sans",
    ];
    let mut chosen: Option<(String, String)> = None;
    for fam in candidates {
        if let Some((found, file)) = fc_match(fam)
            && found.to_lowercase().starts_with(&fam.to_lowercase())
        {
            chosen = Some((fam.to_string(), file));
            break;
        }
    }
    if chosen.is_none() {
        chosen = fc_match("sans-serif")
            .map(|(f, file)| (f.split(['|', ',']).next().unwrap_or("").to_string(), file));
    }

    let mut semibold_family = vec![];
    if let Some((family, file)) = &chosen
        && let Ok(bytes) = std::fs::read(file)
    {
        fonts
            .font_data
            .insert("ui".into(), Arc::new(FontData::from_owned(bytes)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "ui".into());
        for weight in ["semibold", "demibold", "medium", "bold"] {
            if let Some((found, bold_file)) = fc_match(&format!("{family}:style={weight}"))
                && found.to_lowercase().contains(weight)
                && bold_file != *file
                && let Ok(b) = std::fs::read(&bold_file)
            {
                fonts
                    .font_data
                    .insert("ui-semibold".into(), Arc::new(FontData::from_owned(b)));
                semibold_family.push("ui-semibold".to_string());
                break;
            }
        }
    }
    let proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    semibold_family.extend(proportional);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold_family);
    ctx.set_fonts(fonts);
}
