// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! memtest_vulkan: the graphics card's memory, tested the way TM5 tests the DIMMs.
//!
//! <https://github.com/GpuZelenograd/memtest_vulkan>
//!
//! The only tool here that tests video memory. An unstable memory overclock on
//! a graphics card shows up as artefacts and driver resets, and none of the CPU
//! tools would ever see it. It writes both to its console and to
//! memtest_vulkan.log beside itself, so it can be shown in its own window and
//! still be watched. It runs until stopped -- the "standard 5-minute test" it
//! announces is the first pass, not the run -- so the time limit is the
//! runner's. It re-executes itself, so stopping it needs the whole tree, which
//! is what the runner does anyway.

use std::path::Path;

use serde_json::Value;

use crate::config::{Config, ConfigExt, config};
use crate::errors;
use crate::tool::{Field, LaunchSpec, Preset, QuickActions, Tool, path_text};

pub struct MemtestVulkan;

/// The three lengths a GPU memory test is actually run for: long enough to
/// catch a bad clock, long enough to trust, and left going.
const QUICK_LENGTHS: [(&str, i64); 3] = [("10 min", 10), ("30 min", 30), ("Infinite", 0)];

impl Tool for MemtestVulkan {
    fn key(&self) -> &'static str {
        "memtest_vulkan"
    }

    fn name(&self) -> &'static str {
        "memtest Vulkan"
    }

    fn blurb(&self) -> &'static str {
        "Video memory, tested through Vulkan. Nothing else here looks at the graphics card at all, and a bad memory \
         overclock on it shows up as artefacts and driver resets rather than as anything the CPU tests would notice."
    }

    fn exe_globs(&self) -> Vec<&'static str> {
        // The exact name on Linux: a pattern would also match the .log it writes
        // beside itself.
        if cfg!(windows) {
            vec!["memtest_vulkan*/memtest_vulkan*.exe", "memtest_vulkan*.exe"]
        } else {
            vec!["memtest_vulkan*/memtest_vulkan", "memtest_vulkan"]
        }
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::int(
                "device",
                "GPU index",
                1,
                1,
                16,
                "",
                "Which device to test, as memtest_vulkan numbers them in its own listing. 1 is the first.",
            ),
            Field::int(
                "memory",
                "Memory to test",
                0,
                0,
                1_048_576,
                "MB",
                "0 lets it size the test from the card's free memory, which is what you usually want.",
            ),
            Field::int(
                "duration",
                "Stop after",
                30,
                0,
                100_000,
                "min",
                "memtest_vulkan runs until it is stopped, so this limit is enforced here. 0 runs until you press Stop.",
            ),
            Field::flag(
                "show_window",
                "Show its window",
                true,
                "Runs it in its own console. Failures are then read from memtest_vulkan.log, which carries the same text.",
            ),
        ]
    }

    fn presets(&self, _root: &Path) -> Vec<Preset> {
        vec![
            Preset::new(
                "Standard",
                config([("device", Value::from(1)), ("memory", Value::from(0))]),
                "The first GPU, with the test sized from its free memory.",
            ),
            Preset::new(
                "Second GPU",
                config([("device", Value::from(2)), ("memory", Value::from(0))]),
                "For a machine with a second card, or an integrated one.",
            ),
            Preset::new("Custom", Config::new(), "Whatever is in the boxes below."),
        ]
    }

    fn quick_preset(&self) -> Option<&'static str> {
        Some("Standard")
    }

    fn quick_values(&self) -> Config {
        config([("duration", Value::from(15))])
    }

    fn quick_actions(&self, root: &Path) -> QuickActions {
        let base = self.quick_config(root);
        QUICK_LENGTHS.iter().map(|(label, minutes)| (label.to_string(), base.with("duration", Value::from(*minutes)))).collect()
    }

    fn quick_summary(&self, root: &Path) -> String {
        format!("{}  |  {}", self.quick_preset_name(root), QUICK_LENGTHS.map(|(label, _)| label).join(" / "))
    }

    fn build(&self, config: &Config, root: &Path) -> Result<LaunchSpec, String> {
        let exe = self.locate(root).ok_or(if cfg!(windows) {
            "memtest_vulkan was not found. Put memtest_vulkan-vX.Y.Z.exe in a memtest_vulkan folder beside this program -- \
             it is a single file, from github.com/GpuZelenograd/memtest_vulkan."
        } else {
            "memtest_vulkan was not found. Expected memtest_vulkan/memtest_vulkan beside this program \
             (packaging/linux/fetch-tools.sh puts it there)."
        })?;
        let folder = exe.parent().unwrap_or(root).to_path_buf();
        // It appends to this across runs; the runner reads only what is added
        // after the run starts, so the card's history is kept.
        let log = folder.join("memtest_vulkan.log");

        // The device index and the size as bare arguments, in that order. With
        // neither it prompts. The size is in bytes, whatever the box is in:
        // given 4096 it answers "requested test size is smaller than minimum
        // wanted" and tests nothing (v0.5.0).
        let device = config.int_or("device", 1);
        let memory = config.int("memory");
        let mut argv = vec![path_text(&exe), device.to_string()];
        if memory > 0 {
            argv.push((memory * 1024 * 1024).to_string());
        }
        let show = config.flag_or("show_window", true);
        Ok(LaunchSpec {
            argv,
            cwd: folder,
            console: !show,
            new_console: show,
            no_window: !show,
            watch_files: if show { vec![log] } else { Vec::new() },
            error_key: self.key().into(),
            summary: format!("memtest_vulkan GPU {device}, {}", if memory > 0 { format!("{memory} MB") } else { "auto size".into() }),
            duration_seconds: config.int("duration").max(0) as u64 * 60,
            // Without these, "no device selected" or an early exit during init
            // leaves a non-zero exit code and nothing to explain it, and the card
            // would be reported faulty when memtest_vulkan never started.
            abort_patterns: errors::MEMTEST_VULKAN_ABORTED.to_vec(),
            ..Default::default()
        })
    }
}
