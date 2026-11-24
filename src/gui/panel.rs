// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! One tool's tab: what it does, a profile or preset picker, its settings, and
//! Start. It opens on the same defaults as the tool's Quick Start card, so the
//! two never disagree about what "default" means.

use std::path::{Path, PathBuf};

use eframe::egui::{self, ComboBox, Grid, RichText, TextEdit, Ui};
use serde_json::{Map, Value};

use roch_stresstest::config::{Config, as_int};
use roch_stresstest::tool::{Field, Kind, Preset, Tool};
use roch_stresstest::tools::linpack::{self, TEST_KEYS};

use super::theme::{Palette, bold};
use super::widgets::{action_button, hint, section};

const NEW_PROFILE: &str = "New profile";

/// The width of the label column, so every control starts at the same place.
const LABEL_WIDTH: f32 = 160.0;

/// What a row's control holds.
enum Control {
    Text(String),
    Flag(bool),
    Choice(String),
    Multi(Vec<bool>),
}

pub struct Panel {
    fields: Vec<Field>,
    values: Vec<Control>,
    /// Values with no row: quick-start keys the tool has no field for.
    hidden: Config,
    presets: Vec<Preset>,
    preset: usize,
    /// Some for a tool that keeps profiles as files.
    profiles: Option<Vec<(String, PathBuf)>>,
    /// The picked profile, or None for "New profile".
    profile: Option<usize>,
    save_name: String,
    /// Linpack Extended's chain of tests: minutes, problem size, leading
    /// dimension, alignment, as typed.
    tests: Vec<[String; 4]>,
    add_gb: String,
    pub unsupported: String,
}

/// What the tab asks the window to do.
pub enum Action {
    None,
    Start(Config, String),
    Log(String),
    /// A profile was saved or deleted: Quick Start's buttons change.
    ProfilesChanged(String),
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

impl Panel {
    pub fn new(tool: &dyn Tool, root: &Path) -> Panel {
        let fields = tool.fields();
        let values = fields.iter().map(|field| Panel::control_for(field, &field.default)).collect();
        let mut panel = Panel {
            fields,
            values,
            hidden: Config::new(),
            presets: tool.presets(root),
            preset: 0,
            profiles: tool.profiles(root),
            profile: None,
            save_name: String::new(),
            tests: Vec::new(),
            add_gb: "11".into(),
            unsupported: tool.unsupported_reason(root),
        };
        panel.apply_quick_start(tool, root);
        panel
    }

    fn control_for(field: &Field, value: &Value) -> Control {
        match field.kind {
            Kind::Bool => Control::Flag(matches!(field.coerce(value), Value::Bool(true))),
            Kind::Choice => Control::Choice(value_text(&field.coerce(value))),
            Kind::Multi => {
                let chosen = value_text(&field.coerce(value));
                let chosen: Vec<&str> = chosen.split(' ').collect();
                Control::Multi(field.choices.iter().map(|choice| chosen.contains(&choice.as_str())).collect())
            }
            Kind::Int | Kind::Text => Control::Text(value_text(value)),
        }
    }

    /// Put values into the boxes; keys with no box are kept aside.
    pub fn load_values(&mut self, values: &Config) {
        for (key, value) in values {
            if key == "tests" {
                self.set_tests(value);
                continue;
            }
            match self.fields.iter().position(|field| field.key == key) {
                Some(index) => self.values[index] = Panel::control_for(&self.fields[index], value),
                None => {
                    self.hidden.insert(key.clone(), value.clone());
                }
            }
        }
    }

    fn set_tests(&mut self, value: &Value) {
        let tests: Vec<Map<String, Value>> =
            value.as_array().map(|tests| tests.iter().filter_map(|test| test.as_object().cloned()).collect()).unwrap_or_default();
        let tests = if tests.is_empty() { vec![linpack::default_test()] } else { tests };
        self.tests = tests
            .iter()
            .map(|test| TEST_KEYS.map(|(key, default)| test.get(key).and_then(as_int).unwrap_or(default).to_string()))
            .collect();
    }

    fn tests_value(&self) -> Value {
        Value::Array(
            self.tests
                .iter()
                .map(|row| {
                    let mut test = Map::new();
                    for ((key, _), text) in TEST_KEYS.iter().zip(row) {
                        test.insert(key.to_string(), Value::from(text.trim().parse::<i64>().unwrap_or(0)));
                    }
                    Value::Object(test)
                })
                .collect(),
        )
    }

    /// The tab's settings, as `build` takes them.
    pub fn config(&self, tool: &dyn Tool) -> Config {
        let mut config = self.hidden.clone();
        for (field, value) in self.fields.iter().zip(&self.values) {
            let raw = match value {
                Control::Text(text) | Control::Choice(text) => Value::String(text.clone()),
                Control::Flag(flag) => Value::Bool(*flag),
                Control::Multi(ticks) => Value::String(
                    field
                        .choices
                        .iter()
                        .zip(ticks)
                        .filter(|(_, ticked)| **ticked)
                        .map(|(choice, _)| choice.clone())
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
            };
            config.insert(field.key.to_string(), field.coerce(&raw));
        }
        if tool.has_test_chain() {
            config.insert("tests".into(), self.tests_value());
        }
        config
    }

    /// Open on the tool's default configuration, the one Quick Start runs.
    fn apply_quick_start(&mut self, tool: &dyn Tool, root: &Path) {
        if tool.has_test_chain() {
            let name = roch_stresstest::tools::linpack::LinpackExtended.quick_profile_name(root);
            if let Some(index) = self.profiles.as_ref().and_then(|profiles| profiles.iter().position(|(candidate, _)| *candidate == name)) {
                self.profile = Some(index);
                self.save_name = name;
            }
            self.load_values(&tool.quick_config(root));
            return;
        }
        let name = tool.quick_preset_name(root);
        if let Some(index) = self.presets.iter().position(|preset| preset.name == name) {
            self.preset = index;
            let values = self.presets[index].values.clone();
            self.load_values(&values);
        }
        self.load_values(&tool.quick_values());
    }

    fn apply_preset(&mut self, index: usize) {
        self.preset = index;
        let values = self.presets[index].values.clone();
        // A preset with no values of its own ("Custom") leaves the boxes alone.
        if !values.is_empty() {
            self.load_values(&values);
        }
    }

    /// The run's name in the log and the status strip.
    pub fn label(&self, tool: &dyn Tool) -> String {
        if let (Some(index), Some(profiles)) = (self.profile, &self.profiles)
            && let Some((name, _)) = profiles.get(index)
        {
            return format!("{} -- {name}", tool.name());
        }
        match self.presets.get(self.preset) {
            Some(preset) => format!("{} -- {}", tool.name(), preset.name),
            None => tool.name().to_string(),
        }
    }

    pub fn show(&mut self, ui: &mut Ui, p: &Palette, tool: &dyn Tool, root: &Path, busy: bool) -> Action {
        let mut action = Action::None;
        let bar_height = 44.0;
        let height = (ui.available_height() - bar_height).max(80.0);
        egui::ScrollArea::vertical().id_salt(("tab", tool.key())).max_height(height).auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(tool.blurb()).color(p.subtitle)).wrap());
            ui.add_space(6.0);
            if !self.unsupported.is_empty() {
                section(ui, p, "Not for this processor", |ui| {
                    ui.add(egui::Label::new(RichText::new(&self.unsupported).color(p.warn)).wrap());
                });
            }
            if self.profiles.is_some() {
                action = self.profile_section(ui, p, tool, root);
            }
            if tool.has_test_chain() {
                self.tests_section(ui, p);
            }
            if !self.presets.is_empty() {
                self.preset_section(ui, p);
            }
            self.settings_section(ui, p);
            if let Some(title) = tool.profile_preview_label() {
                let preview = tool.profile_preview(&self.config(tool));
                section(ui, p, title, |ui| {
                    ui.label(RichText::new(preview).font(super::theme::regular(12.0)));
                });
            }
        });
        egui::Frame::new().fill(p.section).corner_radius(6).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let enabled = self.unsupported.is_empty() && !busy;
                if action_button(ui, p, "Start", "start", 120.0, enabled).clicked() {
                    action = Action::Start(self.config(tool), self.label(tool));
                }
                if busy {
                    ui.label(RichText::new("A test is running. Stop it first.").color(p.subtitle));
                }
            });
        });
        action
    }

    fn profile_section(&mut self, ui: &mut Ui, p: &Palette, tool: &dyn Tool, root: &Path) -> Action {
        let mut action = Action::None;
        let profiles = self.profiles.clone().unwrap_or_default();
        section(ui, p, "Profile", |ui| {
            Grid::new(("profile", tool.key())).num_columns(2).min_col_width(LABEL_WIDTH - 8.0).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label("Profile");
                let shown =
                    self.profile.and_then(|index| profiles.get(index)).map(|(name, _)| name.clone()).unwrap_or_else(|| NEW_PROFILE.into());
                let mut picked: Option<Option<usize>> = None;
                ComboBox::from_id_salt(("profile-pick", tool.key())).selected_text(shown).width(220.0).show_ui(ui, |ui| {
                    if ui.selectable_label(self.profile.is_none(), NEW_PROFILE).clicked() {
                        picked = Some(None);
                    }
                    for (index, (name, _)) in profiles.iter().enumerate() {
                        if ui.selectable_label(self.profile == Some(index), name).clicked() {
                            picked = Some(Some(index));
                        }
                    }
                });
                ui.end_row();
                if let Some(choice) = picked {
                    self.profile = choice;
                    match choice.and_then(|index| profiles.get(index)) {
                        None => self.save_name.clear(),
                        Some((name, path)) => {
                            self.save_name = name.clone();
                            match tool.profile_values(path) {
                                Ok(values) => self.load_values(&values),
                                Err(error) => action = Action::Log(format!("Could not read {}: {error}", path.display())),
                            }
                        }
                    }
                }

                ui.label("Save as");
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut self.save_name).desired_width(190.0).hint_text("name"));
                    if action_button(ui, p, "Save", "normal", 70.0, true).clicked() {
                        action = self.save(tool, root);
                    }
                    let can_delete = self.profile.is_some();
                    if action_button(ui, p, "Delete", "normal", 70.0, can_delete).clicked() {
                        action = self.delete(tool, root);
                    }
                });
                ui.end_row();
            });
            hint(ui, p, &tool.profile_hint());
            if tool.profile_preview_label().is_none() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Command line").color(p.subtitle));
                    ui.label(RichText::new(tool.profile_preview(&self.config(tool))).font(super::theme::regular(12.0)));
                });
            }
        });
        action
    }

    fn refresh_profiles(&mut self, tool: &dyn Tool, root: &Path, selected: &str) {
        self.profiles = tool.profiles(root);
        self.profile = self.profiles.as_ref().and_then(|profiles| profiles.iter().position(|(name, _)| name == selected));
    }

    fn save(&mut self, tool: &dyn Tool, root: &Path) -> Action {
        let name = roch_stresstest::tool::clean_profile_name(&self.save_name);
        if name.is_empty() {
            return Action::Log("Give the profile a name first.".into());
        }
        match tool.save_profile(root, &name, &self.config(tool)) {
            Ok(path) => {
                self.save_name = name.clone();
                self.refresh_profiles(tool, root, &name);
                Action::ProfilesChanged(format!("Saved {} profile {}", tool.name(), path.display()))
            }
            Err(error) => Action::Log(format!("Could not save the profile: {error}")),
        }
    }

    fn delete(&mut self, tool: &dyn Tool, root: &Path) -> Action {
        let Some((_, path)) = self.profile.and_then(|index| self.profiles.as_ref()?.get(index).cloned()) else { return Action::None };
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.save_name.clear();
                self.refresh_profiles(tool, root, "");
                Action::ProfilesChanged(format!("Deleted {} profile {}", tool.name(), path.display()))
            }
            Err(error) => Action::Log(format!("Could not delete {}: {error}", path.display())),
        }
    }

    fn preset_section(&mut self, ui: &mut Ui, p: &Palette) {
        section(ui, p, "What to run", |ui| {
            let mut picked = None;
            Grid::new("preset").num_columns(2).min_col_width(LABEL_WIDTH - 8.0).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label("Preset");
                ComboBox::from_id_salt("preset-pick").selected_text(self.presets[self.preset].name.clone()).width(190.0).show_ui(
                    ui,
                    |ui| {
                        for (index, preset) in self.presets.iter().enumerate() {
                            if ui.selectable_label(index == self.preset, &preset.name).clicked() {
                                picked = Some(index);
                            }
                        }
                    },
                );
                ui.end_row();
            });
            if let Some(index) = picked {
                self.apply_preset(index);
            }
            hint(ui, p, &self.presets[self.preset].description);
        });
    }

    fn settings_section(&mut self, ui: &mut Ui, p: &Palette) {
        section(ui, p, "Settings", |ui| {
            for (field, value) in self.fields.iter().zip(self.values.iter_mut()) {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(egui::vec2(LABEL_WIDTH, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_min_width(LABEL_WIDTH);
                        ui.label(field.label);
                    });
                    match value {
                        Control::Text(text) => {
                            let width = if field.kind == Kind::Text { 320.0 } else { 130.0 };
                            ui.add(TextEdit::singleline(text).desired_width(width));
                        }
                        Control::Flag(flag) => {
                            ui.checkbox(flag, "");
                        }
                        Control::Choice(choice) => {
                            ComboBox::from_id_salt(("field", field.key)).selected_text(choice.clone()).width(190.0).show_ui(ui, |ui| {
                                for option in &field.choices {
                                    ui.selectable_value(choice, option.clone(), option);
                                }
                            });
                        }
                        Control::Multi(ticks) => {
                            Grid::new(("multi", field.key)).num_columns(4).spacing([18.0, 2.0]).show(ui, |ui| {
                                for (index, (choice, ticked)) in field.choices.iter().zip(ticks.iter_mut()).enumerate() {
                                    ui.checkbox(ticked, choice);
                                    if index % 4 == 3 {
                                        ui.end_row();
                                    }
                                }
                            });
                        }
                    }
                    if !field.unit.is_empty() {
                        ui.allocate_ui_with_layout(egui::vec2(40.0, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.label(RichText::new(field.unit).color(p.subtitle));
                        });
                    }
                });
                if !field.hint.is_empty() {
                    ui.horizontal_top(|ui| {
                        ui.add_space(LABEL_WIDTH + ui.spacing().item_spacing.x);
                        ui.vertical(|ui| {
                            ui.set_max_width(560.0);
                            hint(ui, p, &field.hint);
                        });
                    });
                }
                ui.add_space(2.0);
            }
        });
    }

    fn tests_section(&mut self, ui: &mut Ui, p: &Palette) {
        section(ui, p, "Tests", |ui| {
            let mut remove = None;
            let mut raise = None;
            Grid::new("tests").num_columns(8).spacing([8.0, 3.0]).show(ui, |ui| {
                ui.label("");
                for title in ["Minutes", "Problem size", "Leading dim", "Alignment", "Memory"] {
                    ui.label(RichText::new(title).color(p.subtitle));
                }
                ui.end_row();
                let count = self.tests.len();
                for (index, row) in self.tests.iter_mut().enumerate() {
                    ui.label(RichText::new((index + 1).to_string()).font(bold(13.0)));
                    for text in row.iter_mut() {
                        ui.add(TextEdit::singleline(text).desired_width(100.0));
                    }
                    let test: Map<String, Value> = TEST_KEYS
                        .iter()
                        .zip(row.iter())
                        .map(|((key, _), text)| (key.to_string(), Value::from(text.trim().parse::<i64>().unwrap_or(0))))
                        .collect();
                    ui.label(RichText::new(format!("{:.1} GB", linpack::test_memory_gb(&linpack::clean_test(&test)))).color(p.subtitle));
                    if index > 0 && action_button(ui, p, "Up", "normal", 40.0, true).clicked() {
                        raise = Some(index);
                    } else if index == 0 {
                        ui.label("");
                    }
                    if action_button(ui, p, "Remove", "normal", 70.0, count > 1).clicked() {
                        remove = Some(index);
                    }
                    ui.end_row();
                }
            });
            if let Some(index) = raise {
                self.tests.swap(index - 1, index);
            }
            if let Some(index) = remove {
                self.tests.remove(index);
            }
            ui.horizontal(|ui| {
                ui.label("Add a test sized for");
                ui.add(TextEdit::singleline(&mut self.add_gb).desired_width(50.0));
                ui.label(RichText::new("GB").color(p.subtitle));
                if action_button(ui, p, "Add test", "normal", 80.0, true).clicked() {
                    let gigabytes: f64 = self.add_gb.trim().parse().unwrap_or(11.0);
                    let size = linpack::problem_size_for(gigabytes.max(0.1) * 1024.0);
                    self.tests.push(["30".into(), size.to_string(), linpack::leading_dimension(size).to_string(), "4".into()]);
                }
            });
            hint(
                ui,
                p,
                "They run top to bottom. A leading dimension smaller than the problem size is replaced with Intel's recommended one.",
            );
        });
    }
}
