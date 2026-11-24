// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! The few widgets every page is built from, so a tab is a list of fields and
//! not a hundred lines of layout.

use eframe::egui::{self, Button, Color32, Frame, Margin, Response, RichText, Stroke, Ui};

use super::theme::{Palette, bold};

/// A titled block with a rule under the heading.
pub fn section<R>(ui: &mut Ui, p: &Palette, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let inner =
        Frame::new().fill(p.section).corner_radius(5).inner_margin(Margin { left: 8, right: 8, top: 4, bottom: 6 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).font(bold(13.0)).color(p.text));
            let rect = ui.available_rect_before_wrap();
            ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.rule));
            ui.add_space(4.0);
            add_contents(ui)
        });
    ui.add_space(3.0);
    inner.inner
}

/// Grey explanatory text: why a setting exists. Wrapped, never cut short.
pub fn hint(ui: &mut Ui, p: &Palette, text: &str) {
    if !text.is_empty() {
        ui.add(egui::Label::new(RichText::new(text).color(p.subtitle).size(11.5)).wrap());
    }
}

/// A button in one of the three roles there is for one: "start" (green),
/// "stop" (red) or "normal".
pub fn action_button(ui: &mut Ui, p: &Palette, text: &str, kind: &str, width: f32, enabled: bool) -> Response {
    let (fill, colour) = match kind {
        "start" => (p.start, Color32::WHITE),
        "stop" => (p.stop, Color32::WHITE),
        _ => (p.tab_unselected, p.text),
    };
    let button =
        Button::new(RichText::new(text).font(bold(13.0)).color(colour)).fill(fill).corner_radius(6).min_size(egui::vec2(width, 24.0));
    ui.add_enabled(enabled, button)
}
