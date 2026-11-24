// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cinebench, opened at its own window (Windows): one button per version found.
//!
//! A sustained all-core render reports a score, not a verdict: it does not check
//! its arithmetic the way Prime95 does, so a pass means it finished. Which test
//! a version offers differs between R15 and R26, so the run is started in its
//! window; stdout is still read, because Cinebench prints its result there.
//!
//! BenchMate's copies are deliberately not searched: run outside BenchMate they
//! cannot resolve their string resources, and Cinebench 2024 comes up titled
//! "StrNotFound".

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::{Config, ConfigExt};
use crate::tool::{Field, LaunchSpec, QuickActions, Tool, glob_files, glob_in, path_text};

/// Each version: the label on its button, the folders it is found in, and its
/// executable. Oldest first, so the buttons read in release order.
const VERSIONS: [(&str, &[&str], &str); 7] = [
    ("R11.5", &["CINEBENCH R11.5"], "CINEBENCH Windows 64 Bit.exe"),
    ("R15", &["CINEBENCH R15"], "CINEBENCH Windows 64 Bit.exe"),
    ("R15 Extreme", &["CINEBENCH R15 EXTREME"], "CINEBENCH Windows 64 Bit.exe"),
    ("R20", &["CINEBENCH R20"], "Cinebench.exe"),
    ("R23", &["CINEBENCH R23"], "Cinebench.exe"),
    ("R24", &["CINEBENCH 2024"], "Cinebench.exe"),
    ("R26", &["CINEBENCH 2026"], "Cinebench.exe"),
];

const SEARCH_ROOTS: [&str; 2] = [r"C:\Program Files\Maxon", r"C:\Program Files (x86)\Maxon"];

pub struct Cinebench;

impl Cinebench {
    fn find_installed(folders: &[&str], exe_name: &str) -> Option<PathBuf> {
        for root in SEARCH_ROOTS {
            for folder in folders {
                for pattern in [format!("{folder}/*/{exe_name}"), format!("{folder}/{exe_name}")] {
                    if let Some(found) = glob_files(&glob_in(Path::new(root), &pattern), true).pop() {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    /// Every version present, as (label, path). A copy beside this program wins
    /// over an installed one; both the label and the real folder name are
    /// tried, because Maxon ships 2024 and 2026 in folders that say neither
    /// "R24" nor "R26".
    pub fn installed(&self, root: &Path) -> Vec<(String, PathBuf)> {
        let mut found = Vec::new();
        for (label, folders, exe_name) in VERSIONS {
            let mut local = Vec::new();
            let patterns = std::iter::once(format!("Cinebench*{label}")).chain(folders.iter().map(|folder| folder.to_string()));
            for pattern in patterns {
                local.extend(glob_files(&glob_in(root, &format!("{pattern}/**/{exe_name}")), true));
            }
            local.sort();
            local.dedup();
            if let Some(path) = local.pop().or_else(|| Cinebench::find_installed(folders, exe_name)) {
                found.push((label.to_string(), path));
            }
        }
        found
    }
}

impl Tool for Cinebench {
    fn key(&self) -> &'static str {
        "cinebench"
    }

    fn name(&self) -> &'static str {
        "Cinebench"
    }

    fn blurb(&self) -> &'static str {
        "A sustained all-core render: the load that finds an unstable all-core clock or a cooling limit. It reports a score \
         rather than a verdict -- it does not check its own arithmetic the way Prime95 does -- so a pass here means it \
         finished, and a score that drops between runs is worth as much as the number itself."
    }

    fn supported(&self) -> bool {
        cfg!(windows)
    }

    fn exe_globs(&self) -> Vec<&'static str> {
        Vec::new()
    }

    fn locate(&self, root: &Path) -> Option<PathBuf> {
        self.installed(root).into_iter().next().map(|(_, path)| path)
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0, 0, 100_000, "min", "0 lets it stay open for as many runs as you want.")]
    }

    fn has_tab(&self) -> bool {
        false
    }

    fn quick_actions(&self, root: &Path) -> QuickActions {
        let found = self.installed(root);
        if found.is_empty() {
            return vec![("Start".into(), self.quick_config(root))];
        }
        found.into_iter().map(|(label, _)| (label.clone(), self.quick_config(root).with("version", Value::from(label)))).collect()
    }

    fn quick_summary(&self, root: &Path) -> String {
        let labels: Vec<String> = self.installed(root).into_iter().map(|(label, _)| label).collect();
        if labels.is_empty() { "None found".into() } else { format!("Opens {}", labels.join(" / ")) }
    }

    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, String> {
        let found = self.installed(root);
        let wanted = config.text("version");
        let (label, exe) = found
            .iter()
            .find(|(label, _)| *label == wanted)
            .or_else(|| found.first())
            .cloned()
            .ok_or("No Cinebench installation was found. Expected a folder beside this program, or an installed copy.")?;
        // Opened, not driven; its stdout is read so the score reaches the log.
        // No completion pattern: the first score is not the end of the run.
        Ok(LaunchSpec {
            argv: vec![path_text(&exe)],
            cwd: exe.parent().unwrap_or(root).to_path_buf(),
            console: true,
            no_window: true,
            error_key: self.key().into(),
            summary: format!("Cinebench {label} (run it in its own window)"),
            duration_seconds: config.int("duration").max(0) as u64 * 60,
            leave_open: true,
            ..Default::default()
        })
    }
}
