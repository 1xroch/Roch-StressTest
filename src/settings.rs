// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-user settings and the folders runs write into.
//!
//! Kept in `%LOCALAPPDATA%\RochStressTest` (Linux: `~/.local/share/RochStressTest`)
//! rather than beside the executable, so the program works from a read-only
//! install and a rebuild does not throw the settings away. The settings file is
//! the one the Python builds wrote, so a saved theme carries over.

use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::system;

pub const APP_DIR_NAME: &str = "RochStressTest";

/// The per-user folder this program owns, created on demand.
pub fn app_data_dir() -> PathBuf {
    let path = system::data_home().join(APP_DIR_NAME);
    let _ = std::fs::create_dir_all(&path);
    path
}

/// A scratch folder for one tool's run artifacts. Prime95 insists on writing
/// prime.txt and results.txt somewhere; pointing it here keeps its state out of
/// the distribution folder, which stays exactly as it was unpacked.
pub fn run_dir(name: &str) -> PathBuf {
    let path = app_data_dir().join("runs").join(name);
    let _ = std::fs::create_dir_all(&path);
    path
}

/// Where finished-run transcripts are kept.
pub fn log_dir() -> PathBuf {
    let path = app_data_dir().join("logs");
    let _ = std::fs::create_dir_all(&path);
    path
}

fn settings_path() -> PathBuf {
    app_data_dir().join("settings.json")
}

fn load() -> Map<String, Value> {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

/// Merge one setting into the file, leaving the others alone.
pub fn save_setting(key: &str, value: Value) {
    let mut settings = load();
    settings.insert(key.to_string(), value);
    if let Ok(text) = serde_json::to_string_pretty(&Value::Object(settings)) {
        let _ = std::fs::write(settings_path(), text);
    }
}

/// Dark unless Light was chosen.
pub fn load_dark_mode() -> bool {
    !load().get("appearance_mode").and_then(Value::as_str).is_some_and(|mode| mode.eq_ignore_ascii_case("light"))
}

pub fn save_dark_mode(dark: bool) {
    save_setting("appearance_mode", Value::String(if dark { "Dark" } else { "Light" }.into()));
}
