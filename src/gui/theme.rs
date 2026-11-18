//! Looks: one palette and a few layout rules per look, applied to egui's style.
//!
//! Modern is the default (graphite neutrals, one amber accent used for search matches, keycap
//! signature). Ubuntu, Windows 95 and macOS are inspired by those desktops; Classic keeps the
//! first egui port's look.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Margin, Stroke, Vec2,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    ModernDark,
    ModernLight,
    UbuntuLight,
    UbuntuDark,
    Win95,
    MacLight,
    MacDark,
    Classic,
}

pub const ALL: [Look; 8] = [
    Look::ModernDark,
    Look::ModernLight,
    Look::UbuntuLight,
    Look::UbuntuDark,
    Look::MacLight,
    Look::MacDark,
    Look::Win95,
    Look::Classic,
];

/// How the commands are reached: a classic menu bar, or a header bar with a menu button.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Chrome {
    Menubar,
    Headerbar,
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// header and status bar
    pub bar: Color32,
    /// result list and page
    pub page: Color32,
    /// every other row (`None`: no stripes)
    pub stripe: Option<Color32>,
    /// inputs, hover, raised controls
    pub raised: Color32,
    pub hairline: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    /// selected row
    pub sel_bg: Color32,
    pub sel_fg: Color32,
    /// draw an accent bar at the left of the selected row (instead of a filled selection)
    pub sel_marker: bool,
    /// search match marking inside names
    pub match_bg: Color32,
    pub match_fg: Color32,
    pub radius: u8,
    pub window_radius: u8,
    pub row_height: f32,
    pub body_size: f32,
    pub small_size: f32,
    /// Windows 95 style two-tone bevels
    pub bevel: bool,
    pub chrome: Chrome,
}

impl Look {
    pub fn id(self) -> &'static str {
        match self {
            Look::ModernDark => "modern-dark",
            Look::ModernLight => "modern-light",
            Look::UbuntuLight => "ubuntu",
            Look::UbuntuDark => "ubuntu-dark",
            Look::Win95 => "win95",
            Look::MacLight => "macos",
            Look::MacDark => "macos-dark",
            Look::Classic => "classic",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Look::ModernDark => "Modern",
            Look::ModernLight => "Modern light",
            Look::UbuntuLight => "Ubuntu",
            Look::UbuntuDark => "Ubuntu dark",
            Look::Win95 => "Windows 95",
            Look::MacLight => "macOS",
            Look::MacDark => "macOS dark",
            Look::Classic => "Classic",
        }
    }

    pub fn from_id(s: &str) -> Look {
        ALL.into_iter()
            .find(|l| l.id() == s)
            .unwrap_or(Look::ModernDark)
    }

    pub fn palette(self) -> Palette {
        let hex = |v: u32| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        let a = |c: Color32, alpha: f32| c.gamma_multiply(alpha);
        match self {
            Look::ModernDark => Palette {
                dark: true,
                bar: hex(0x0f0f10),
                page: hex(0x141415),
                stripe: None,
                raised: hex(0x232325),
                hairline: a(hex(0xededed), 0.09),
                text: hex(0xededed),
                text2: hex(0xa1a1a6),
                muted: hex(0x85858b),
                accent: hex(0xe8a33d),
                on_accent: hex(0x1a1206),
                sel_bg: hex(0xe8a33d),
                sel_fg: hex(0x1a1206),
                sel_marker: true,
                match_bg: a(hex(0xe8a33d), 0.24),
                match_fg: hex(0xf3c27a),
                radius: 8,
                window_radius: 14,
                row_height: 32.0,
                body_size: 14.0,
                small_size: 12.0,
                bevel: false,
                chrome: Chrome::Menubar,
            },
            Look::ModernLight => Palette {
                dark: false,
                bar: hex(0xf4f4f5),
                page: hex(0xfafafa),
                stripe: None,
                raised: hex(0xffffff),
                hairline: a(hex(0x171717), 0.09),
                text: hex(0x171717),
                text2: hex(0x525252),
                muted: hex(0x6b6b6b),
                accent: hex(0xa8620a),
                on_accent: hex(0xffffff),
                sel_bg: hex(0xa8620a),
                sel_fg: hex(0xffffff),
                sel_marker: true,
                match_bg: a(hex(0xf2b544), 0.38),
                match_fg: hex(0x3d2604),
                radius: 8,
                window_radius: 14,
                row_height: 32.0,
                body_size: 14.0,
                small_size: 12.0,
                bevel: false,
                chrome: Chrome::Menubar,
            },
            Look::UbuntuLight => Palette {
                dark: false,
                bar: hex(0xebebeb),
                page: hex(0xfafafa),
                stripe: None,
                raised: hex(0xffffff),
                hairline: a(hex(0x000000), 0.10),
                text: hex(0x3d3d3d),
                text2: hex(0x5e5c64),
                muted: hex(0x77767b),
                accent: hex(0xe95420),
                on_accent: hex(0xffffff),
                sel_bg: hex(0xe95420),
                sel_fg: hex(0xffffff),
                sel_marker: false,
                match_bg: a(hex(0xe95420), 0.18),
                match_fg: hex(0xb33d0d),
                radius: 8,
                window_radius: 12,
                row_height: 34.0,
                body_size: 14.0,
                small_size: 12.0,
                bevel: false,
                chrome: Chrome::Headerbar,
            },
            Look::UbuntuDark => Palette {
                dark: true,
                bar: hex(0x303030),
                page: hex(0x242424),
                stripe: None,
                raised: hex(0x3a3a3a),
                hairline: a(hex(0xffffff), 0.10),
                text: hex(0xf7f7f7),
                text2: hex(0xc0bfbc),
                muted: hex(0x9a9996),
                accent: hex(0xe95420),
                on_accent: hex(0xffffff),
                sel_bg: hex(0xc7461a),
                sel_fg: hex(0xffffff),
                sel_marker: false,
                match_bg: a(hex(0xe95420), 0.30),
                match_fg: hex(0xffa37c),
                radius: 8,
                window_radius: 12,
                row_height: 34.0,
                body_size: 14.0,
                small_size: 12.0,
                bevel: false,
                chrome: Chrome::Headerbar,
            },
            Look::Win95 => Palette {
                dark: false,
                bar: hex(0xc0c0c0),
                page: hex(0xffffff),
                stripe: None,
                raised: hex(0xffffff),
                hairline: hex(0x808080),
                text: hex(0x000000),
                text2: hex(0x000000),
                muted: hex(0x404040),
                accent: hex(0x000080),
                on_accent: hex(0xffffff),
                sel_bg: hex(0x000080),
                sel_fg: hex(0xffffff),
                sel_marker: false,
                match_bg: hex(0xffff00),
                match_fg: hex(0x000000),
                radius: 0,
                window_radius: 0,
                row_height: 22.0,
                body_size: 13.0,
                small_size: 12.0,
                bevel: true,
                chrome: Chrome::Menubar,
            },
            Look::MacLight => Palette {
                dark: false,
                bar: hex(0xececec),
                page: hex(0xffffff),
                stripe: Some(hex(0xf4f5f5)),
                raised: hex(0xffffff),
                hairline: a(hex(0x000000), 0.12),
                text: hex(0x1d1d1f),
                text2: hex(0x4b4b4f),
                muted: hex(0x6e6e73),
                accent: hex(0x0064e1),
                on_accent: hex(0xffffff),
                sel_bg: hex(0x0064e1),
                sel_fg: hex(0xffffff),
                sel_marker: false,
                match_bg: a(hex(0xffd60a), 0.55),
                match_fg: hex(0x1d1d1f),
                radius: 6,
                window_radius: 12,
                row_height: 26.0,
                body_size: 13.0,
                small_size: 11.0,
                bevel: false,
                chrome: Chrome::Headerbar,
            },
            Look::MacDark => Palette {
                dark: true,
                bar: hex(0x2c2c2e),
                page: hex(0x1e1e1e),
                stripe: Some(hex(0x252526)),
                raised: hex(0x3a3a3c),
                hairline: a(hex(0xffffff), 0.10),
                text: hex(0xf5f5f7),
                text2: hex(0xc7c7cc),
                muted: hex(0x98989d),
                accent: hex(0x0a84ff),
                on_accent: hex(0xffffff),
                sel_bg: hex(0x0058d0),
                sel_fg: hex(0xffffff),
                sel_marker: false,
                match_bg: a(hex(0xffd60a), 0.35),
                match_fg: hex(0xffe58a),
                radius: 6,
                window_radius: 12,
                row_height: 26.0,
                body_size: 13.0,
                small_size: 11.0,
                bevel: false,
                chrome: Chrome::Headerbar,
            },
            Look::Classic => {
                let v = egui::Visuals::dark();
                Palette {
                    dark: true,
                    bar: v.panel_fill,
                    page: v.panel_fill,
                    stripe: Some(v.faint_bg_color),
                    raised: v.extreme_bg_color,
                    hairline: v.widgets.noninteractive.bg_stroke.color,
                    text: v.text_color(),
                    text2: v.widgets.inactive.fg_stroke.color,
                    muted: v.weak_text_color(),
                    accent: v.selection.bg_fill,
                    on_accent: Color32::WHITE,
                    sel_bg: v.selection.bg_fill,
                    sel_fg: Color32::WHITE,
                    sel_marker: false,
                    match_bg: v.selection.bg_fill.gamma_multiply(0.35),
                    match_fg: v.strong_text_color(),
                    radius: 2,
                    window_radius: 6,
                    row_height: 26.0,
                    body_size: 14.0,
                    small_size: 12.0,
                    bevel: false,
                    chrome: Chrome::Menubar,
                }
            }
        }
    }

    /// Font files to try, first found wins: (proportional, monospace). Missing ones fall back to
    /// egui's bundled fonts.
    fn font_candidates(self) -> (&'static [&'static str], &'static [&'static str]) {
        const SANS: &[&str] = &[
            "/usr/share/fonts/truetype/inter/Inter-Regular.ttf",
            "/usr/share/fonts/opentype/inter/Inter-Regular.otf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "C:\\Windows\\Fonts\\segoeui.ttf",
        ];
        const MONO: &[&str] = &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
            "C:\\Windows\\Fonts\\consola.ttf",
        ];
        const UBUNTU: &[&str] = &[
            "/usr/share/fonts/truetype/ubuntu/UbuntuSans[wdth,wght].ttf",
            "/usr/share/fonts/truetype/ubuntu/Ubuntu[wdth,wght].ttf",
            "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "C:\\Windows\\Fonts\\segoeui.ttf",
        ];
        const UBUNTU_MONO: &[&str] = &[
            "/usr/share/fonts/truetype/ubuntu/UbuntuSansMono[wght].ttf",
            "/usr/share/fonts/truetype/ubuntu/UbuntuMono[wght].ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "C:\\Windows\\Fonts\\consola.ttf",
        ];
        const WIN95: &[&str] = &[
            "C:\\Windows\\Fonts\\micross.ttf",
            "C:\\Windows\\Fonts\\tahoma.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ];
        const WIN95_MONO: &[&str] = &[
            "C:\\Windows\\Fonts\\cour.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        ];
        match self {
            Look::ModernDark | Look::ModernLight | Look::MacLight | Look::MacDark => (SANS, MONO),
            Look::UbuntuLight | Look::UbuntuDark => (UBUNTU, UBUNTU_MONO),
            Look::Win95 => (WIN95, WIN95_MONO),
            Look::Classic => (&[], &[]),
        }
    }
}

fn first_font(paths: &[&str]) -> Option<(String, Vec<u8>)> {
    paths
        .iter()
        .find_map(|p| std::fs::read(p).ok().map(|b| (p.to_string(), b)))
}

/// CJK and other wide-coverage fallbacks, so names in any script render (egui's bundled fonts
/// cover Latin, Greek and Cyrillic only). Read in the background: the files are large.
pub fn load_fallback_fonts() -> Vec<(String, Vec<u8>, u32)> {
    const CANDIDATES: &[(&str, u32)] = &[
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
        ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 0),
        (
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            0,
        ),
        ("C:\\Windows\\Fonts\\msyh.ttc", 0),
        ("C:\\Windows\\Fonts\\meiryo.ttc", 0),
        ("C:\\Windows\\Fonts\\seguisym.ttf", 0),
    ];
    let mut out = Vec::new();
    for (p, idx) in CANDIDATES {
        if let Ok(b) = std::fs::read(p) {
            out.push((p.to_string(), b, *idx));
        }
    }
    out
}

/// Fonts for a look, plus already loaded fallbacks.
pub fn fonts(look: Look, fallbacks: &[(String, Arc<FontData>)]) -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let (sans, mono) = look.font_candidates();
    if let Some((name, bytes)) = first_font(sans) {
        defs.font_data
            .insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
        if let Some(f) = defs.families.get_mut(&FontFamily::Proportional) {
            f.insert(0, name);
        }
    }
    if let Some((name, bytes)) = first_font(mono) {
        defs.font_data
            .insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
        if let Some(f) = defs.families.get_mut(&FontFamily::Monospace) {
            f.insert(0, name);
        }
    }
    for (name, data) in fallbacks {
        defs.font_data.insert(name.clone(), data.clone());
        for fam in [FontFamily::Proportional, FontFamily::Monospace] {
            if let Some(f) = defs.families.get_mut(&fam) {
                f.push(name.clone());
            }
        }
    }
    defs
}

/// Apply a look's palette to egui's style.
pub fn apply(ctx: &egui::Context, look: Look) {
    let p = look.palette();
    if look == Look::Classic {
        ctx.set_visuals(egui::Visuals::dark());
        ctx.all_styles_mut(|s| {
            s.text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        });
        return;
    }
    let mut v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let r = CornerRadius::same(p.radius);
    v.panel_fill = p.page;
    v.window_fill = if p.bevel { p.bar } else { p.page };
    v.extreme_bg_color = p.raised;
    v.faint_bg_color = p.stripe.unwrap_or(p.page);
    v.code_bg_color = p.raised;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.sel_bg;
    v.selection.stroke = Stroke::new(1.0, p.sel_fg);
    v.window_corner_radius = CornerRadius::same(p.window_radius);
    v.menu_corner_radius = CornerRadius::same(p.radius.saturating_add(2));
    v.window_stroke = Stroke::new(1.0, p.hairline);
    v.window_shadow = if p.bevel || p.dark {
        egui::Shadow::NONE
    } else {
        egui::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(18),
        }
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 60 } else { 20 }),
    };
    v.striped = p.stripe.is_some();
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    let control_fill = if p.bevel { p.bar } else { p.raised };
    let set = |w: &mut egui::style::WidgetVisuals, fill: Color32, stroke: Color32, fg: Color32| {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0, stroke);
        w.fg_stroke = Stroke::new(1.0, fg);
        w.corner_radius = r;
        w.expansion = 0.0;
    };
    let hover_fill = if p.bevel {
        p.bar
    } else if p.dark {
        p.raised.gamma_multiply(1.25)
    } else {
        p.bar
    };
    set(&mut v.widgets.noninteractive, p.page, p.hairline, p.text);
    set(
        &mut v.widgets.inactive,
        control_fill,
        if p.bevel { Color32::BLACK } else { p.hairline },
        p.text,
    );
    set(
        &mut v.widgets.hovered,
        hover_fill,
        if p.bevel {
            Color32::BLACK
        } else {
            p.text2.gamma_multiply(0.5)
        },
        p.text,
    );
    set(
        &mut v.widgets.active,
        if p.bevel { p.bar } else { p.sel_bg },
        p.accent,
        if p.bevel { p.text } else { p.sel_fg },
    );
    set(&mut v.widgets.open, hover_fill, p.hairline, p.text);
    if p.bevel {
        v.widgets.active.bg_stroke = Stroke::new(1.0, Color32::BLACK);
    }
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        use egui::{FontId, TextStyle};
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(p.body_size));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(p.body_size));
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(p.small_size));
        s.text_styles
            .insert(TextStyle::Monospace, FontId::monospace(p.small_size + 1.0));
        s.text_styles
            .insert(TextStyle::Heading, FontId::proportional(p.body_size + 6.0));
        s.spacing.item_spacing = Vec2::new(8.0, 4.0);
        s.spacing.button_padding = Vec2::new(12.0, 4.0);
        s.spacing.interact_size = Vec2::new(32.0, if p.bevel { 22.0 } else { 28.0 });
        s.spacing.menu_margin = Margin::same(6);
        s.spacing.window_margin = Margin::same(16);
        s.spacing.combo_width = 140.0;
        s.visuals.button_frame = true;
        s.interaction.selectable_labels = false;
    });
}
