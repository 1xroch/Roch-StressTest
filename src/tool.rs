// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! What every stress tool has in common, and the shape of a launch.
//!
//! Each tool is a small adapter: it says where its executable lives, what can be
//! configured about it, and how a set of configured values turns into a process
//! to start. Everything after that -- running it, timing it, watching it for
//! failures, stopping it -- is the runner's job and is written once.
//!
//! The adapters are deliberately thin. None reimplements a tool; they write the
//! configuration file or command line the tool already understands.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::{Config, ConfigExt, as_int};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Int,
    Text,
    Bool,
    Choice,
    /// Any combination of the choices, written space-separated in choice order.
    Multi,
}

/// One configurable setting, and how to draw it. A field is all the window
/// needs to build a row, so adding a setting means adding a Field and reading
/// its key in `build` -- never touching the window code.
#[derive(Clone, Debug)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub default: Value,
    pub choices: Vec<String>,
    pub hint: String,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
    pub unit: &'static str,
}

impl Field {
    pub fn int(key: &'static str, label: &'static str, default: i64, minimum: i64, maximum: i64, unit: &'static str, hint: &str) -> Field {
        Field {
            key,
            label,
            kind: Kind::Int,
            default: Value::from(default),
            choices: Vec::new(),
            hint: hint.to_string(),
            minimum: Some(minimum),
            maximum: Some(maximum),
            unit,
        }
    }

    pub fn flag(key: &'static str, label: &'static str, default: bool, hint: &str) -> Field {
        Field { kind: Kind::Bool, default: Value::Bool(default), ..Field::text(key, label, "", hint) }
    }

    pub fn text(key: &'static str, label: &'static str, default: &str, hint: &str) -> Field {
        Field {
            key,
            label,
            kind: Kind::Text,
            default: Value::String(default.to_string()),
            choices: Vec::new(),
            hint: hint.to_string(),
            minimum: None,
            maximum: None,
            unit: "",
        }
    }

    pub fn choice(key: &'static str, label: &'static str, default: &str, choices: &[&str], hint: &str) -> Field {
        Field { kind: Kind::Choice, choices: choices.iter().map(|c| c.to_string()).collect(), ..Field::text(key, label, default, hint) }
    }

    pub fn multi(key: &'static str, label: &'static str, default: &str, choices: &[&str], hint: &str) -> Field {
        Field { kind: Kind::Multi, ..Field::choice(key, label, default, choices, hint) }
    }

    /// Turn what is in a box into the value `build` expects. Out-of-range and
    /// unparseable values fall back to the default rather than failing: a typo
    /// in a memory box should not stop a run about to be left for six hours.
    pub fn coerce(&self, raw: &Value) -> Value {
        match self.kind {
            Kind::Int => {
                let Some(mut value) = as_int(raw).filter(|_| !matches!(raw, Value::Bool(_))) else {
                    return self.default.clone();
                };
                if let Some(minimum) = self.minimum {
                    value = value.max(minimum);
                }
                if let Some(maximum) = self.maximum {
                    value = value.min(maximum);
                }
                Value::from(value)
            }
            Kind::Bool => {
                let mut map = Config::new();
                map.insert("v".into(), raw.clone());
                Value::Bool(map.flag("v"))
            }
            Kind::Multi => {
                let text = match raw {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                let wanted: Vec<&str> = text.split([',', ' ']).filter(|part| !part.is_empty()).collect();
                Value::String(self.choices.iter().filter(|c| wanted.contains(&c.as_str())).cloned().collect::<Vec<_>>().join(" "))
            }
            Kind::Choice => match raw {
                Value::String(text) if self.choices.contains(text) => raw.clone(),
                _ => self.default.clone(),
            },
            Kind::Text => match raw {
                Value::String(_) => raw.clone(),
                Value::Null => Value::String(String::new()),
                other => Value::String(other.to_string()),
            },
        }
    }
}

/// A named set of field values, and why you would pick it.
#[derive(Clone, Debug)]
pub struct Preset {
    pub name: String,
    pub values: Config,
    pub description: String,
}

impl Preset {
    pub fn new(name: &str, values: Config, description: &str) -> Preset {
        Preset { name: name.into(), values, description: description.into() }
    }
}

/// Everything the runner needs to start and supervise one test.
#[derive(Clone, Debug, Default)]
pub struct LaunchSpec {
    pub argv: Vec<String>,
    /// Windows only: the arguments after `argv[0]` as one raw string, used
    /// instead of the rest of `argv`, for the one launch (Linpack Extended
    /// through cmd) whose quoting no argument list survives.
    pub cmdline: Option<String>,
    pub cwd: PathBuf,
    /// Variables added to this program's environment for the child.
    pub env: Vec<(String, String)>,
    /// True when the child writes to a pipe that is read. Tools shown in a
    /// window of their own are watched through their log files instead.
    pub console: bool,
    /// Shown in a window of its own: a new console on Windows, a terminal on
    /// Linux. Its output then goes there, so it is watched through files.
    pub new_console: bool,
    /// Windows: started with no console window at all (piped tools).
    pub no_window: bool,
    pub watch_files: Vec<PathBuf>,
    /// Which failure patterns apply (see `errors::scan`).
    pub error_key: String,
    pub summary: String,
    /// 0 means until stopped. Enforced by the runner, because most of these
    /// tools cannot limit themselves.
    pub duration_seconds: u64,
    /// Lines that mean the tool has finished, for tools that do not exit when
    /// done (TM5 leaves its window open forever).
    pub completion_patterns: Vec<&'static str>,
    /// Lines that mean it stopped without finishing.
    pub abort_patterns: Vec<&'static str>,
    /// Leave the tool on screen once it has finished by itself, so a pause:1
    /// result or TM5's summary can be read. A time limit or a stop still stops it.
    pub leave_open: bool,
    /// Linux only: text typed into the tool before its input is handed to the
    /// keyboard in its terminal. mprime is taken to its torture-test questions
    /// this way.
    pub stdin_feed: String,
}

impl LaunchSpec {
    /// The command as it would be typed, for the log.
    pub fn command_text(&self) -> String {
        match &self.cmdline {
            Some(raw) => format!("{} {raw}", self.argv.first().map(String::as_str).unwrap_or("")),
            None => self.argv.join(" "),
        }
    }
}

/// What a tool's own window is for: there is no tab when it is configured there.
pub type QuickActions = Vec<(String, Config)>;

/// A stress tool adapter.
pub trait Tool: Send + Sync {
    fn key(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn blurb(&self) -> &'static str;

    /// Whether this tool has a build for the system this is running on. A
    /// Windows-only tool is left out of the window on Linux entirely, rather
    /// than shown as "not found": there is nothing to download that would run.
    fn supported(&self) -> bool {
        true
    }

    /// Relative to the tools root: the first path that exists wins, so a version
    /// bump that renames the folder only needs a pattern added.
    fn exe_globs(&self) -> Vec<&'static str>;

    /// Absolute patterns searched after the tools root, for tools that are
    /// installed rather than unpacked. Newest (last sorted) wins.
    fn external_globs(&self) -> Vec<String> {
        Vec::new()
    }

    fn locate(&self, root: &Path) -> Option<PathBuf> {
        for pattern in self.exe_globs() {
            if let Some(found) = glob_files(&glob_in(root, pattern), true).into_iter().next() {
                return Some(found);
            }
        }
        for pattern in self.external_globs() {
            if let Some(found) = glob_files(&pattern, true).pop() {
                return Some(found);
            }
        }
        None
    }

    fn available(&self, root: &Path) -> bool {
        self.locate(root).is_some()
    }

    /// Why this tool cannot run on this machine, or "" when it can. Separate
    /// from `available`: a tool can be present and the wrong one for the CPU.
    fn unsupported_reason(&self, _root: &Path) -> String {
        String::new()
    }

    fn fields(&self) -> Vec<Field>;

    fn presets(&self, _root: &Path) -> Vec<Preset> {
        Vec::new()
    }

    /// The preset Quick Start and the tab open on, if any.
    fn quick_preset(&self) -> Option<&'static str> {
        None
    }

    /// Values that override the preset for Quick Start and the tab's defaults.
    fn quick_values(&self) -> Config {
        Config::new()
    }

    /// False for a tool configured in its own window: a tab of settings that do
    /// not reach it would be a tab that lies.
    fn has_tab(&self) -> bool {
        true
    }

    fn defaults(&self) -> Config {
        self.fields().into_iter().map(|field| (field.key.to_string(), field.default)).collect()
    }

    fn quick_preset_name(&self, root: &Path) -> String {
        let names: Vec<String> = self.presets(root).into_iter().map(|preset| preset.name).collect();
        match self.quick_preset() {
            Some(wanted) if names.iter().any(|name| name == wanted) => wanted.to_string(),
            _ => names.into_iter().next().unwrap_or_default(),
        }
    }

    /// The default configuration: field defaults, then the named preset, then
    /// the quick-start overrides, in that order so an override always wins.
    fn quick_config(&self, root: &Path) -> Config {
        let mut config = self.defaults();
        let name = self.quick_preset_name(root);
        if let Some(preset) = self.presets(root).into_iter().find(|preset| preset.name == name) {
            config.extend(preset.values);
        }
        config.extend(self.quick_values());
        config
    }

    /// The Quick Start buttons, as (label, config) pairs.
    fn quick_actions(&self, root: &Path) -> QuickActions {
        vec![("Start".into(), self.quick_config(root))]
    }

    /// One line describing the default, with no side effects: building a spec
    /// writes configuration files, and Quick Start describes every tool the
    /// moment the window opens.
    fn quick_summary(&self, root: &Path) -> String {
        let config = self.quick_config(root);
        [self.quick_preset_name(root), duration_note(&config)].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("  |  ")
    }

    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, String>;

    // ----- profiles: config files the tool keeps, made and edited on its tab -----

    /// None for a tool without saved profiles.
    fn profiles(&self, _root: &Path) -> Option<Vec<(String, PathBuf)>> {
        None
    }

    fn profile_values(&self, _path: &Path) -> Result<Config, String> {
        Err("This tool has no profiles.".into())
    }

    fn save_profile(&self, _root: &Path, _name: &str, _config: &Config) -> Result<PathBuf, String> {
        Err("This tool has no profiles.".into())
    }

    fn profile_hint(&self) -> String {
        String::new()
    }

    /// What a saved profile will contain, shown as it is edited.
    fn profile_preview(&self, _config: &Config) -> String {
        String::new()
    }

    /// A title for a multi-line preview shown in a section of its own; None
    /// puts a one-line preview under the profile name.
    fn profile_preview_label(&self) -> Option<&'static str> {
        None
    }

    /// Linpack Extended's chain of tests, edited as rows on its tab.
    fn has_test_chain(&self) -> bool {
        false
    }
}

/// What a card should say about how a run ends.
pub fn duration_note(config: &Config) -> String {
    match config.int("duration") {
        0 => "no time limit".into(),
        minutes => format!("{minutes} min"),
    }
}

/// "N min", or nothing when there is no limit.
pub fn limit_note(config: &Config) -> String {
    match config.int("duration") {
        0 => String::new(),
        minutes => format!("{minutes} min"),
    }
}

/// A pattern for files under `folder`, whose own name is taken literally: a
/// folder called "[test]" must not be read as a character class.
pub fn glob_in(folder: &Path, pattern: &str) -> String {
    format!("{}/{pattern}", glob::Pattern::escape(&folder.to_string_lossy()))
}

/// Files matching a glob pattern, sorted.
pub fn glob_files(pattern: &str, files_only: bool) -> Vec<PathBuf> {
    let options = glob::MatchOptions { case_sensitive: !cfg!(windows), ..Default::default() };
    let mut found: Vec<PathBuf> = glob::glob_with(pattern, options)
        .map(|paths| paths.flatten().filter(|path| !files_only || path.is_file()).collect())
        .unwrap_or_default();
    found.sort();
    found
}

/// A profile name safe as a file name on either system, or "".
pub fn clean_profile_name(name: &str) -> String {
    let kept: String =
        name.chars().filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') && !c.is_control()).collect();
    kept.trim().trim_matches('.').trim().to_string()
}

/// Whether this program can create files in `folder`.
pub fn writable(folder: &Path) -> bool {
    let probe = folder.join(format!(".roch-stresstest-write-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Remove a file if it is there; a run's leftover log must never be mistaken
/// for this run's.
pub fn remove_stale(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Write a text file with the line endings the system's tools expect.
pub fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let body = if cfg!(windows) { text.replace("\r\n", "\n").replace('\n', "\r\n") } else { text.to_string() };
    std::fs::write(path, body).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn path_text(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_coerced_not_rejected() {
        let field = Field::int("memory", "Memory", 28, 0, 1024, "GB", "");
        assert_eq!(field.coerce(&Value::String("4000".into())), Value::from(1024));
        assert_eq!(field.coerce(&Value::String("x".into())), Value::from(28));
        let multi = Field::multi("algorithms", "Algorithms", "VSTv3", &["BKT", "SFTv4", "VSTv3"], "");
        assert_eq!(multi.coerce(&Value::String("VSTv3,BKT".into())), Value::String("BKT VSTv3".into()));
        assert_eq!(clean_profile_name(" vt3: 28gb? "), "vt3 28gb");
    }
}
