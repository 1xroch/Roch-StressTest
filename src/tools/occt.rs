// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! OCCT, opened at its own window.
//!
//! The one tool here that covers the whole machine from a single program: an
//! AVX2 CPU test, a memory test, a combined run, a 3D load and a power test that
//! drives CPU and GPU together, each checking its own results. Its command line
//! exists, but the settings that matter differ per test type and the free
//! edition limits an unattended run to an hour, so it is opened rather than
//! driven. OCCT shows its errors as they happen; the runner holds the run to a
//! limit if one was set and notices if it dies. Not bundled: it is someone
//! else's to hand out, and OCCT.exe alone is past GitHub's file limit.

use std::path::Path;

use crate::config::{Config, ConfigExt};
use crate::tool::{Field, LaunchSpec, QuickActions, Tool, limit_note, path_text};

pub struct Occt;

impl Tool for Occt {
    fn key(&self) -> &'static str {
        "occt"
    }

    fn name(&self) -> &'static str {
        "OCCT"
    }

    fn blurb(&self) -> &'static str {
        "CPU, memory, combined and power tests in one program, each checking its own answers. The AVX2 CPU test and the \
         memory test are the two worth reaching for after a clock or timing change; the power test loads CPU and GPU \
         together and is the hardest thing here on a PSU."
    }

    fn exe_globs(&self) -> Vec<&'static str> {
        if cfg!(windows) {
            vec!["OCCT*/OCCT*.exe", "OCCT*.exe"]
        } else {
            vec!["OCCT*/OCCT", "OCCT*/OCCT*.AppImage", "OCCT", "OCCT*.AppImage"]
        }
    }

    fn external_globs(&self) -> Vec<String> {
        if cfg!(windows) {
            vec![r"C:\Program Files\OCCT\OCCT*.exe".into(), r"C:\Program Files (x86)\OCCT\OCCT*.exe".into()]
        } else {
            let home = std::env::var("HOME").unwrap_or_default();
            vec![format!("{home}/Applications/OCCT*.AppImage"), format!("{home}/Applications/OCCT")]
        }
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0, 0, 100_000, "min", "0 lets the test you set up in OCCT run to its own end.")]
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
        let exe = self.locate(root).ok_or(if cfg!(windows) {
            "OCCT.exe was not found. It is a single portable executable -- put it in an OCCT folder beside this program."
        } else {
            "OCCT was not found. Download OCCT for Linux and put it in an OCCT folder beside this program."
        })?;
        #[cfg(unix)]
        {
            // A download arrives without the execute bit.
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&exe).map(|meta| meta.permissions().mode()).unwrap_or(0);
            if mode & 0o111 == 0 {
                std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(mode | 0o755))
                    .map_err(|error| format!("{} is not executable and could not be made so: {error}", exe.display()))?;
            }
        }
        Ok(LaunchSpec {
            argv: vec![path_text(&exe)],
            cwd: exe.parent().unwrap_or(root).to_path_buf(),
            error_key: self.key().into(),
            summary: "OCCT (test chosen in its own window)".into(),
            duration_seconds: config.int("duration").max(0) as u64 * 60,
            leave_open: true,
            ..Default::default()
        })
    }
}
