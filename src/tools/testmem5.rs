// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! TestMem5, opened at its own window (Windows).
//!
//! The profile and the cycles are chosen in TM5's window. It used to be driven
//! from a tab, with the cycle count written into a copy of the .cfg, and that
//! copy is what TM5 then showed -- "Roch active" instead of the profile picked
//! -- and a run given two cycles was seen going into a third. Its documented
//! parameters are only Config File and Minutes, and Minutes is a floor, not a
//! limit, so the window that owns them is where they are set.
//!
//! TM5 0.13.1 appends to Log.txt beside TM5.exe, which is read live, and it
//! never exits when it finishes: completion is read from the log.

use std::path::Path;

use crate::config::{Config, ConfigExt};
use crate::errors;
use crate::tool::{Field, LaunchSpec, QuickActions, Tool, limit_note, path_text, remove_stale};

/// What TM5 opens on when nothing has been picked yet. TM5 remembers its
/// settings in TM5.ini -- the profile as "Config File" and "Test at least,
/// minutes" as "Minutes", where its own infinity button writes 0. Defaults are
/// set by seeding keys that are absent, never by overwriting a choice made in
/// TM5's own window.
const DEFAULT_PROFILE: &str = "1usmus v3 @ 1usmus.cfg";
const DEFAULT_SETTINGS: [(&str, &str); 2] = [("Config File", DEFAULT_PROFILE), ("Minutes", "0")];

pub struct TestMem5;

impl TestMem5 {
    fn seed_defaults(folder: &Path) {
        let ini = folder.join("TM5.ini");
        let text = std::fs::read(&ini).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        let present: Vec<String> =
            lines.iter().filter_map(|line| line.split_once('=').map(|(key, _)| key.trim().to_ascii_lowercase())).collect();
        let profile_exists = folder.join("bin").join(DEFAULT_PROFILE).exists();
        let missing: Vec<(&str, &str)> = DEFAULT_SETTINGS
            .iter()
            .copied()
            .filter(|(key, _)| !present.contains(&key.to_ascii_lowercase()))
            .filter(|(key, _)| profile_exists || *key != "Config File")
            .collect();
        if missing.is_empty() {
            return;
        }
        if !lines.iter().any(|line| line.trim().eq_ignore_ascii_case("[settings]")) {
            lines.insert(0, "[Settings]".into());
        }
        let index = lines.iter().position(|line| line.trim().eq_ignore_ascii_case("[settings]")).unwrap_or(0);
        for (offset, (key, value)) in missing.iter().enumerate() {
            lines.insert(index + 1 + offset, format!("{key}={value}"));
        }
        let _ = std::fs::write(&ini, lines.join("\r\n") + "\r\n");
    }
}

impl Tool for TestMem5 {
    fn key(&self) -> &'static str {
        "testmem5"
    }

    fn name(&self) -> &'static str {
        "TestMem5"
    }

    fn blurb(&self) -> &'static str {
        "The memory test overclockers actually use. The anta777 profiles are the ones worth running: Extreme for a first \
         pass, Absolut when you want to be sure. Pick the profile and the cycles in TM5's own window."
    }

    fn supported(&self) -> bool {
        cfg!(windows)
    }

    fn exe_globs(&self) -> Vec<&'static str> {
        vec!["TestMem5*/TM5.exe", "TM5.exe"]
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0, 0, 100_000, "min", "0 lets the cycles you set in TM5 run to their end.")]
    }

    fn has_tab(&self) -> bool {
        false
    }

    fn quick_actions(&self, root: &Path) -> QuickActions {
        vec![("Open".into(), self.quick_config(root))]
    }

    fn quick_summary(&self, root: &Path) -> String {
        limit_note(&self.quick_config(root))
    }

    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, String> {
        let exe = self.locate(root).ok_or("TM5.exe was not found. Expected a TestMem5 folder beside this program.")?;
        let folder = exe.parent().unwrap_or(root).to_path_buf();
        let log = folder.join("Log.txt");
        // Started fresh: TM5 appends, so every run would inherit the last one's.
        remove_stale(&log);
        TestMem5::seed_defaults(&folder);
        // No arguments: given a Config File it starts that profile at once;
        // given none it opens and waits, which is the point.
        Ok(LaunchSpec {
            argv: vec![path_text(&exe)],
            cwd: folder,
            watch_files: vec![log],
            error_key: self.key().into(),
            summary: "TestMem5 (profile and cycles set in its own window)".into(),
            duration_seconds: config.int("duration").max(0) as u64 * 60,
            completion_patterns: errors::TESTMEM5_COMPLETE.to_vec(),
            // No abort pattern: "Testing stopped by user" is also what it writes
            // when a test is stopped inside its window to change a profile. Now
            // that the window is yours, that is the middle of a run.
            leave_open: true,
            ..Default::default()
        })
    }
}
