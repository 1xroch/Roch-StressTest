// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! 3DMark 11, opened at its own window (Windows).
//!
//! A looping graphics load -- how an unstable GPU usually fails -- but it
//! verifies no pixel; memtest_vulkan is the one that checks its answers. The
//! command-line runner that could loop it unattended needs the Professional
//! edition, so the window is opened, where Advanced can set a loop count, and
//! the runner holds it to a limit and notices it dying.

use std::path::Path;

use crate::config::{Config, ConfigExt};
use crate::tool::{Field, LaunchSpec, QuickActions, Tool, limit_note, path_text};

const SEARCH_ROOTS: [&str; 3] =
    [r"C:\Program Files\Futuremark\3DMark 11", r"C:\Program Files (x86)\Futuremark\3DMark 11", r"C:\Program Files\Futuremark\3DMark11"];

pub struct ThreeDMark11;

impl Tool for ThreeDMark11 {
    fn key(&self) -> &'static str {
        "3dmark11"
    }

    fn name(&self) -> &'static str {
        "3DMark 11"
    }

    fn blurb(&self) -> &'static str {
        "A looping graphics load: shaders, geometry and the whole board at a game-like duty cycle, which is how an unstable \
         GPU core clock usually gets caught. It checks nothing, so what it finds is a driver reset or a hang rather than a \
         wrong answer. Opens in its own window, where Advanced can set a loop count; the command-line runner that does it \
         unattended needs the Professional edition."
    }

    fn supported(&self) -> bool {
        cfg!(windows)
    }

    /// The application itself, not 3DMarkLauncher.exe: the launcher starts this
    /// and exits at once, so a run supervised through it "finished with no
    /// errors" a second after starting.
    fn exe_globs(&self) -> Vec<&'static str> {
        vec!["3DMark 11*/bin/x64/3DMark11.exe"]
    }

    fn external_globs(&self) -> Vec<String> {
        SEARCH_ROOTS.iter().map(|root| format!(r"{root}\bin\x64\3DMark11.exe")).collect()
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::int(
            "duration",
            "Stop after",
            0,
            0,
            100_000,
            "min",
            "0 lets the loop you set in 3DMark's own window run until you press Stop.",
        )]
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
        let exe =
            self.locate(root).ok_or(r"3DMark 11 was not found. Expected bin\x64\3DMark11.exe under Program Files\Futuremark\3DMark 11.")?;
        Ok(LaunchSpec {
            argv: vec![path_text(&exe)],
            cwd: exe.parent().unwrap_or(root).to_path_buf(),
            error_key: self.key().into(),
            summary: "3DMark 11 (preset and loop set in its own window)".into(),
            duration_seconds: config.int("duration").max(0) as u64 * 60,
            leave_open: true,
            ..Default::default()
        })
    }
}
