// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! RAM Test Pro, opened at its own window (Windows).
//!
//! It has no command line, no usable settings file and no registry keys; its
//! profile, size and thread count exist only as boxes in its window, so that is
//! where they are set. Failures are read from logs/log.txt beside it: a
//! non-zero "Test errors detected" and its own ERROR lines.

use std::path::Path;

use crate::config::{Config, ConfigExt};
use crate::tool::{Field, LaunchSpec, QuickActions, Tool, limit_note, path_text, remove_stale};

pub struct RamTestPro;

impl Tool for RamTestPro {
    fn key(&self) -> &'static str {
        "ramtest"
    }

    fn name(&self) -> &'static str {
        "RAM Test Pro"
    }

    fn blurb(&self) -> &'static str {
        "A commercial memory test with its own error counters and a stop-on-first-error option. Set the profile, the size \
         and the thread count in its own window."
    }

    fn supported(&self) -> bool {
        cfg!(windows)
    }

    fn exe_globs(&self) -> Vec<&'static str> {
        vec!["ram_test_pro*/**/RAM Test Pro.exe", "**/RAM Test Pro.exe"]
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0, 0, 100_000, "min", "0 lets the run you started in RAM Test Pro finish.")]
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
        let exe = self.locate(root).ok_or("RAM Test Pro.exe was not found. Expected its folder beside this program.")?;
        let folder = exe.parent().unwrap_or(root).to_path_buf();
        let log = folder.join("logs").join("log.txt");
        // git keeps no empty folders, so a fresh clone has no logs folder.
        let _ = std::fs::create_dir_all(folder.join("logs"));
        remove_stale(&log);
        Ok(LaunchSpec {
            argv: vec![path_text(&exe)],
            cwd: folder,
            watch_files: vec![log],
            error_key: self.key().into(),
            summary: "RAM Test Pro (settings entered in its own window)".into(),
            duration_seconds: config.int("duration").max(0) as u64 * 60,
            // No completion or abort pattern: the cycle count and the stops are
            // RAM Test Pro's own business now that its window is where it is run.
            leave_open: true,
            ..Default::default()
        })
    }
}
