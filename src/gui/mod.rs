// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window: Quick Start, a tab per tool that has settings worth offering,
//! the log, and the status strip.
//!
//! The runner works on its own thread and reports through a channel that is
//! drained here every frame; that boundary is the only threading rule in the
//! program.

mod panel;
pub mod theme;
mod widgets;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ComboBox, Frame, Margin, RichText, Ui};

use roch_stresstest::config::Config;
use roch_stresstest::runner::{Event, Runner, State};
use roch_stresstest::tool::Tool;
use roch_stresstest::{APP_NAME, VERSION, hardware, memory, settings, tools};

use panel::{Action, Panel};
use theme::{Palette, bold, palette};
use widgets::{action_button, hint, section};

/// The log keeps this many lines; a twelve-hour run produces far more, and only
/// the tail is ever read.
const LOG_LIMIT: usize = 4000;

const SOCIAL_LINKS: [(&str, &str); 3] =
    [("YouTube", "https://www.youtube.com/@MateoPcTech"), ("X", "https://x.com/1xroch"), ("Discord", "https://discord.gg/KfzExpKQHB")];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Quick,
    Tool(usize),
    Log,
}

/// A Quick Start card, worked out once: describing a tool reads its folder,
/// which is not something to do every frame.
struct Card {
    tool: usize,
    headline: String,
    tone: Tone,
    note: String,
    actions: Vec<(String, Config)>,
    enabled: bool,
}

#[derive(Clone, Copy)]
enum Tone {
    Normal,
    Warn,
    Fail,
}

pub struct App {
    tools: Vec<Box<dyn Tool>>,
    root: PathBuf,
    available: Vec<bool>,
    panels: Vec<Option<Panel>>,
    cards: Vec<Card>,
    runner: Runner,
    dark: bool,
    tab: Tab,
    log: VecDeque<String>,
    state: State,
    detail: String,
    clock: String,
    stat: String,
    current_label: String,
    cpu: String,
    ram: (u64, u64, u64),
    ram_read: Instant,
}

impl App {
    pub fn new(ctx: &egui::Context, elevated_note: Option<&str>, tab: &str) -> App {
        theme::install_fonts(ctx);
        let dark = settings::load_dark_mode();
        theme::apply(ctx, dark);
        let tools = tools::tools();
        let root = tools::tools_root();
        let available: Vec<bool> = tools.iter().map(|tool| tool.available(&root)).collect();
        let panels = tools
            .iter()
            .zip(&available)
            .map(|(tool, available)| (tool.has_tab() && *available).then(|| Panel::new(tool.as_ref(), &root)))
            .collect();
        let mut app = App {
            tools,
            root,
            available,
            panels,
            cards: Vec::new(),
            runner: Runner::new(),
            dark,
            tab: Tab::Quick,
            log: VecDeque::new(),
            state: State::Idle,
            detail: "Nothing running.".into(),
            clock: String::new(),
            stat: String::new(),
            current_label: String::new(),
            cpu: hardware::describe_cpu(),
            ram: memory::reading(),
            ram_read: Instant::now(),
        };
        app.refresh_cards();
        if tab.eq_ignore_ascii_case("log") {
            app.tab = Tab::Log;
        } else if let Some(index) = app.tools.iter().position(|tool| tool.has_tab() && tool.name().eq_ignore_ascii_case(tab)) {
            app.tab = Tab::Tool(index);
        }
        app.log(&format!("{APP_NAME} {VERSION}"));
        app.log(&hardware::describe());
        for (tool, available) in app.tools.iter().zip(&app.available) {
            if !available {
                app.log.push_back(format!("Not found: {} (its tab or card explains where to unpack it)", tool.name()));
            }
        }
        if let Some(note) = elevated_note {
            app.log(note);
        }
        app
    }

    fn refresh_cards(&mut self) {
        let columns = tools::quick_columns(&self.tools);
        self.cards = columns
            .iter()
            .flatten()
            .map(|&index| {
                let tool = &self.tools[index];
                let available = self.available[index];
                let blocked = if available { tool.unsupported_reason(&self.root) } else { String::new() };
                let (headline, tone) = if !available && !tool.has_tab() {
                    ("Not found -- unpack it into the program folder.".to_string(), Tone::Fail)
                } else if !available {
                    (format!("Not found -- see the {} tab.", tool.name()), Tone::Fail)
                } else if !blocked.is_empty() {
                    ("Not for this processor".to_string(), Tone::Warn)
                } else {
                    (tool.quick_summary(&self.root), Tone::Normal)
                };
                let note = if !blocked.is_empty() {
                    blocked.clone()
                } else if !available && !tool.has_tab() {
                    self.root.display().to_string()
                } else {
                    String::new()
                };
                let enabled = available && blocked.is_empty();
                let actions = if enabled { tool.quick_actions(&self.root) } else { vec![("Start".into(), Config::new())] };
                Card { tool: index, headline, tone, note, actions, enabled }
            })
            .collect();
    }

    fn palette(&self) -> &'static Palette {
        palette(self.dark)
    }

    fn log(&mut self, text: &str) {
        for line in text.lines() {
            self.log.push_back(line.to_string());
        }
        if text.is_empty() {
            self.log.push_back(String::new());
        }
        while self.log.len() > LOG_LIMIT {
            self.log.pop_front();
        }
    }

    fn busy(&self) -> bool {
        self.runner.running()
    }

    fn start(&mut self, index: usize, config: &Config, label: &str) {
        if self.busy() {
            self.log("Already running. Stop the current test first.");
            return;
        }
        let tool = &self.tools[index];
        let name = tool.name();
        let spec = match tool.build(config, &self.root) {
            Ok(spec) => spec,
            Err(error) => {
                self.log(&format!("Could not start {name}: {error}"));
                self.set_state(State::Broken, &error);
                return;
            }
        };
        self.current_label = label.to_string();
        self.log("");
        self.log(&format!("=== {label} ==="));
        self.log(&spec.summary);
        self.log(&format!("Command: {}", spec.command_text()));
        if spec.duration_seconds > 0 {
            self.log(&format!("Time limit: {}", format_duration(spec.duration_seconds)));
        } else {
            self.log("No time limit -- runs until you press Stop.");
        }
        if let Err(error) = self.runner.start(spec, label) {
            self.log(&format!("Could not start the process: {error}"));
            self.set_state(State::Broken, &error);
        }
    }

    fn set_state(&mut self, state: State, note: &str) {
        self.state = state;
        self.detail = if !note.is_empty() {
            note.to_string()
        } else if !self.current_label.is_empty() {
            self.current_label.clone()
        } else {
            "Nothing running.".into()
        };
    }

    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.runner.events.try_recv() {
            match event {
                Event::Output(line) => self.log(&format!("  {line}")),
                Event::Stat(text) => self.stat = text,
                Event::Tick { elapsed, remaining } => {
                    self.clock = format_duration(elapsed);
                    if let Some(remaining) = remaining {
                        self.clock += &format!("  /  {} left", format_duration(remaining));
                    }
                    if !self.stat.is_empty() {
                        self.detail = format!("{}   {}", self.current_label, self.stat);
                    }
                }
                Event::State(state, note) => self.on_state(ctx, state, &note),
            }
        }
    }

    fn on_state(&mut self, ctx: &egui::Context, state: State, note: &str) {
        self.set_state(state, note);
        if state == State::Running {
            self.stat.clear();
            return;
        }
        self.log(&format!("{}: {note}", state.text().to_uppercase()));
        if !self.stat.is_empty() {
            let stat = self.stat.clone();
            self.log(&format!("  {stat}"));
        }
        if state == State::Failed {
            self.announce_failure(ctx, note);
        }
        self.clock.clear();
    }

    /// Make a failure impossible to miss, and keep a copy of it: the run that
    /// matters is the one that failed at 4am, and the window may be gone by the
    /// time anybody looks.
    fn announce_failure(&mut self, ctx: &egui::Context, note: &str) {
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Critical));
        #[cfg(windows)]
        // SAFETY: MessageBeep only plays a sound.
        unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(0x10);
        }
        let path = settings::log_dir().join(chrono::Local::now().format("FAILED-%Y%m%d-%H%M%S.txt").to_string());
        let mut text = format!("{APP_NAME} {VERSION}\n{}\n{}\n{note}\n\n", hardware::describe(), self.current_label);
        for line in &self.log {
            text.push_str(line);
            text.push('\n');
        }
        if std::fs::write(&path, text).is_ok() {
            self.log(&format!("Transcript of the failure saved to {}", path.display()));
        }
        self.tab = Tab::Log;
    }

    fn save_log(&mut self) {
        let path = settings::log_dir().join(chrono::Local::now().format("stress-%Y%m%d-%H%M%S.txt").to_string());
        let text: String = self.log.iter().map(|line| format!("{line}\n")).collect();
        match std::fs::write(&path, text) {
            Ok(()) => self.log(&format!("Transcript saved to {}", path.display())),
            Err(error) => self.log(&format!("Could not save the transcript: {error}")),
        }
    }

    // ----- drawing --------------------------------------------------------------

    fn toolbar(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let p = self.palette();
        ui.horizontal_centered(|ui| {
            ui.label(RichText::new(&self.cpu).color(p.subtitle));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut dark = self.dark;
                ComboBox::from_id_salt("appearance").selected_text(if dark { "Dark" } else { "Light" }).width(70.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut dark, true, "Dark");
                    ui.selectable_value(&mut dark, false, "Light");
                });
                if dark != self.dark {
                    self.dark = dark;
                    theme::apply(ctx, dark);
                    settings::save_dark_mode(dark);
                }
                // Windows only: Linux already counts its cache as available.
                if memory::SUPPORTED && action_button(ui, p, "Clean memory", "normal", 120.0, true).clicked() {
                    let result = memory::clean();
                    self.log(&result.describe());
                    if result.done.is_empty() && !result.failed.is_empty() {
                        self.log("  Memory cleaning needs administrator rights, which this program normally asks for at launch.");
                    }
                    self.ram = memory::reading();
                }
                let (available, total, used) = self.ram;
                let colour = if used >= 90 {
                    p.fail
                } else if used >= 75 {
                    p.warn
                } else {
                    p.text
                };
                ui.label(
                    RichText::new(format!("RAM {} / {} MB free", memory::group(available), memory::group(total)))
                        .font(bold(13.0))
                        .color(colour),
                );
            });
        });
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
        let p = self.palette();
        ui.horizontal_wrapped(|ui| {
            let mut entries: Vec<(Tab, String)> = vec![(Tab::Quick, "Quick Start".into())];
            for (index, tool) in self.tools.iter().enumerate() {
                if tool.has_tab() {
                    entries.push((Tab::Tool(index), tool.name().into()));
                }
            }
            entries.push((Tab::Log, "Log".into()));
            for (tab, title) in entries {
                let selected = self.tab == tab;
                let fill = if selected { p.tab_selected } else { p.tab_unselected };
                let colour = if selected && !self.dark { Color32::WHITE } else { p.text };
                let button = egui::Button::new(RichText::new(title).font(bold(13.0)).color(colour)).fill(fill).corner_radius(8);
                if ui.add(button).clicked() {
                    self.tab = tab;
                }
            }
        });
    }

    fn quick_tab(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let busy = self.busy();
        let mut clicked: Option<(usize, Config, String)> = None;
        let columns = tools::quick_columns(&self.tools);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.columns(2, |sides| {
                for (side, column) in sides.iter_mut().zip(columns.iter()) {
                    for index in column {
                        let Some(card) = self.cards.iter().find(|card| card.tool == *index) else { continue };
                        let tool = &self.tools[card.tool];
                        section(side, p, tool.name(), |ui| {
                            if !card.headline.is_empty() {
                                let colour = match card.tone {
                                    Tone::Normal => p.text,
                                    Tone::Warn => p.warn,
                                    Tone::Fail => p.fail,
                                };
                                ui.add(egui::Label::new(RichText::new(&card.headline).color(colour)).wrap());
                            }
                            hint(ui, p, &card.note);
                            // Four to a row: Cinebench has a button per version
                            // installed, six on a machine with the lot.
                            let width = if card.actions.len() == 1 { 110.0 } else { 88.0 };
                            for row in card.actions.chunks(4) {
                                ui.horizontal(|ui| {
                                    for (label, config) in row {
                                        if action_button(ui, p, label, "start", width, card.enabled && !busy).clicked() {
                                            clicked = Some((card.tool, config.clone(), format!("{} -- {label}", tool.name())));
                                        }
                                    }
                                });
                            }
                        });
                    }
                }
            });
        });
        if let Some((index, config, label)) = clicked {
            self.start(index, &config, &label);
        }
    }

    fn tool_tab(&mut self, ui: &mut Ui, index: usize) {
        let p = self.palette();
        let busy = self.busy();
        let tool = &self.tools[index];
        let Some(panel) = self.panels[index].as_mut() else {
            ui.add_space(16.0);
            ui.label(RichText::new(format!("{} was not found.", tool.name())).font(bold(13.0)).color(p.fail));
            ui.add_space(4.0);
            let globs = tool.exe_globs().join("\n  ");
            ui.label(RichText::new(format!("Unpack it into\n{}\n\nExpected one of:\n  {globs}", self.root.display())).color(p.subtitle));
            return;
        };
        let action = panel.show(ui, p, tool.as_ref(), &self.root, busy);
        match action {
            Action::None => {}
            Action::Log(text) => self.log(&text),
            Action::Start(config, label) => self.start(index, &config, &label),
            Action::ProfilesChanged(text) => {
                self.log(&text);
                self.refresh_cards();
            }
        }
    }

    fn log_tab(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let bar = 40.0;
        let height = (ui.available_height() - bar).max(60.0);
        Frame::new().fill(p.bg2).stroke(egui::Stroke::new(1.0, p.border)).inner_margin(Margin::same(6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
            egui::ScrollArea::both().max_height(height - 14.0).auto_shrink([false, false]).stick_to_bottom(true).show_rows(
                ui,
                row_height,
                self.log.len(),
                |ui, rows| {
                    for line in self.log.range(rows) {
                        ui.add(egui::Label::new(RichText::new(line).monospace()).extend());
                    }
                },
            );
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if action_button(ui, p, "Save transcript", "normal", 130.0, true).clicked() {
                self.save_log();
            }
            if action_button(ui, p, "Clear", "normal", 80.0, true).clicked() {
                self.log.clear();
            }
        });
    }

    fn status_strip(&mut self, ui: &mut Ui) {
        let p = self.palette();
        let colour = match self.state {
            State::Idle => p.subtitle,
            State::Running => p.running,
            State::Passed => p.pass,
            State::Failed | State::Broken => p.fail,
            State::Stopped => p.warn,
        };
        ui.horizontal_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(110.0, 20.0), egui::Sense::hover());
            ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, self.state.text(), bold(13.0), colour);
            if self.busy() && action_button(ui, p, "Stop", "stop", 70.0, true).clicked() {
                self.runner.stop();
                self.log("Stopping...");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(&self.clock).font(bold(13.0)));
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(&self.detail).truncate());
                });
            });
        });
    }

    fn links(&self, ui: &mut Ui) {
        let p = self.palette();
        ui.horizontal(|ui| {
            for (index, (text, url)) in SOCIAL_LINKS.iter().enumerate() {
                if index > 0 {
                    ui.label(RichText::new("|").font(bold(13.0)));
                }
                let link = ui.add(egui::Label::new(RichText::new(*text).font(bold(13.0)).color(p.value)).sense(egui::Sense::click()));
                if link.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    let _ = open::that(url);
                }
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pump(&ctx);
        // Never leave a stress test running with no window to stop it.
        if ctx.input(|input| input.viewport().close_requested()) && self.busy() {
            self.runner.stop_and_wait();
        }
        if self.ram_read.elapsed() >= Duration::from_secs(1) {
            self.ram = memory::reading();
            self.ram_read = Instant::now();
        }
        ctx.request_repaint_after(if self.busy() { Duration::from_millis(120) } else { Duration::from_secs(1) });

        let p = self.palette();
        egui::Panel::top("toolbar")
            .exact_size(34.0)
            .frame(Frame::new().fill(p.header).inner_margin(Margin::symmetric(8, 2)))
            .show_separator_line(false)
            .show(ui, |ui| self.toolbar(ui, &ctx));
        egui::Panel::bottom("links")
            .frame(Frame::new().fill(p.bg).inner_margin(Margin { left: 12, right: 12, top: 2, bottom: 6 }))
            .show_separator_line(false)
            .show(ui, |ui| self.links(ui));
        egui::Panel::bottom("status")
            .exact_size(34.0)
            .frame(Frame::new().fill(p.header).inner_margin(Margin::symmetric(10, 2)))
            .show_separator_line(false)
            .show(ui, |ui| self.status_strip(ui));
        egui::CentralPanel::default().frame(Frame::new().fill(p.bg).inner_margin(Margin::same(6))).show(ui, |ui| {
            self.tab_bar(ui);
            ui.add_space(6.0);
            match self.tab {
                Tab::Quick => self.quick_tab(ui),
                Tab::Tool(index) => self.tool_tab(ui, index),
                Tab::Log => self.log_tab(ui),
            }
        });
    }
}

/// Seconds as h:mm:ss, which is how a stress run is actually read.
pub fn format_duration(seconds: u64) -> String {
    format!("{}:{:02}:{:02}", seconds / 3600, seconds % 3600 / 60, seconds % 60)
}
