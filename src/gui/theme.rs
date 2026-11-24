// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Roch StressTest look: the palette Roch Viewer uses -- dark surfaces, grey
//! chrome, blue tabs, red kept for the thing you need to look at, which here is
//! a failure -- and a monospace face throughout, because these panels are full
//! of numbers that want to line up.

use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke, TextStyle, Visuals};

pub const BOLD: &str = "bold";

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color32,
    pub bg2: Color32,
    pub section: Color32,
    pub border: Color32,
    pub header: Color32,
    pub text: Color32,
    pub subtitle: Color32,
    pub value: Color32,
    pub rule: Color32,
    pub tab_selected: Color32,
    pub tab_unselected: Color32,
    pub tab_unselected_hover: Color32,
    pub running: Color32,
    pub pass: Color32,
    pub fail: Color32,
    pub warn: Color32,
    pub start: Color32,
    pub stop: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const LIGHT: Palette = Palette {
    bg: hex(0xF1F5F9),
    bg2: hex(0xFFFFFF),
    section: hex(0xE2E8F0),
    border: hex(0xCBD5E1),
    header: hex(0xE2E8F0),
    text: hex(0x0F172A),
    subtitle: hex(0x475569),
    value: hex(0xB91C1C),
    rule: hex(0x94A3B8),
    tab_selected: hex(0x2563EB),
    tab_unselected: hex(0xD7E1EC),
    tab_unselected_hover: hex(0xC5D2E0),
    running: hex(0x2563EB),
    pass: hex(0x15803D),
    fail: hex(0xB91C1C),
    warn: hex(0xB45309),
    start: hex(0x15803D),
    stop: hex(0xB91C1C),
};

pub const DARK: Palette = Palette {
    bg: hex(0x161616),
    bg2: hex(0x1C1C1C),
    section: hex(0x1C1C1C),
    border: hex(0x101010),
    header: hex(0x222222),
    text: hex(0xFFFFFF),
    subtitle: hex(0xB0B0B0),
    value: hex(0xFF4D4D),
    rule: hex(0x4A4A4A),
    tab_selected: hex(0x1A3C5D),
    tab_unselected: hex(0x2E2E2E),
    tab_unselected_hover: hex(0x3A3A3A),
    running: hex(0x6AA9FF),
    pass: hex(0x4ADE80),
    fail: hex(0xFF4D4D),
    warn: hex(0xFBBF24),
    start: hex(0x1F5F38),
    stop: hex(0x5D1A1A),
};

pub fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD.into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// The monospace face: Consolas on Windows, as Roch Viewer uses; on Linux
/// whatever fontconfig calls monospace. egui's own fonts stand in behind it.
fn font_files() -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    #[cfg(windows)]
    {
        let fonts =
            std::env::var_os("WINDIR").map(|dir| std::path::PathBuf::from(dir).join("Fonts")).unwrap_or_else(|| r"C:\Windows\Fonts".into());
        (std::fs::read(fonts.join("consola.ttf")).ok(), std::fs::read(fonts.join("consolab.ttf")).ok())
    }
    #[cfg(not(windows))]
    {
        let find = |pattern: &str| -> Option<Vec<u8>> {
            let output = std::process::Command::new("fc-match").args(["-f", "%{file}", pattern]).output().ok()?;
            let path = String::from_utf8(output.stdout).ok()?;
            // egui reads TrueType and OpenType outlines; a bitmap or WOFF face
            // would not draw.
            let lower = path.to_ascii_lowercase();
            if !(lower.ends_with(".ttf") || lower.ends_with(".otf")) {
                return None;
            }
            std::fs::read(path.trim()).ok()
        };
        (find("monospace"), find("monospace:bold"))
    }
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let (regular, heavy) = font_files();
    let defaults: Vec<String> = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mut base: Vec<String> = Vec::new();
    if let Some(bytes) = regular {
        fonts.font_data.insert("mono".into(), Arc::new(FontData::from_owned(bytes)));
        base.push("mono".into());
    }
    base.extend(defaults);
    let mut heavy_family = Vec::new();
    if let Some(bytes) = heavy {
        fonts.font_data.insert("mono-bold".into(), Arc::new(FontData::from_owned(bytes)));
        heavy_family.push("mono-bold".to_string());
    }
    heavy_family.extend(base.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, base.clone());
    fonts.families.insert(FontFamily::Monospace, base);
    fonts.families.insert(FontFamily::Name(BOLD.into()), heavy_family);
    ctx.set_fonts(fonts);
}

/// Style every widget for the given theme.
pub fn apply(ctx: &egui::Context, dark: bool) {
    let p = palette(dark);
    let mut visuals = if dark { Visuals::dark() } else { Visuals::light() };
    visuals.override_text_color = Some(p.text);
    visuals.window_fill = p.bg2;
    visuals.panel_fill = p.bg;
    visuals.extreme_bg_color = p.bg2;
    visuals.faint_bg_color = p.section;
    visuals.window_stroke = Stroke::new(1.0, p.rule);
    visuals.window_corner_radius = CornerRadius::same(6);
    visuals.menu_corner_radius = CornerRadius::same(4);
    visuals.hyperlink_color = p.value;
    visuals.warn_fg_color = p.warn;
    visuals.error_fg_color = p.fail;
    visuals.selection.bg_fill = p.tab_selected;
    visuals.selection.stroke = Stroke::new(1.0, p.text);
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;

    let radius = CornerRadius::same(5);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = p.section;
    widgets.noninteractive.weak_bg_fill = p.section;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.rule);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    for (state, fill, stroke) in [
        (&mut widgets.inactive, p.tab_unselected, p.border),
        (&mut widgets.hovered, p.tab_unselected_hover, p.rule),
        (&mut widgets.active, p.tab_selected, p.tab_selected),
        (&mut widgets.open, p.tab_unselected_hover, p.border),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, stroke);
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, regular(11.0)),
            (TextStyle::Body, regular(13.0)),
            (TextStyle::Monospace, regular(12.0)),
            (TextStyle::Button, bold(13.0)),
            (TextStyle::Heading, bold(13.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(10.0, 3.0);
        style.spacing.interact_size.y = 24.0;
        style.spacing.combo_height = 300.0;
        style.spacing.window_margin = Margin::same(10);
        style.spacing.text_edit_width = 130.0;
    });
}
