// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! Prime95's torture test, opened at its own dialog.
//!
//! Prime95's torture dialog cannot be preset from a file: it opens on Blend
//! whatever prime.txt holds, and resets to Blend every time it is opened, even
//! mid-run. Its four presets are not fixed ranges either -- Large FFTs is worked
//! out from the caches of the processor it finds, so it starts at 957K on one
//! chip and 2048K on another. A tab of FFT sizes could never be shown to agree
//! with what Prime95 was going to do, so the test is chosen in the one place
//! that decides it, and results.txt is watched for a failing worker.
//!
//! `-W` points it at a scratch folder of this program's, so prime.txt,
//! results.txt and the worker save files stay out of the unpacked p95 folder,
//! and results.txt has a stable path to watch.
//!
//! On Linux the same program is mprime, with the same -W and the same files. It
//! asks its questions in a terminal behind a main menu, so "16" -- Options /
//! Torture Test -- is typed for it and it opens at the torture-test questions,
//! as Prime95 opens at its dialog.

use std::path::Path;

use crate::config::Config;
use crate::settings;
use crate::tool::{Field, LaunchSpec, QuickActions, Tool, limit_note, path_text, remove_stale, write_text};

pub struct Prime95;

impl Tool for Prime95 {
    fn key(&self) -> &'static str {
        "prime95"
    }

    fn name(&self) -> &'static str {
        "Prime95"
    }

    fn blurb(&self) -> &'static str {
        "FFT torture test. Smallest and Small FFTs are the heat and power test; Large, Blend and Huge lean on the memory \
         controller and RAM."
    }

    fn exe_globs(&self) -> Vec<&'static str> {
        if cfg!(windows) {
            vec!["p95*/prime95.exe", "prime95*/prime95.exe", "prime95.exe"]
        } else {
            vec!["p95*/mprime", "mprime*/mprime", "mprime"]
        }
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::int("duration", "Stop after", 0, 0, 100_000, "min", "0 lets whatever you chose in Prime95 run until you press Stop.")]
    }

    fn has_tab(&self) -> bool {
        false
    }

    fn quick_actions(&self, root: &Path) -> QuickActions {
        vec![("Open".into(), self.quick_config(root))]
    }

    fn quick_summary(&self, root: &Path) -> String {
        let opens = if cfg!(windows) { "Opens its torture dialog" } else { "Opens its torture-test questions" };
        match limit_note(&self.quick_config(root)) {
            limit if limit.is_empty() => opens.into(),
            limit => format!("{opens}  |  {limit}"),
        }
    }

    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, String> {
        let exe = self.locate(root).ok_or(if cfg!(windows) {
            "prime95.exe was not found. Expected a folder like p95v3019b20.win64 beside this program."
        } else {
            "mprime was not found. Expected a folder like p95v3019b20.linux64 beside this program \
             (packaging/linux/fetch-tools.sh puts it there)."
        })?;
        let work = settings::run_dir("prime95");

        // Four keys and no torture settings. StressTester=1 is the "just stress
        // testing" answer that stops the GIMPS welcome waiting for somebody, and
        // it is what makes Prime95 raise its torture dialog on startup. The
        // other two mark the file as already migrated.
        write_text(&work.join("prime.txt"), "StressTester=1\nUsePrimenet=0\nV24OptionsConverted=1\nWGUID_version=2\n")?;
        // A local.txt would be migrated on top of what was just written, and a
        // results.txt from the last run must not be reported as this one's.
        remove_stale(&work.join("local.txt"));
        let results = work.join("results.txt");
        remove_stale(&results);

        use crate::config::ConfigExt;
        // No -t: that would start a torture test from the file before there was
        // a chance to choose one.
        Ok(LaunchSpec {
            argv: vec![path_text(&exe), format!("-W{}", work.display())],
            cwd: exe.parent().unwrap_or(root).to_path_buf(),
            watch_files: vec![results],
            error_key: self.key().into(),
            summary: if cfg!(windows) {
                "Prime95 (test chosen in its own dialog)"
            } else {
                "Prime95 (mprime; test chosen at its own prompts)"
            }
            .into(),
            duration_seconds: config.int("duration").max(0) as u64 * 60,
            new_console: !cfg!(windows),
            stdin_feed: if cfg!(windows) { String::new() } else { "16\n".into() },
            ..Default::default()
        })
    }
}
